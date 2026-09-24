//! Small SOCKS5 CONNECT transport shared by the egress gateway and SSH transport.
use anyhow::{ensure, Context, Result};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub async fn connect(proxy: &str, host: &str, port: u16) -> Result<TcpStream> {
    tokio::time::timeout(Duration::from_secs(20), handshake(proxy, host, port))
        .await
        .context("SOCKS connection timed out")?
}
async fn handshake(proxy: &str, host: &str, port: u16) -> Result<TcpStream> {
    let url = reqwest::Url::parse(proxy)?;
    ensure!(
        ["socks5", "socks5h"].contains(&url.scheme()),
        "SOCKS5 proxy required"
    );
    let mut stream = TcpStream::connect((
        url.host_str().context("Missing proxy host")?,
        url.port().context("Missing proxy port")?,
    ))
    .await
    .context("Cannot reach SOCKS server")?;
    let username = percent_encoding::percent_decode_str(url.username()).decode_utf8()?;
    let password =
        percent_encoding::percent_decode_str(url.password().unwrap_or("")).decode_utf8()?;
    ensure!(
        username.len() <= 255 && password.len() <= 255,
        "SOCKS credentials are too long"
    );
    let auth = !username.is_empty();
    stream.write_all(&[5, 1, if auth { 2 } else { 0 }]).await?;
    let mut reply = [0u8; 2];
    stream.read_exact(&mut reply).await?;
    ensure!(
        reply == [5, if auth { 2 } else { 0 }],
        "SOCKS authentication method rejected"
    );
    if auth {
        let mut request = vec![1, username.len() as u8];
        request.extend(username.as_bytes());
        request.push(password.len() as u8);
        request.extend(password.as_bytes());
        stream.write_all(&request).await?;
        stream.read_exact(&mut reply).await?;
        ensure!(reply == [1, 0], "SOCKS authentication failed");
    }
    let mut request = vec![5, 1, 0];
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            request.push(1);
            request.extend(ip.octets());
        }
        Ok(std::net::IpAddr::V6(ip)) => {
            request.push(4);
            request.extend(ip.octets());
        }
        Err(_) => {
            ensure!(
                !host.is_empty() && host.len() <= 255,
                "Invalid SOCKS destination"
            );
            request.extend([3, host.len() as u8]);
            request.extend(host.as_bytes());
        }
    }
    request.extend(port.to_be_bytes());
    stream.write_all(&request).await?;
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    ensure!(
        header[0] == 5 && header[1] == 0,
        "SOCKS destination connection failed"
    );
    let length = match header[3] {
        1 => 4,
        4 => 16,
        3 => stream.read_u8().await? as usize,
        _ => anyhow::bail!("Invalid SOCKS reply"),
    };
    let mut rest = vec![0u8; length + 2];
    stream.read_exact(&mut rest).await?;
    Ok(stream)
}
