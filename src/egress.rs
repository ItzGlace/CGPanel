//! Runs only inside a rootless gateway container; never configure host networking.
use anyhow::{ensure, Context, Result};
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    os::fd::AsRawFd,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::Semaphore,
};

fn rule(binary: &str, arguments: &[&str]) -> Result<()> {
    ensure!(
        std::process::Command::new(binary)
            .args(arguments)
            .status()?
            .success(),
        "Gateway firewall setup failed"
    );
    Ok(())
}
fn firewall(ip: &str, port: &str) -> Result<()> {
    ensure!(
        std::path::Path::new("/run/.containerenv").exists()
            || std::path::Path::new("/.dockerenv").exists(),
        "Egress gateway requires a container"
    );
    // Default-deny first: a failed setup cannot create a direct route around the proxy.
    rule("iptables", &["-P", "OUTPUT", "DROP"])?;
    rule("ip6tables", &["-P", "OUTPUT", "DROP"])?;
    rule("iptables", &["-F", "OUTPUT"])?;
    rule("iptables", &["-t", "nat", "-F", "OUTPUT"])?;
    rule("iptables", &["-A", "OUTPUT", "-o", "lo", "-j", "ACCEPT"])?;
    rule(
        "iptables",
        &[
            "-A",
            "OUTPUT",
            "-d",
            "127.0.0.1",
            "-p",
            "tcp",
            "--dport",
            "12345",
            "-j",
            "ACCEPT",
        ],
    )?;
    rule(
        "iptables",
        &[
            "-A",
            "OUTPUT",
            "-d",
            "127.0.0.1",
            "-p",
            "udp",
            "--dport",
            "53",
            "-j",
            "ACCEPT",
        ],
    )?;
    rule(
        "iptables",
        &[
            "-A",
            "OUTPUT",
            "-m",
            "conntrack",
            "--ctstate",
            "ESTABLISHED,RELATED",
            "-j",
            "ACCEPT",
        ],
    )?;
    // The network namespace and keep-id user namespace can use different UID maps.
    // A destination exception is independent of those maps: only the fixed SOCKS
    // endpoint is reachable directly; all other new TCP sockets are redirected.
    rule(
        "iptables",
        &[
            "-A", "OUTPUT", "-d", ip, "-p", "tcp", "--dport", port, "-j", "ACCEPT",
        ],
    )?;
    rule(
        "iptables",
        &[
            "-t",
            "nat",
            "-A",
            "OUTPUT",
            "-d",
            "127.0.0.0/8",
            "-j",
            "RETURN",
        ],
    )?;
    rule(
        "iptables",
        &[
            "-t", "nat", "-A", "OUTPUT", "-d", ip, "-p", "tcp", "--dport", port, "-j", "RETURN",
        ],
    )?;
    rule(
        "iptables",
        &[
            "-t",
            "nat",
            "-A",
            "OUTPUT",
            "-p",
            "tcp",
            "--syn",
            "-j",
            "REDIRECT",
            "--to-ports",
            "12345",
        ],
    )?;
    rule(
        "iptables",
        &[
            "-t",
            "nat",
            "-A",
            "OUTPUT",
            "-p",
            "udp",
            "--dport",
            "53",
            "-j",
            "REDIRECT",
            "--to-ports",
            "53",
        ],
    )?;
    // IPv6 and non-DNS UDP remain blocked; there is no direct fallback.
    Ok(())
}
fn original_destination(stream: &TcpStream) -> Result<SocketAddrV4> {
    let mut address: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_IP,
            80,
            &mut address as *mut _ as *mut libc::c_void,
            &mut size,
        )
    };
    ensure!(
        rc == 0
            && size as usize == std::mem::size_of::<libc::sockaddr_in>()
            && address.sin_family == libc::AF_INET as u16,
        "Cannot identify original destination"
    );
    Ok(SocketAddrV4::new(
        Ipv4Addr::from(address.sin_addr.s_addr.to_ne_bytes()),
        u16::from_be(address.sin_port),
    ))
}
async fn tcp(mut incoming: TcpStream, proxy: Arc<String>) -> Result<()> {
    let target = original_destination(&incoming)?;
    ensure!(
        !target.ip().is_loopback(),
        "Direct gateway access is not permitted"
    );
    let mut remote =
        cgpanel::socks::connect(&proxy, &target.ip().to_string(), target.port()).await?;
    tokio::time::timeout(
        Duration::from_secs(3600),
        tokio::io::copy_bidirectional(&mut incoming, &mut remote),
    )
    .await??;
    Ok(())
}
async fn dns(packet: Vec<u8>, proxy: Arc<String>) -> Result<Vec<u8>> {
    ensure!(
        packet.len() >= 12 && packet.len() <= 4096,
        "Invalid DNS packet"
    );
    let mut remote = cgpanel::socks::connect(&proxy, "1.1.1.1", 53).await?;
    remote.write_u16(packet.len() as u16).await?;
    remote.write_all(&packet).await?;
    let length = remote.read_u16().await? as usize;
    ensure!(length <= 65535, "DNS reply is too large");
    let mut result = vec![0u8; length];
    remote.read_exact(&mut result).await?;
    Ok(result)
}
#[tokio::main]
async fn main() -> Result<()> {
    let config: serde_json::Value =
        serde_json::from_slice(&tokio::fs::read("/run/secrets/proxy.json").await?)?;
    let proxy = Arc::new(
        config["url"]
            .as_str()
            .context("Proxy is missing")?
            .to_string(),
    );
    let url = reqwest::Url::parse(&proxy)?;
    let ip = url.host_str().context("Missing proxy IP")?;
    ensure!(
        ip.parse::<Ipv4Addr>().is_ok(),
        "Gateway currently requires an IPv4 SOCKS endpoint"
    );
    firewall(ip, &url.port().context("Missing proxy port")?.to_string())?;
    let listener = TcpListener::bind("127.0.0.1:12345").await?;
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:53").await?);
    tokio::fs::write("/tmp/ready", "ready").await?;
    let limit = Arc::new(Semaphore::new(256));
    let dns_socket = socket.clone();
    let dns_proxy = proxy.clone();
    let dns_limit = limit.clone();
    tokio::spawn(async move {
        let mut buffer = vec![0u8; 4096];
        while let Ok((len, peer)) = dns_socket.recv_from(&mut buffer).await {
            let Ok(permit) = dns_limit.clone().try_acquire_owned() else {
                continue;
            };
            let packet = buffer[..len].to_vec();
            let socket = dns_socket.clone();
            let proxy = dns_proxy.clone();
            tokio::spawn(async move {
                let _permit = permit;
                if let Ok(Ok(reply)) =
                    tokio::time::timeout(Duration::from_secs(20), dns(packet, proxy)).await
                {
                    let _ = socket.send_to(&reply, peer).await;
                }
            });
        }
    });
    loop {
        let (stream, _) = listener.accept().await?;
        let Ok(permit) = limit.clone().try_acquire_owned() else {
            continue;
        };
        let proxy = proxy.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = tcp(stream, proxy).await;
        });
    }
}
