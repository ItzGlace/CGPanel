use super::*;
use cgpanel::outbound;
use reqwest::{
    multipart::{Form, Part},
    Url,
};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

async fn telegram_document(
    reg: &Registry,
    tenant: &str,
    item: &Item,
    name: &str,
    bytes: Vec<u8>,
) -> Result<()> {
    let url = Url::parse(&format!(
        "https://api.telegram.org/bot{}/sendDocument",
        s(&item.data, "token")
    ))?;
    let proxy = super::integrations_agent::proxy(reg, tenant, item)?;
    for attempt in 0..3 {
        let client = outbound::client(&url, proxy.as_deref(), 180).await?;
        let form = Form::new()
            .text("chat_id", s(&item.data, "chat_id").to_owned())
            .part(
                "document",
                Part::bytes(bytes.clone()).file_name(name.to_string()),
            );
        if let Ok(response) = client.post(url.clone()).multipart(form).send().await {
            let code = response.status();
            let body = outbound::bounded_body(response, 65536).await?;
            let value: Value = serde_json::from_slice(&body).unwrap_or(json!({}));
            if code.is_success() && value["ok"] == true {
                return Ok(());
            }
            if code.as_u16() == 429 {
                tokio::time::sleep(Duration::from_secs(
                    value["parameters"]["retry_after"]
                        .as_u64()
                        .unwrap_or(5)
                        .min(60),
                ))
                .await;
                continue;
            }
            if code.is_client_error() {
                bail!("Telegram rejected the document; check the bot's document permissions and chat ID");
            }
        }
        if attempt < 2 {
            tokio::time::sleep(Duration::from_secs(2u64.pow(attempt + 1))).await;
        }
    }
    bail!("Telegram document delivery failed after retries")
}
async fn telegram_upload(
    reg: &Registry,
    tenant: &str,
    item: &Item,
    path: &Path,
    id: &str,
) -> Result<()> {
    const CHUNK: usize = 45 * 1024 * 1024;
    let size = tokio::fs::metadata(path).await?.len();
    let split = size > CHUNK as u64;
    let mut file = tokio::fs::File::open(path).await?;
    let mut parts = Vec::new();
    let mut number = 1;
    let mut all = Sha256::new();
    loop {
        let mut bytes = vec![0u8; CHUNK];
        let mut length = 0;
        while length < CHUNK {
            let n = file.read(&mut bytes[length..]).await?;
            if n == 0 {
                break;
            }
            length += n;
        }
        if length == 0 {
            break;
        }
        bytes.truncate(length);
        all.update(&bytes);
        let name = if split {
            format!("{id}.cgp.part{number:04}")
        } else {
            format!("{id}.cgp")
        };
        parts.push(
            json!({"name":name,"bytes":length,"sha256":format!("{:x}",Sha256::digest(&bytes))}),
        );
        telegram_document(reg, tenant, item, &name, bytes).await?;
        number += 1;
        if split {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    if split {
        let manifest = json!({"format":"CGPanel split backup","filename":format!("{id}.cgp"),"bytes":size,"sha256":format!("{:x}",all.finalize()),"parts":parts,"reassembly":"Concatenate parts in the listed order as binary bytes, then verify the SHA-256 before opening the .cgp ZIP."});
        telegram_document(
            reg,
            tenant,
            item,
            &format!("{id}.cgp.parts.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )
        .await?;
    }
    Ok(())
}
async fn ssh_upload(
    reg: &Registry,
    tenant: &str,
    item: &Item,
    path: &Path,
    id: &str,
) -> Result<()> {
    let port = item.data["port"].as_u64().unwrap_or(22) as u16;
    let ip = outbound::addresses(s(&item.data, "host"), port).await?[0].ip();
    let folder = format!("{ROOT}/transfers/{}", uuid::Uuid::new_v4().simple());
    tokio::fs::create_dir_all(&folder).await?;
    atomic(
        &format!("{folder}/key"),
        s(&item.data, "private_key"),
        0o600,
    )
    .await?;
    let host_label = if port == 22 {
        ip.to_string()
    } else {
        format!("[{ip}]:{port}")
    };
    atomic(
        &format!("{folder}/known_hosts"),
        &format!("{host_label} {}\n", s(&item.data, "host_key")),
        0o600,
    )
    .await?;
    let mut config=format!("[destination]\ntype = sftp\nhost = {ip}\nport = {port}\nuser = {}\nkey_file = {folder}/key\nknown_hosts_file = {folder}/known_hosts\ndisable_hashcheck = true\n",s(&item.data,"username"));
    let key_type = s(&item.data, "host_key")
        .split_whitespace()
        .next()
        .context("Missing pinned host key")?;
    let algorithms = if key_type == "ssh-rsa" {
        "rsa-sha2-512 rsa-sha2-256"
    } else {
        key_type
    };
    config.push_str(&format!("host_key_algorithms = {algorithms}\n"));
    if let Some(proxy) = super::integrations_agent::proxy(reg, tenant, item)? {
        let proxy = outbound::proxy_url(&proxy).await?;
        config.push_str(&format!(
            "socks_proxy = {}\n",
            proxy.trim_start_matches("socks5h://").trim_end_matches('/')
        ));
    }
    atomic(&format!("{folder}/rclone.conf"), &config, 0o600).await?;
    let target = format!(
        "destination:{}/{}.cgp",
        s(&item.data, "remote_dir").trim_end_matches('/'),
        id
    );
    let result = exec(
        "/usr/local/lib/cgpanel/rclone",
        args(&[
            "copyto",
            path.to_str().context("Invalid archive path")?,
            &target,
            "--config",
            &format!("{folder}/rclone.conf"),
            "--retries",
            "3",
            "--low-level-retries",
            "3",
            "--timeout",
            "2m",
            "--contimeout",
            "20s",
            "--stats",
            "0",
            "--log-level",
            "ERROR",
        ]),
        None,
        1800,
    )
    .await;
    let _ = tokio::fs::remove_dir_all(&folder).await;
    result.map_err(|error|{
        let mut detail=error.to_string();
        for key in ["private_key","url"]{let secret=s(&item.data,key);if !secret.is_empty(){detail=detail.replace(secret,"[redacted]");}}
        if let Ok(Some(proxy))=super::integrations_agent::proxy(reg,tenant,item){detail=detail.replace(&proxy,"[redacted]");if let Ok(url)=Url::parse(&proxy){if let Some(secret)=url.password(){detail=detail.replace(secret,"[redacted]");}}}
        tracing::warn!(detail=%detail,"SFTP backup transfer failed");
        anyhow!("SSH/SFTP upload failed; check destination access, the pinned host key, and proxy settings")
    })?;
    Ok(())
}
pub async fn upload(
    reg: &Registry,
    tenant: &str,
    integration: &str,
    path: &Path,
    id: &str,
) -> Result<Value> {
    let item = reference(reg, integration, tenant, "integrations")?;
    match s(&item.data, "type") {
        "telegram" => telegram_upload(reg, tenant, item, path, id).await?,
        "s3" => {
            let proxy = super::integrations_agent::proxy(reg, tenant, item)?;
            let target = cgpanel::s3_upload::Target {
                endpoint: s(&item.data, "endpoint"),
                bucket: s(&item.data, "bucket"),
                region: s(&item.data, "region"),
                access: s(&item.data, "access_key"),
                secret: s(&item.data, "secret_key"),
                proxy: proxy.as_deref(),
                ca: Some(s(&item.data, "ca_pem")),
            };
            let prefix = s(&item.data, "prefix").trim_matches('/');
            let key = if prefix.is_empty() {
                format!("{id}.cgp")
            } else {
                format!("{prefix}/{id}.cgp")
            };
            target.upload(&key, path).await?;
        }
        "ssh" => ssh_upload(reg, tenant, item, path, id).await?,
        _ => bail!("Choose Telegram, S3, or SSH storage"),
    }
    Ok(json!({"delivered":true,"destination":integration}))
}
pub async fn test(reg: &Registry, op: &Operation) -> Result<Value> {
    let item = owned(reg, op, "integrations")?;
    match s(&item.data, "type") {
        "telegram" => {
            super::integrations_agent::telegram(
                reg,
                &op.tenant,
                &op.id,
                "CGPanel connection test: this chat is ready to receive your website alerts.",
            )
            .await
        }
        "proxy" => {
            let url = Url::parse("https://api.myip.com")?;
            let response = outbound::client(&url, Some(s(&item.data, "url")), 25)
                .await?
                .get(url)
                .send()
                .await
                .map_err(|_| anyhow!("Proxy test failed"))?;
            ensure!(
                response.status().is_success(),
                "IP check service returned an error"
            );
            let bytes = outbound::bounded_body(response, 16384).await?;
            let value: Value = serde_json::from_slice(&bytes)?;
            Ok(json!({"connected":true,"exit_ip":value["ip"]}))
        }
        "cloudflare" => {
            let url = Url::parse("https://api.cloudflare.com/client/v4/user/tokens/verify")?;
            let response = outbound::client(&url, None, 25)
                .await?
                .get(url)
                .bearer_auth(s(&item.data, "token"))
                .send()
                .await
                .map_err(|_| anyhow!("Cloudflare connection failed"))?;
            let bytes = outbound::bounded_body(response, 65536).await?;
            let value: Value = serde_json::from_slice(&bytes)?;
            ensure!(
                value["success"] == true && value["result"]["status"] == "active",
                "Cloudflare token is not active"
            );
            Ok(json!({"active":true}))
        }
        "s3" | "ssh" => {
            let id = uuid::Uuid::new_v4().simple().to_string();
            let path = format!("{ROOT}/backups/test-{id}.cgp");
            {
                let file = std::fs::File::create(&path)?;
                let mut zip = zip::ZipWriter::new(file);
                zip.start_file(
                    "connection-test.txt",
                    zip::write::SimpleFileOptions::default(),
                )?;
                std::io::Write::write_all(
                    &mut zip,
                    b"CGPanel storage connection test. This object may be removed.\n",
                )?;
                zip.finish()?;
            }
            let result = upload(
                reg,
                &op.tenant,
                &op.id,
                Path::new(&path),
                &format!("connection-test-{id}"),
            )
            .await;
            let _ = tokio::fs::remove_file(path).await;
            result
        }
        _ => bail!("Unsupported integration test"),
    }
}
