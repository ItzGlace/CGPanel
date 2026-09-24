use anyhow::{bail, Result};
use ipnet::IpNet;
use std::net::IpAddr;

pub fn network(value: &str) -> Result<IpNet> {
    Ok(if value.contains('/') {
        value.parse::<IpNet>()?.trunc()
    } else {
        IpNet::from(value.parse::<IpAddr>()?)
    })
}
pub fn canonical(value: &str) -> Result<String> {
    let net = network(value)?;
    Ok(if value.contains('/') {
        net.to_string()
    } else {
        net.addr().to_string()
    })
}
pub fn range(value: &str) -> Result<(String, String)> {
    let net = network(value)?;
    Ok((net.network().to_string(), net.broadcast().to_string()))
}
pub fn mysql_host(value: &str) -> Result<String> {
    let net = network(value)?;
    match net {
        IpNet::V4(n) if n.prefix_len()<32 => Ok(format!("{}/{}",n.network(),n.netmask())),
        IpNet::V6(n) if n.prefix_len()<128 => bail!("MariaDB supports IPv4 network masks and individual IPv6 addresses; use PostgreSQL for IPv6 prefixes"),
        _ => Ok(net.addr().to_string()),
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn ranges_and_database_hosts() {
        assert_eq!(
            super::range("10.10.10.57/24").unwrap(),
            ("10.10.10.0".into(), "10.10.10.255".into())
        );
        assert_eq!(
            super::mysql_host("10.10.10.57/24").unwrap(),
            "10.10.10.0/255.255.255.0"
        );
        assert_eq!(
            super::range("2001:db8::1/126").unwrap(),
            ("2001:db8::".into(), "2001:db8::3".into())
        );
        assert!(super::mysql_host("2001:db8::/64").is_err());
        assert!(super::network("10.0.0.1/33").is_err());
        assert!(super::network("10.0.0.1';DROP USER x").is_err());
    }
}
