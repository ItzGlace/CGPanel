//! Certbot invokes this fixed hook for domains served by CGPanel's authoritative BIND.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    io::Write,
    os::{fd::AsRawFd, unix::fs::PermissionsExt},
    path::Path,
    process::Command,
};
fn replace(path: &str, bytes: &[u8]) -> Result<()> {
    let temporary = format!("{path}.new");
    let mut f = std::fs::File::create(&temporary)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(temporary, path)?;
    Ok(())
}
fn main() -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "ACME hook requires the host administrator"
    );
    let arguments: Vec<_> = std::env::args().collect();
    ensure!(
        arguments.len() == 3 && ["auth", "cleanup"].contains(&arguments[1].as_str()),
        "Invalid hook arguments"
    );
    let id = &arguments[2];
    ensure!(
        cgpanel::identifier(id) && id.len() == 32,
        "Invalid domain ID"
    );
    let domain = std::env::var("CERTBOT_DOMAIN")?;
    let token = std::env::var("CERTBOT_VALIDATION")?;
    ensure!(
        token.len() <= 128
            && !token.is_empty()
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "Invalid ACME challenge"
    );
    let registry: Value =
        serde_json::from_slice(&std::fs::read("/var/lib/cgpanel-agent/registry.json")?)?;
    ensure!(
        registry["items"][id]["kind"] == "domains"
            && registry["items"][id]["data"]["name"] == domain,
        "Domain does not match the registered zone"
    );
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open("/run/cgpanel/dns.lock")?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } == 0,
        "Cannot lock DNS configuration"
    );
    let folder = "/var/lib/cgpanel-agent/challenges";
    std::fs::create_dir_all(folder)?;
    let file = format!("{folder}/{id}.json");
    let mut challenges: Vec<String> = if Path::new(&file).exists() {
        serde_json::from_slice(&std::fs::read(&file)?)?
    } else {
        Vec::new()
    };
    let old = challenges.clone();
    challenges.retain(|v| v != &token);
    if arguments[1] == "auth" {
        challenges.push(token);
    }
    replace(&file, &serde_json::to_vec(&challenges)?)?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
    let zonefile = format!("/etc/bind/cgpanel/{id}.zone");
    let existing = std::fs::read_to_string(&zonefile)?;
    let mut lines = Vec::new();
    for line in existing.lines() {
        if old
            .iter()
            .any(|v| line == format!("_acme-challenge 60 IN TXT \"{v}\""))
        {
            continue;
        }
        if line.contains(" IN SOA ") {
            let (prefix, rest) = line.split_once('(').context("Invalid SOA record")?;
            let (serial, suffix) = rest.split_once(' ').context("Invalid SOA serial")?;
            let serial = serial
                .parse::<i64>()?
                .saturating_add(1)
                .max(chrono::Utc::now().timestamp());
            lines.push(format!("{prefix}({serial} {suffix}"));
        } else {
            lines.push(line.to_string());
        }
    }
    for value in challenges {
        lines.push(format!("_acme-challenge 60 IN TXT \"{value}\""));
    }
    replace(&zonefile, format!("{}\n", lines.join("\n")).as_bytes())?;
    ensure!(
        Command::new("named-checkzone")
            .args([&domain, &zonefile])
            .output()?
            .status
            .success(),
        "Challenge zone validation failed"
    );
    ensure!(
        Command::new("rndc")
            .args(["reload", &domain])
            .output()?
            .status
            .success(),
        "Challenge zone reload failed"
    );
    drop(lock);
    if arguments[1] == "auth" {
        std::thread::sleep(std::time::Duration::from_secs(15));
    }
    println!("{}", json!({"updated":true}));
    Ok(())
}
