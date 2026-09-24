use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
pub mod network;
pub mod outbound;
pub mod s3_upload;
pub mod schedule;
pub mod site;
pub mod socks;

#[derive(Debug, Serialize, Deserialize)]
pub struct Operation {
    pub action: String,
    pub tenant: String,
    pub id: String,
    pub data: Value,
}

/// Client-supplied application properties must never become broker-owned service
/// metadata (OS account names, IDE ports, credentials, or transfer configuration).
pub fn application_input(value: &Value) -> Value {
    let allowed = [
        "name",
        "runtime",
        "version",
        "mode",
        "command",
        "env",
        "memory_mb",
        "cpu_millis",
        "disk_mb",
    ];
    Value::Object(
        value
            .as_object()
            .into_iter()
            .flat_map(|m| m.iter())
            .filter(|(key, _)| allowed.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

#[cfg(test)]
mod application_input_tests {
    #[test]
    fn service_metadata_cannot_be_injected() {
        let value = super::application_input(
            &serde_json::json!({"name":"site","runtime":"php","transfer_user":"root","ide_enabled":true,"ide_port":22,"port":22,"image":"untrusted","memory_mb":128}),
        );
        assert_eq!(
            value,
            serde_json::json!({"name":"site","runtime":"php","memory_mb":128})
        );
    }
}

pub fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}
pub fn domain(s: &str) -> bool {
    s.len() <= 253
        && s.contains('.')
        && s.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        })
}
pub fn within_domain(s: &str, root: &str) -> bool {
    s == root || s.ends_with(&format!(".{root}"))
}
pub fn relative_path(s: &str) -> bool {
    !s.is_empty()
        && s.len() < 512
        && !s.starts_with('/')
        && !s.contains('\\')
        && s.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && p.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        })
}
pub fn runtime(s: &str) -> bool {
    ["python", "php", "node", "java", "rust", "static"].contains(&s)
}
pub fn valid_ip(s: &str) -> bool {
    s.parse::<std::net::IpAddr>().is_ok()
}
pub fn dns_record(kind: &str, name: &str, value: &str, zone: &str) -> bool {
    if !(name == "@"
        || (name.len() < 128
            && name.split('.').all(|p| {
                !p.is_empty()
                    && p.bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            })))
    {
        return false;
    }
    if value.is_empty()
        || value.len() > 250
        || value
            .chars()
            .any(|c| c.is_control() || c == '"' || c == '\\' || c == ';' || c == '(' || c == ')')
    {
        return false;
    }
    match kind {
        "A" => value.parse::<std::net::Ipv4Addr>().is_ok(),
        "AAAA" => value.parse::<std::net::Ipv6Addr>().is_ok(),
        "CNAME" | "NS" => domain(value.trim_end_matches('.')) && !(kind == "CNAME" && name == "@"),
        "MX" => value
            .split_once(' ')
            .is_some_and(|(p, h)| p.parse::<u16>().is_ok() && domain(h.trim_end_matches('.'))),
        "TXT" => true,
        _ => {
            let _ = zone;
            false
        }
    }
}

pub async fn agent_call(socket: &str, op: Operation) -> Result<Value> {
    agent_call_timeout(socket, op, 360).await
}
pub async fn agent_call_timeout(socket: &str, op: Operation, seconds: u64) -> Result<Value> {
    #[cfg(unix)]
    {
        let mut stream = tokio::net::UnixStream::connect(socket).await?;
        let mut request = serde_json::to_vec(&op)?;
        request.push(b'\n');
        stream.write_all(&request).await?;
        let mut response = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(seconds),
            BufReader::new(stream).read_line(&mut response),
        )
        .await??;
        let result: Value = serde_json::from_str(&response)?;
        if result["ok"] != true {
            bail!(
                "{}",
                result["error"].as_str().unwrap_or("Agent request failed")
            );
        }
        Ok(result["result"].clone())
    }
    #[cfg(not(unix))]
    {
        let _ = (socket, op, seconds);
        bail!("Host operations require Linux")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn domain_boundary() {
        assert!(within_domain("api.example.com", "example.com"));
        assert!(!within_domain("evilexample.com", "example.com"));
        assert!(!within_domain("example.com.evil.org", "example.com"));
    }
    #[test]
    fn reject_injection() {
        for x in ["a;id", "../root", "-oops", "A", "x\ny", ""] {
            assert!(!identifier(x));
        }
        assert!(!domain("example.com;id"));
        assert!(!domain("-bad.org"));
    }
    #[test]
    fn paths_confined() {
        for x in ["../etc/passwd", "/etc/shadow", "a/../../b", "a\\b", "a/./b"] {
            assert!(!relative_path(x));
        }
        assert!(relative_path("src/main.py"));
    }
    #[test]
    fn dns_injection() {
        assert!(!dns_record(
            "TXT",
            "@",
            "x\ninclude /etc/passwd",
            "example.org"
        ));
        assert!(!dns_record("TXT", "@", "x\"", "example.org"));
        assert!(!dns_record("CNAME", "@", "example.net", "example.org"));
        assert!(dns_record("MX", "@", "10 mail.example.org", "example.org"));
    }
}
