use anyhow::{ensure, Context, Result};
use hmac::{Hmac, Mac};
use reqwest::{Method, Url};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
type HmacSha256 = Hmac<Sha256>;
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn hash(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
fn mac(key: &[u8], message: &str) -> Vec<u8> {
    let mut h = HmacSha256::new_from_slice(key).expect("HMAC accepts any key");
    h.update(message.as_bytes());
    h.finalize().into_bytes().to_vec()
}
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn tag(body: &str, name: &str) -> Result<String> {
    let mut reader = quick_xml::Reader::from_str(body);
    loop {
        match reader.read_event()? {
            quick_xml::events::Event::Start(e) if e.local_name().as_ref() == name.as_bytes() => {
                return Ok(quick_xml::escape::unescape(&reader.read_text(e.name())?)?.into_owned())
            }
            quick_xml::events::Event::Eof => anyhow::bail!("S3 response is missing {name}"),
            _ => {}
        }
    }
}
pub struct Target<'a> {
    pub endpoint: &'a str,
    pub bucket: &'a str,
    pub region: &'a str,
    pub access: &'a str,
    pub secret: &'a str,
    pub proxy: Option<&'a str>,
    pub ca: Option<&'a str>,
}
impl Target<'_> {
    async fn request(
        &self,
        method: Method,
        url: Url,
        body: Vec<u8>,
    ) -> Result<(u16, reqwest::header::HeaderMap, Vec<u8>)> {
        let now = chrono::Utc::now();
        let date = now.format("%Y%m%d").to_string();
        let amz = now.format("%Y%m%dT%H%M%SZ").to_string();
        let payload = hash(&body);
        let host = match url.port() {
            Some(port) => format!("{}:{port}", url.host_str().context("Invalid S3 host")?),
            None => url.host_str().context("Invalid S3 host")?.to_string(),
        };
        let mut pairs: Vec<_> = url
            .query_pairs()
            .map(|(k, v)| (encode(&k), encode(&v)))
            .collect();
        pairs.sort();
        let query = pairs
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        let headers = format!("host:{host}\nx-amz-content-sha256:{payload}\nx-amz-date:{amz}\n");
        let signed = "host;x-amz-content-sha256;x-amz-date";
        let canonical = format!(
            "{}\n{}\n{query}\n{headers}\n{signed}\n{payload}",
            method.as_str(),
            url.path()
        );
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let input = format!(
            "AWS4-HMAC-SHA256\n{amz}\n{scope}\n{}",
            hash(canonical.as_bytes())
        );
        let key = mac(
            &mac(
                &mac(
                    &mac(format!("AWS4{}", self.secret).as_bytes(), &date),
                    self.region,
                ),
                "s3",
            ),
            "aws4_request",
        );
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={}",
            self.access,
            hex(&mac(&key, &input))
        );
        let client = crate::outbound::client_with_ca(&url, self.proxy, 120, self.ca).await?;
        let response = client
            .request(method, url)
            .header("x-amz-date", amz)
            .header("x-amz-content-sha256", payload)
            .header("authorization", authorization)
            .body(body)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("S3 connection failed"))?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = crate::outbound::bounded_body(response, 1024 * 1024).await?;
        Ok((status, headers, body))
    }
    pub async fn upload(&self, key: &str, path: &std::path::Path) -> Result<()> {
        let mut url = Url::parse(self.endpoint)?;
        url.set_path(&format!("/{}/{key}", self.bucket));
        let mut init = url.clone();
        init.query_pairs_mut().append_pair("uploads", "");
        let (status, _, body) = self.request(Method::POST, init, Vec::new()).await?;
        ensure!(
            (200..300).contains(&status),
            "S3 multipart initiation failed (HTTP {status})"
        );
        let upload = tag(&String::from_utf8_lossy(&body), "UploadId")?;
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1800),
            self.parts(&url, &upload, path),
        )
        .await;
        match result {
            Ok(Ok(())) => Ok(()),
            other => {
                let mut abort = url;
                abort.query_pairs_mut().append_pair("uploadId", &upload);
                let _ = self.request(Method::DELETE, abort, Vec::new()).await;
                match other {
                    Ok(Err(error)) => Err(error),
                    _ => anyhow::bail!("S3 upload exceeded 30 minutes"),
                }
            }
        }
    }
    async fn parts(&self, url: &Url, upload: &str, path: &std::path::Path) -> Result<()> {
        let mut file = tokio::fs::File::open(path).await?;
        let mut parts = String::from("<CompleteMultipartUpload>");
        let mut number = 1;
        loop {
            let mut bytes = vec![0u8; 8 * 1024 * 1024];
            let mut read = 0;
            while read < bytes.len() {
                let n = file.read(&mut bytes[read..]).await?;
                if n == 0 {
                    break;
                }
                read += n;
            }
            if read == 0 && number > 1 {
                break;
            }
            bytes.truncate(read);
            let mut target = url.clone();
            target
                .query_pairs_mut()
                .append_pair("partNumber", &number.to_string())
                .append_pair("uploadId", upload);
            let mut etag = None;
            for attempt in 0..3 {
                if let Ok((status, headers, _)) = self
                    .request(Method::PUT, target.clone(), bytes.clone())
                    .await
                {
                    if (200..300).contains(&status) {
                        etag = headers
                            .get("etag")
                            .and_then(|h| h.to_str().ok())
                            .map(str::to_owned);
                        break;
                    }
                }
                if attempt < 2 {
                    tokio::time::sleep(std::time::Duration::from_secs(2u64.pow(attempt + 1))).await;
                }
            }
            let etag = etag.context("S3 part upload failed after retries")?;
            parts.push_str(&format!(
                "<Part><PartNumber>{number}</PartNumber><ETag>{}</ETag></Part>",
                quick_xml::escape::escape(&etag)
            ));
            number += 1;
            ensure!(number <= 10001, "Too many S3 upload parts");
            if read < 8 * 1024 * 1024 {
                break;
            }
        }
        parts.push_str("</CompleteMultipartUpload>");
        let mut complete = url.clone();
        complete.query_pairs_mut().append_pair("uploadId", upload);
        let (status, _, body) = self
            .request(Method::POST, complete, parts.into_bytes())
            .await?;
        ensure!(
            (200..300).contains(&status) && tag(&String::from_utf8_lossy(&body), "ETag").is_ok(),
            "S3 multipart completion failed"
        );
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn encoding_and_xml() {
        assert_eq!(super::encode("a b/+~"), "a%20b%2F%2B~");
        assert_eq!(
            super::tag("<Result><UploadId>a&amp;b</UploadId></Result>", "UploadId").unwrap(),
            "a&b"
        );
    }
}
