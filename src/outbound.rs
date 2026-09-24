use anyhow::{ensure, Context, Result};
use reqwest::{Client, Url};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

/// Connectors may not turn the privileged service into a private-network proxy.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_unspecified()
                || a == 0
                || a >= 240
                || (a == 100 && (64..=127).contains(&b))
                || (a == 192 && b == 0 && (c == 0 || c == 2))
                || (a == 198 && (b == 18 || b == 19 || b == 51))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(v4));
            }
            let s = ip.segments();
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && (s[1] == 0xdb8 || s[1] == 0 || s[1] == 2))
                && s[0] != 0x2002 // Do not admit IPv4-embedded transition networks.
        }
    }
}
pub async fn addresses(host: &str, port: u16) -> Result<Vec<SocketAddr>> {
    let result: Vec<_> = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .context("DNS lookup timed out")?
    .context("DNS lookup failed")?
    .collect();
    ensure!(
        !result.is_empty() && result.iter().all(|a| public_ip(a.ip())),
        "Destination must resolve only to public IP addresses"
    );
    Ok(result)
}
pub async fn proxy_url(value: &str) -> Result<String> {
    let mut url = Url::parse(value).context("Invalid proxy URL")?;
    ensure!(
        ["socks5", "socks5h"].contains(&url.scheme())
            && url.port().is_some()
            && ["", "/"].contains(&url.path())
            && url.query().is_none()
            && url.fragment().is_none(),
        "Use socks5h://user:password@host:port"
    );
    let host = url
        .host_str()
        .context("Proxy hostname is required")?
        .to_owned();
    let ips = addresses(&host, url.port().unwrap()).await?;
    url.set_ip_host(ips[0].ip())
        .map_err(|_| anyhow::anyhow!("Invalid proxy address"))?;
    url.set_scheme("socks5h")
        .map_err(|_| anyhow::anyhow!("Invalid proxy scheme"))?;
    Ok(url.to_string())
}
pub async fn client(url: &Url, proxy: Option<&str>, seconds: u64) -> Result<Client> {
    client_with_ca(url, proxy, seconds, None).await
}
pub async fn client_with_ca(
    url: &Url,
    proxy: Option<&str>,
    seconds: u64,
    ca: Option<&str>,
) -> Result<Client> {
    ensure!(
        ["http", "https"].contains(&url.scheme())
            && url.username().is_empty()
            && url.password().is_none(),
        "Only HTTP(S) URLs without embedded credentials are allowed"
    );
    let host = url.host_str().context("URL hostname is required")?;
    let ips = addresses(host, url.port_or_known_default().context("Missing port")?).await?;
    let mut builder = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(seconds))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("CGPanel/0.2 (+self-hosted monitoring)")
        .resolve_to_addrs(host, &ips);
    if let Some(proxy) = proxy {
        builder = builder
            .proxy(reqwest::Proxy::all(proxy_url(proxy).await?).context("Invalid SOCKS proxy")?);
    }
    if let Some(ca) = ca.filter(|v| !v.is_empty()) {
        ensure!(ca.len() <= 20000, "CA certificate is too large");
        builder = builder.add_root_certificate(
            reqwest::Certificate::from_pem(ca.as_bytes()).context("Invalid CA certificate")?,
        );
    }
    builder.build().context("Cannot build HTTP client")
}
pub async fn bounded_body(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("Reading HTTP response failed")?
    {
        ensure!(
            data.len() + chunk.len() <= limit,
            "HTTP response exceeds size limit"
        );
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deny_local_and_metadata_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "169.254.169.254",
            "100.64.1.2",
            "192.168.1.1",
            "::1",
            "fc00::1",
            "::ffff:127.0.0.1",
            "2002:7f00:1::",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        assert!(public_ip("1.1.1.1".parse().unwrap()));
    }
}
