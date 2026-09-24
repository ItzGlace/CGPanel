//! Linux privileged broker. The HTTP process never runs as root.
mod backups_agent;
mod cron_agent;
mod egress_agent;
mod integrations_agent;
mod tls_agent;
mod transfers_agent;
mod workspace_agent;
use anyhow::{anyhow, bail, ensure, Context, Result};
use cgpanel::{identifier, Operation};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::os::unix::process::CommandExt;
use std::{
    collections::BTreeMap, os::unix::fs::PermissionsExt, path::Path, process::Stdio, sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::Mutex,
};
const ROOT: &str = "/var/lib/cgpanel-agent";
#[derive(Clone, Serialize, Deserialize)]
struct Item {
    tenant: String,
    kind: String,
    data: Value,
}
#[derive(Default, Serialize, Deserialize)]
struct Registry {
    items: BTreeMap<String, Item>,
    ports: u16,
}
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn user(t: &str) -> String {
    format!("cg_{}", &t[..20])
}
fn home(t: &str) -> String {
    format!("/srv/cgpanel/tenants/{t}")
}
fn appdir(t: &str, id: &str) -> String {
    format!("{}/apps/{id}", home(t))
}
async fn bounded<R: AsyncRead + Unpin>(mut r: R) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = r.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let remaining = 2097152usize.saturating_sub(result.len());
        result.extend_from_slice(&buf[..n.min(remaining)]);
    }
    Ok(result)
}
async fn exec(
    program: &str,
    args: Vec<String>,
    input: Option<String>,
    seconds: u64,
) -> Result<String> {
    let mut cmd = Command::new(program);
    cmd.as_std_mut().process_group(0);
    cmd.args(args)
        .current_dir("/")
        .env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        )
        .env("LC_ALL", "C")
        .kill_on_drop(true)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("Cannot start {program}"))?;
    let process_group = child.id().context("Missing child process ID")?;
    if let Some(input) = input {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.as_bytes()).await?;
        drop(stdin);
    }
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let result = tokio::time::timeout(Duration::from_secs(seconds), async {
        let (status, out, err) = tokio::join!(child.wait(), bounded(out), bounded(err));
        Ok::<_, anyhow::Error>((status?, out?, err?))
    })
    .await;
    let (status, out, err) = match result {
        Ok(r) => r?,
        Err(_) => {
            unsafe {
                libc::kill(-(process_group as i32), libc::SIGKILL);
            }
            let _ = child.kill().await;
            bail!("Operation timed out");
        }
    };
    let out = String::from_utf8_lossy(&out).into_owned();
    let err = String::from_utf8_lossy(&err).into_owned();
    if !status.success() {
        bail!(
            "{program} failed ({}): {}",
            status.code().unwrap_or(-1),
            err.chars().take(1500).collect::<String>()
        );
    }
    Ok(if err.is_empty() {
        out
    } else {
        format!("{out}\n{err}")
    })
}
fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}
async fn run(p: &str, a: &[&str]) -> Result<String> {
    exec(p, args(a), None, 60).await
}
async fn atomic(path: &str, content: &str, mode: u32) -> Result<()> {
    let tmp = format!("{path}.tmp");
    tokio::fs::write(&tmp, content).await?;
    tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode)).await?;
    tokio::fs::rename(tmp, path).await?;
    Ok(())
}
async fn uid(t: &str) -> Result<String> {
    Ok(run("id", &["-u", &user(t)]).await?.trim().into())
}
async fn pod(t: &str, a: Vec<String>, input: Option<String>, seconds: u64) -> Result<String> {
    let uid = uid(t).await?;
    let mut cmd = args(&[
        "-u",
        &user(t),
        "--",
        "env",
        &format!("XDG_RUNTIME_DIR=/run/user/{uid}"),
        &format!("DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{uid}/bus"),
        &format!("HOME={}", home(t)),
        "podman",
    ]);
    cmd.extend(a);
    exec("runuser", cmd, input, seconds).await
}
async fn tenant(t: &str) -> Result<()> {
    if run("id", &["-u", &user(t)]).await.is_ok() {
        run("loginctl", &["enable-linger", &user(t)]).await?;
        let uid = uid(t).await?;
        run("systemctl", &["start", &format!("user@{uid}.service")]).await?;
        return Ok(());
    }
    run(
        "useradd",
        &[
            "--create-home",
            "--home-dir",
            &home(t),
            "--shell",
            "/usr/sbin/nologin",
            &user(t),
        ],
    )
    .await?;
    run("chmod", &["0700", &home(t)]).await?;
    run("loginctl", &["enable-linger", &user(t)]).await?;
    let uid = uid(t).await?;
    run("systemctl", &["start", &format!("user@{uid}.service")]).await?;
    tokio::fs::create_dir_all(format!("{}/apps", home(t))).await?;
    run(
        "chown",
        &[
            &format!("{}:{}", user(t), user(t)),
            &format!("{}/apps", home(t)),
        ],
    )
    .await?;
    Ok(())
}
fn owned<'a>(reg: &'a Registry, op: &Operation, kind: &str) -> Result<&'a Item> {
    let item = reg
        .items
        .get(&op.id)
        .context("Resource not found in agent")?;
    ensure!(
        item.tenant == op.tenant && item.kind == kind,
        "Resource ownership mismatch"
    );
    Ok(item)
}
fn reference<'a>(reg: &'a Registry, id: &str, t: &str, kind: &str) -> Result<&'a Item> {
    let item = reg.items.get(id).context("Referenced resource not found")?;
    ensure!(
        item.tenant == t && item.kind == kind,
        "Referenced resource ownership mismatch"
    );
    Ok(item)
}
fn container(id: &str) -> String {
    format!("cgp_{id}")
}
fn image(runtime: &str) -> Result<String> {
    let registry = std::env::var("CGPANEL_IMAGE_REGISTRY").unwrap_or("docker.io".into());
    ensure!(
        ["docker.io", "public.ecr.aws/docker"].contains(&registry.as_str()),
        "Unsupported image registry"
    );
    let image = match runtime {
        "python" | "static" => "python:3.13-slim-bookworm",
        "php" => "php:8.4-cli-bookworm",
        "node" => "node:22-bookworm-slim",
        "java" => "eclipse-temurin:21-jdk",
        "rust" => "rust:1-bookworm",
        _ => bail!("Unsupported runtime"),
    };
    Ok(format!("{registry}/library/{image}"))
}
fn starter(runtime: &str) -> (&'static str, &'static str, &'static str) {
    match runtime{
    "python"|"static"=>("index.html","<!doctype html><title>CGPanel</title><h1>Your CGPanel application is running.</h1>","python -m http.server 8080 --bind 0.0.0.0"),
    "php"=>("index.php","<?php echo '<h1>Your PHP application is running on CGPanel.</h1>';", "php -S 0.0.0.0:8080 -t /workspace"),
    "node"=>("server.js","require('http').createServer((q,s)=>s.end('CGPanel Node.js application is running')).listen(8080,'0.0.0.0');", "node server.js"),
    "java"=>("Main.java","import com.sun.net.httpserver.HttpServer; import java.net.InetSocketAddress; class Main { public static void main(String[] a) throws Exception {var s=HttpServer.create(new InetSocketAddress(8080),0);s.createContext(\"/\",e->{byte[] b=\"CGPanel Java application is running\".getBytes();e.sendResponseHeaders(200,b.length);e.getResponseBody().write(b);e.close();});s.start();}}", "java Main.java"),
    _=>("main.rs","use std::{net::TcpListener,io::{Read,Write}};fn main(){for s in TcpListener::bind(\"0.0.0.0:8080\").unwrap().incoming(){if let Ok(mut s)=s{let mut b=[0;4096];let _=s.read(&mut b);let _=s.write_all(b\"HTTP/1.1 200 OK\\r\\nContent-Length: 13\\r\\n\\r\\nCGPanel Rust!\");}}}", "rustc main.rs -o /workspace/server && ./server")}
}
async fn create_app(reg: &mut Registry, op: &Operation) -> Result<Value> {
    ensure!(identifier(s(&op.data, "name")), "Invalid application name");
    let runtime = s(&op.data, "runtime");
    let image = if s(&op.data, "version").is_empty() {
        image(runtime)?
    } else {
        workspace_agent::image(runtime, s(&op.data, "version"))?
    };
    ensure!(
        ["web", "worker"].contains(&s(&op.data, "mode")),
        "Invalid application mode"
    );
    tenant(&op.tenant).await?;
    let dir = appdir(&op.tenant, &op.id);
    tokio::fs::create_dir_all(&dir).await?;
    let (file, content, default) = starter(runtime);
    if !["egress_configure", "runtime_configure", "runtime_activate"].contains(&op.action.as_str())
    {
        tokio::fs::write(format!("{dir}/{file}"), content).await?;
    }
    run(
        "chown",
        &[
            "-R",
            &format!("{}:{}", user(&op.tenant), user(&op.tenant)),
            &dir,
        ],
    )
    .await?;
    let port = if ["egress_configure", "runtime_configure", "runtime_activate"]
        .contains(&op.action.as_str())
    {
        reg.items
            .get(&op.id)
            .and_then(|i| i.data["port"].as_u64())
            .context("Missing application port")? as u16
    } else {
        reg.ports.max(20000) + 1
    };
    ensure!(port < 40000, "Application port range exhausted");
    reg.ports = reg.ports.max(port);
    let proxy_network = egress_agent::prepare(reg, op, port).await?;
    let user_namespace = if proxy_network.is_empty() {
        "keep-id:uid=1000,gid=1000".to_owned()
    } else {
        proxy_network.clone()
    };
    let network = if proxy_network.is_empty() {
        "slirp4netns:allow_host_loopback=false"
    } else {
        &proxy_network
    };
    let custom = s(&op.data, "command");
    let command = if custom.is_empty() { default } else { custom };
    ensure!(command.len() <= 2000, "Command too long");
    let mut a = args(&[
        "run",
        "-d",
        "--name",
        &container(&op.id),
        "--restart",
        if op.action == "runtime_configure" { "no" } else { "on-failure:3" },
        "--userns",
        &user_namespace,
        "--user",
        "1000:1000",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges",
        "--memory",
        "512m",
        "--cpus",
        "1",
        "--pids-limit",
        "128",
        "--network",
        network,
        "--read-only",
        "--tmpfs",
        "/tmp:rw,noexec,nosuid,size=128m",
        "--volume",
        &format!("{dir}:/workspace:rw"),
        "--workdir",
        "/workspace",
        "--env",
        "HOME=/workspace",
        "--env",
        "PORT=8080",
        "--env",
        "PYTHONUSERBASE=/workspace/.local",
        "--env",
        "NPM_CONFIG_PREFIX=/workspace/.local",
        "--env",
        "CARGO_HOME=/workspace/.cargo",
        "--env",
        "PATH=/workspace/.local/bin:/workspace/.cargo/bin:/usr/local/cargo/bin:/opt/java/openjdk/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        "--log-opt",
        "max-size=10mb",
    ]);
    if s(&op.data, "mode") == "web" && proxy_network.is_empty() {
        a.extend(args(&["-p", &format!("127.0.0.1:{port}:8080")]));
    }
    if let Some(env) = op.data["env"].as_object() {
        ensure!(env.len() <= 64, "Too many environment variables");
        for (k, v) in env {
            ensure!(
                !k.is_empty()
                    && k.len() < 64
                    && k.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
                "Invalid environment key"
            );
            ensure!(
                ![
                    "HOME",
                    "PATH",
                    "LD_PRELOAD",
                    "LD_LIBRARY_PATH",
                    "CARGO_HOME",
                    "RUSTUP_HOME"
                ]
                .contains(&k.as_str()),
                "Reserved environment variable"
            );
            let value = v.as_str().context("Environment values must be strings")?;
            ensure!(
                value.len() < 8192 && !value.contains('\0'),
                "Invalid environment value"
            );
            a.extend(args(&["--env", &format!("{k}={value}")]));
        }
    }
    a.extend(args(&[&image, "sh", "-c", command]));
    pod(&op.tenant, a, None, 300).await?;
    let mut d = op.data.clone();
    d["port"] = json!(port);
    d["image"] = json!(image);
    reg.items.insert(
        op.id.clone(),
        Item {
            tenant: op.tenant.clone(),
            kind: "apps".into(),
            data: d,
        },
    );
    let uid = uid(&op.tenant).await?;
    let _ = exec(
        "runuser",
        args(&[
            "-u",
            &user(&op.tenant),
            "--",
            "env",
            &format!("XDG_RUNTIME_DIR=/run/user/{uid}"),
            "systemctl",
            "--user",
            "enable",
            "podman-restart.service",
        ]),
        None,
        30,
    )
    .await;
    Ok(
        json!({"port":port,"image":image,"state":"created","workspace":format!("/workspace/{file}")}),
    )
}
async fn create_domain(reg: &mut Registry, op: &Operation) -> Result<Value> {
    let name = s(&op.data, "name");
    ensure!(cgpanel::domain(name), "Invalid domain");
    ensure!(
        !reg.items
            .values()
            .any(|i| i.kind == "domains" && s(&i.data, "name") == name),
        "Domain already provisioned"
    );
    let app = s(&op.data, "app_id");
    let location = if app.is_empty() {
        let dir = format!("/srv/cgpanel/public/{}", op.id);
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(format!("{dir}/index.html"), "<h1>Hosted by CGPanel</h1>").await?;
        format!("root {dir}; index index.html; location / {{ try_files $uri $uri/ =404; }}")
    } else {
        let item = reference(reg, app, &op.tenant, "apps")?;
        ensure!(
            s(&item.data, "mode") == "web",
            "Workers cannot receive web traffic"
        );
        let port = item.data["port"].as_u64().context("Missing app port")?;
        format!("location / {{ proxy_pass http://127.0.0.1:{port}; proxy_set_header Host $host; proxy_set_header X-Real-IP $remote_addr; proxy_set_header X-Forwarded-For $remote_addr; proxy_set_header X-Forwarded-Proto $scheme; proxy_http_version 1.1; proxy_set_header Upgrade $http_upgrade; proxy_set_header Connection $cg_connection; proxy_read_timeout 60s; }}")
    };
    let conf=format!("server {{ listen 80; listen [::]:80; server_name {name}; limit_req zone=cgp_web burst=40 nodelay; limit_conn cgp_conn 30; client_max_body_size 32m; {location} }}\n");
    let path = format!("/etc/nginx/conf.d/cgp_{}.conf", op.id);
    atomic(&path, &conf, 0o644).await?;
    if let Err(e) = run("nginx", &["-t"]).await {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(e);
    }
    run("systemctl", &["reload", "nginx"]).await?;
    reg.items.insert(
        op.id.clone(),
        Item {
            tenant: op.tenant.clone(),
            kind: "domains".into(),
            data: op.data.clone(),
        },
    );
    if let Err(e) = dns_sync(reg).await {
        reg.items.remove(&op.id);
        let _ = tokio::fs::remove_file(path).await;
        let _ = run("systemctl", &["reload", "nginx"]).await;
        return Err(e);
    }
    tls_agent::render_domain(reg, &op.id, &reg.items[&op.id]).await?;
    Ok(json!({"url":format!("http://{name}"),"tls":"not issued"}))
}
async fn dns_sync(reg: &Registry) -> Result<()> {
    use std::os::fd::AsRawFd;
    let _lock = tokio::task::spawn_blocking(|| -> Result<std::fs::File> {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open("/run/cgpanel/dns.lock")?;
        ensure!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0,
            "Cannot lock DNS"
        );
        Ok(file)
    })
    .await??;
    let mut config = String::new();
    let serial = chrono::Utc::now().timestamp();
    let server_ip = std::env::var("CGPANEL_PUBLIC_IP").unwrap_or("127.0.0.1".into());
    ensure!(
        server_ip.parse::<std::net::Ipv4Addr>().is_ok(),
        "Invalid public IP"
    );
    for (id, item) in reg.items.iter().filter(|(_, i)| i.kind == "domains") {
        let zone = s(&item.data, "name");
        let file = format!("/etc/bind/cgpanel/{id}.zone");
        let old = tokio::fs::read_to_string(&file).await.unwrap_or_default();
        let previous = old
            .lines()
            .find(|l| l.contains(" IN SOA "))
            .and_then(|l| l.split_once('('))
            .and_then(|(_, r)| r.split_whitespace().next())
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let serial = serial.max(previous + 1);
        let records: Vec<_> = reg
            .items
            .values()
            .filter(|i| i.kind == "dns" && s(&i.data, "domain_id") == id)
            .collect();
        let mut content=format!("$ORIGIN {zone}.\n$TTL 300\n@ IN SOA ns1.{zone}. hostmaster.{zone}. ({serial} 3600 900 1209600 300)\n@ IN NS ns1.{zone}.\nns1 IN A {server_ip}\n");
        if !records
            .iter()
            .any(|i| s(&i.data, "name") == "@" && s(&i.data, "type") == "A")
        {
            content.push_str(&format!("@ IN A {server_ip}\n"));
        }
        for r in records {
            let name = s(&r.data, "name");
            let kind = s(&r.data, "type");
            let val = s(&r.data, "value");
            ensure!(
                cgpanel::dns_record(kind, name, val, zone),
                "Invalid stored DNS record"
            );
            let ttl = r.data["ttl"].as_u64().unwrap_or(300).clamp(60, 86400);
            let value = match kind {
                "TXT" => format!("\"{val}\""),
                "CNAME" | "NS" => format!("{}.", val.trim_end_matches('.')),
                "MX" => {
                    let (p, h) = val.split_once(' ').unwrap();
                    format!("{p} {}.", h.trim_end_matches('.'))
                }
                _ => val.into(),
            };
            content.push_str(&format!("{name} {ttl} IN {kind} {value}\n"));
        }
        if let Ok(challenges) =
            tokio::fs::read_to_string(format!("{ROOT}/challenges/{id}.json")).await
        {
            for value in serde_json::from_str::<Vec<String>>(&challenges)? {
                ensure!(
                    value.len() <= 128
                        && value
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
                    "Invalid stored ACME token"
                );
                content.push_str(&format!("_acme-challenge 60 IN TXT \"{value}\"\n"));
            }
        }
        atomic(&file, &content, 0o644).await?;
        run("named-checkzone", &[zone, &file]).await?;
        config.push_str(&format!(
            "zone \"{zone}\" {{ type master; file \"{file}\"; allow-transfer {{ none; }}; }};\n"
        ));
    }
    atomic("/etc/bind/cgpanel/zones.conf", &config, 0o644).await?;
    run("named-checkconf", &[]).await?;
    run("rndc", &["reconfig"]).await?;
    let _ = run("rndc", &["reload"]).await;
    Ok(())
}
fn db_name(id: &str) -> String {
    format!("cgp_{}", &id[..20])
}
fn random_secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn exact_ips(v: &Value) -> Result<Vec<String>> {
    let a = v.as_array().cloned().unwrap_or_default();
    ensure!(a.len() <= 16, "Too many IP addresses");
    a.iter()
        .map(|v| {
            let s = v.as_str().context("Invalid IP")?;
            ensure!(cgpanel::valid_ip(s), "Use exact IP addresses");
            Ok(s.into())
        })
        .collect()
}
async fn mysql(sql: &str) -> Result<String> {
    exec(
        "mariadb",
        args(&["--batch", "--skip-column-names"]),
        Some(sql.into()),
        30,
    )
    .await
}
async fn pg(sql: &str) -> Result<String> {
    exec(
        "runuser",
        args(&[
            "-u",
            "postgres",
            "--",
            "psql",
            "-v",
            "ON_ERROR_STOP=1",
            "-q",
            "-X",
        ]),
        Some(sql.into()),
        30,
    )
    .await
}
async fn db_access(reg: &Registry) -> Result<()> {
    let mut hba=String::from("# Managed by CGPanel. Local OS admin uses peer authentication.\nlocal all postgres peer\nlocal all all scram-sha-256\n");
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for (id, i) in reg.items.iter().filter(|(_, i)| i.kind == "databases") {
        let name = db_name(id);
        for ip in exact_ips(&i.data["allowed_ips"])? {
            if ip.contains(':') {
                v6.push(ip.clone())
            } else {
                v4.push(ip.clone())
            }
            if s(&i.data, "engine") == "postgresql" {
                hba.push_str(&format!(
                    "hostssl {name} {name} {ip}/{} scram-sha-256\n",
                    if ip.contains(':') { 128 } else { 32 }
                ));
            }
        }
        if s(&i.data, "engine") == "postgresql" {
            hba.push_str(&format!("hostssl {name} {name} 127.0.0.1/32 scram-sha-256\nhostssl {name} {name} ::1/128 scram-sha-256\n"));
        }
    }
    let pg_hba =
        std::env::var("CGPANEL_PG_HBA").unwrap_or("/etc/postgresql/16/main/pg_hba.conf".into());
    atomic(&pg_hba, &hba, 0o640).await?;
    run("chown", &["postgres:postgres", &pg_hba]).await?;
    pg("SELECT pg_reload_conf();").await?;
    v4.sort();
    v4.dedup();
    v6.sort();
    v6.dedup();
    let mut nft =
        String::from("flush set inet cgpanel database4\nflush set inet cgpanel database6\n");
    if !v4.is_empty() {
        nft.push_str(&format!(
            "add element inet cgpanel database4 {{ {} }}\n",
            v4.join(", ")
        ));
    }
    if !v6.is_empty() {
        nft.push_str(&format!(
            "add element inet cgpanel database6 {{ {} }}\n",
            v6.join(", ")
        ));
    }
    exec("nft", args(&["-f", "-"]), Some(nft), 30).await?;
    Ok(())
}
async fn create_database(reg: &mut Registry, op: &Operation) -> Result<Value> {
    let engine = s(&op.data, "engine");
    ensure!(
        ["mysql", "postgresql"].contains(&engine),
        "Invalid database engine"
    );
    ensure!(identifier(s(&op.data, "name")), "Invalid database name");
    let ips = exact_ips(&op.data["allowed_ips"])?;
    let name = db_name(&op.id);
    let password = random_secret();
    if engine == "mysql" {
        let mut sql=format!("CREATE DATABASE `{name}` CHARACTER SET utf8mb4;\nCREATE USER '{name}'@'localhost' IDENTIFIED BY '{password}';\nGRANT ALL PRIVILEGES ON `{name}`.* TO '{name}'@'localhost';\n");
        for ip in &ips {
            sql.push_str(&format!("CREATE USER '{name}'@'{ip}' IDENTIFIED BY '{password}' REQUIRE SSL;\nGRANT ALL PRIVILEGES ON `{name}`.* TO '{name}'@'{ip}';\n"));
        }
        mysql(&sql).await?;
    } else {
        pg(&format!("CREATE ROLE {name} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION CONNECTION LIMIT 20;\nCREATE DATABASE {name} OWNER {name};\nREVOKE CONNECT ON DATABASE {name} FROM PUBLIC;\n")).await?;
    }
    let mut data = op.data.clone();
    data["password"] = json!(password);
    reg.items.insert(
        op.id.clone(),
        Item {
            tenant: op.tenant.clone(),
            kind: "databases".into(),
            data,
        },
    );
    db_access(reg).await?;
    Ok(
        json!({"database":name,"username":name,"password":password,"port":if engine=="mysql"{3306}else{5432},"local_host":"127.0.0.1","remote_tls_required":true}),
    )
}
async fn execute(reg: &mut Registry, op: Operation) -> Result<Value> {
    ensure!(
        identifier(&op.tenant) && op.tenant.len() == 32,
        "Invalid tenant ID"
    );
    ensure!(identifier(&op.id), "Invalid resource ID");
    ensure!(op.data.is_object(), "Operation data must be an object");
    if op.action.starts_with("create_") {
        ensure!(op.id.len() == 32, "Invalid resource ID");
        ensure!(!reg.items.contains_key(&op.id), "Resource already exists");
    }
    match op.action.as_str() {
        "workspace_files" => workspace_agent::files(reg, &op).await,
        "runtime_catalog" => Ok(workspace_agent::catalog()),
        "runtime_status" => {
            let i = owned(reg, &op, "apps")?;
            Ok(
                json!({"runtime":i.data["runtime"],"version":i.data["version"],"image":i.data["image"],"command":i.data["command"]}),
            )
        }
        "runtime_configure" => workspace_agent::runtime(reg, &op).await,
        "ide_configure" => workspace_agent::ide(reg, &op).await,
        "ide_status" => workspace_agent::ide_status(reg, &op).await,
        "ide_password" => {
            let i = owned(reg, &op, "apps")?;
            ensure!(i.data["ide_enabled"] == true, "IDE is not enabled");
            Ok(json!({"password":i.data["ide_password"]}))
        }
        "tenant_stop_ide" => {
            let ids: Vec<_> = reg
                .items
                .iter()
                .filter(|(_, i)| {
                    i.tenant == op.tenant && i.kind == "apps" && i.data["ide_enabled"] == true
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in ids {
                workspace_agent::ide(
                    reg,
                    &Operation {
                        action: "ide_configure".into(),
                        tenant: op.tenant.clone(),
                        id,
                        data: json!({"enabled":false}),
                    },
                )
                .await?;
            }
            Ok(json!({"stopped":true}))
        }
        "update_status" => {
            let config = tokio::fs::read_to_string("/etc/cgpanel/updates.json")
                .await
                .unwrap_or("{}".into());
            let status = tokio::fs::read_to_string("/var/lib/cgpanel-updates/status.json")
                .await
                .unwrap_or("{}".into());
            Ok(
                json!({"installed":env!("CARGO_PKG_VERSION"),"config":serde_json::from_str::<Value>(&config)?,"status":serde_json::from_str::<Value>(&status)?,"repository":"https://github.com/ItzGlace/CGPanel"}),
            )
        }
        "update_configure" => {
            match s(&op.data, "action") {
                "settings" => {
                    ensure!(op.data["enabled"].is_boolean(), "enabled must be a boolean");
                    atomic(
                        "/etc/cgpanel/updates.json",
                        &json!({"enabled":op.data["enabled"]}).to_string(),
                        0o600,
                    )
                    .await?;
                }
                "check" => {
                    run(
                        "systemctl",
                        &["start", "--no-block", "cgpanel-update-check.service"],
                    )
                    .await?;
                }
                "apply" => {
                    run(
                        "systemctl",
                        &["start", "--no-block", "cgpanel-update.service"],
                    )
                    .await?;
                }
                _ => bail!("Choose settings, check or apply"),
            };
            Ok(json!({"accepted":true}))
        }
        "tls_issue" | "tls_status" | "tls_panel_ip" | "zone_export" | "cdn_configure" => {
            tls_agent::execute(reg, &op).await
        }
        "egress_configure" => egress_agent::configure(reg, &op).await,
        "egress_status" => {
            let item = owned(reg, &op, "apps")?;
            Ok(
                json!({"proxy_id":s(&item.data,"egress_proxy_id"),"locked":item.data["egress_locked"]==true}),
            )
        }
        "integration_test" => transfers_agent::test(reg, &op).await,
        action if action.starts_with("full_backup_") => backups_agent::execute(reg, &op).await,
        action if action.starts_with("integration_") || action == "telegram_send" => {
            integrations_agent::execute(reg, &op).await
        }
        "site_seo" => {
            let item = owned(reg, &op, "domains")?;
            let scheme = if s(&op.data, "scheme") == "http" {
                "http"
            } else {
                "https"
            };
            cgpanel::site::seo(&format!("{scheme}://{}/", s(&item.data, "name"))).await
        }
        "health" => {
            let memory = tokio::fs::read_to_string("/proc/meminfo")
                .await
                .unwrap_or_default();
            let load = tokio::fs::read_to_string("/proc/loadavg")
                .await
                .unwrap_or_default();
            let disk = run("df", &["-h", "/srv/cgpanel"]).await.unwrap_or_default();
            Ok(
                json!({"agent":"online","memory":memory.lines().take(3).collect::<Vec<_>>(),"load":load.trim(),"disk":disk,"resources":reg.items.len()}),
            )
        }
        "create_tenant" => {
            tenant(&op.tenant).await?;
            Ok(json!({"user":user(&op.tenant)}))
        }
        "create_app" => create_app(reg, &op).await,
        "create_domain" => create_domain(reg, &op).await,
        "create_database" => create_database(reg, &op).await,
        "database_access" => {
            let item = owned(reg, &op, "databases")?.clone();
            let ips = exact_ips(&op.data["allowed_ips"])?;
            let name = db_name(&op.id);
            if s(&item.data, "engine") == "mysql" {
                let old = exact_ips(&item.data["allowed_ips"])?;
                let pw = s(&item.data, "password");
                let mut sql = String::new();
                for ip in old.iter().filter(|ip| !ips.contains(ip)) {
                    sql.push_str(&format!("DROP USER IF EXISTS '{name}'@'{ip}';\n"));
                }
                for ip in ips.iter().filter(|ip| !old.contains(ip)) {
                    sql.push_str(&format!("CREATE USER '{name}'@'{ip}' IDENTIFIED BY '{pw}' REQUIRE SSL;\nGRANT ALL PRIVILEGES ON `{name}`.* TO '{name}'@'{ip}';\n"));
                }
                if !sql.is_empty() {
                    mysql(&sql).await?;
                }
            }
            reg.items.get_mut(&op.id).unwrap().data["allowed_ips"] = json!(ips);
            db_access(reg).await?;
            Ok(json!({"ok":true}))
        }
        "create_dns" => {
            let z = reference(reg, s(&op.data, "domain_id"), &op.tenant, "domains")?;
            ensure!(
                cgpanel::dns_record(
                    s(&op.data, "type"),
                    s(&op.data, "name"),
                    s(&op.data, "value"),
                    s(&z.data, "name")
                ),
                "Invalid DNS record"
            );
            reg.items.insert(
                op.id.clone(),
                Item {
                    tenant: op.tenant.clone(),
                    kind: "dns".into(),
                    data: op.data.clone(),
                },
            );
            if let Err(e) = dns_sync(reg).await {
                reg.items.remove(&op.id);
                let _ = dns_sync(reg).await;
                return Err(e);
            }
            Ok(json!({"published":true}))
        }
        "start_app" | "stop_app" | "restart_app" | "logs_app" | "inspect_app" => {
            owned(reg, &op, "apps")?;
            let verb = op.action.split('_').next().unwrap();
            let mut a = args(&[verb]);
            if verb == "logs" {
                a.extend(args(&["--tail", "200"]));
            }
            if verb == "inspect" {
                a.extend(args(&["--format", "{{.State.Status}}"]));
            }
            a.push(container(&op.id));
            Ok(json!({"output":pod(&op.tenant,a,None,40).await?}))
        }
        "terminal" => {
            owned(reg, &op, "apps")?;
            let command = s(&op.data, "command");
            ensure!(
                !command.is_empty() && command.len() <= 4000,
                "Invalid terminal command"
            );
            let result = pod(
                &op.tenant,
                args(&[
                    "exec",
                    "--user",
                    "1000:1000",
                    "--workdir",
                    "/workspace",
                    &container(&op.id),
                    "timeout",
                    "--signal=KILL",
                    "25s",
                    "sh",
                    "-c",
                    command,
                ]),
                None,
                35,
            )
            .await;
            Ok(json!({"output":match result{Ok(s)=>s,Err(e)=>e.to_string()}}))
        }
        "files" => {
            owned(reg, &op, "apps")?;
            Ok(
                json!({"output":pod(&op.tenant,args(&["exec","--user","1000:1000",&container(&op.id),"find","/workspace","-maxdepth","3","-type","f"]),None,30).await?}),
            )
        }
        "read_file" | "write_file" => {
            owned(reg, &op, "apps")?;
            let path = s(&op.data, "path");
            ensure!(cgpanel::relative_path(path), "Invalid path");
            let full = format!("/workspace/{path}");
            if op.action == "read_file" {
                Ok(
                    json!({"output":pod(&op.tenant,args(&["exec","--user","1000:1000",&container(&op.id),"head","-c","262144","--",&full]),None,30).await?}),
                )
            } else {
                let content = s(&op.data, "content");
                ensure!(content.len() <= 262144, "File exceeds 256 KiB");
                pod(
                    &op.tenant,
                    args(&[
                        "exec",
                        "-i",
                        "--user",
                        "1000:1000",
                        &container(&op.id),
                        "sh",
                        "-c",
                        "cat > \"$1\"",
                        "cgpanel",
                        &full,
                    ]),
                    Some(content.into()),
                    30,
                )
                .await?;
                Ok(json!({"saved":true}))
            }
        }
        "domain_tls" => tls_agent::issue(reg, &op).await,
        "create_schedule" => {
            reference(reg, s(&op.data, "app_id"), &op.tenant, "apps")?;
            let next = cgpanel::schedule::next(
                s(&op.data, "schedule"),
                s(&op.data, "timezone"),
                chrono::Utc::now().timestamp(),
            )?;
            ensure!(
                !s(&op.data, "command").is_empty() && s(&op.data, "command").len() <= 2000,
                "Invalid schedule command"
            );
            let mut d = op.data.clone();
            d["next"] = json!(next);
            reg.items.insert(
                op.id.clone(),
                Item {
                    tenant: op.tenant.clone(),
                    kind: "schedules".into(),
                    data: d,
                },
            );
            Ok(json!({"state":"scheduled"}))
        }
        "schedule_status" => Ok(owned(reg, &op, "schedules")?.data.clone()),
        "create_backup" => {
            let app = s(&op.data, "app_id");
            reference(reg, app, &op.tenant, "apps")?;
            let file = format!("{ROOT}/backups/{}.tar.gz", op.id);
            let dir = appdir(&op.tenant, app); // Stream archive bytes directly to a root-owned file.
            let output = std::fs::File::create(&file)?;
            let mut child = Command::new("runuser")
                .args([
                    "-u",
                    &user(&op.tenant),
                    "--",
                    "tar",
                    "-czf",
                    "-",
                    "--one-file-system",
                    "-C",
                    &dir,
                    ".",
                ])
                .stdout(Stdio::from(output))
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()?;
            let status = tokio::time::timeout(Duration::from_secs(120), child.wait()).await??;
            ensure!(status.success(), "Backup failed");
            let size = tokio::fs::metadata(&file).await?.len();
            reg.items.insert(
                op.id.clone(),
                Item {
                    tenant: op.tenant.clone(),
                    kind: "backups".into(),
                    data: op.data.clone(),
                },
            );
            Ok(json!({"bytes":size,"scope":"application workspace"}))
        }
        "restore_backup" => {
            let item = owned(reg, &op, "backups")?;
            let app = s(&item.data, "app_id");
            reference(reg, app, &op.tenant, "apps")?;
            ensure!(
                op.data["confirm"] == "RESTORE",
                "Restore requires confirm=RESTORE"
            );
            let _ = pod(&op.tenant, args(&["stop", &container(app)]), None, 40).await;
            let input = std::fs::File::open(format!("{ROOT}/backups/{}.tar.gz", op.id))?;
            let mut child = Command::new("runuser")
                .args([
                    "-u",
                    &user(&op.tenant),
                    "--",
                    "tar",
                    "-xzf",
                    "-",
                    "--no-same-owner",
                    "--no-same-permissions",
                    "-C",
                    &appdir(&op.tenant, app),
                ])
                .stdin(Stdio::from(input))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()?;
            ensure!(
                tokio::time::timeout(Duration::from_secs(120), child.wait())
                    .await??
                    .success(),
                "Restore failed"
            );
            pod(&op.tenant, args(&["start", &container(app)]), None, 30).await?;
            Ok(json!({"restored":true}))
        }
        "create_block" => {
            let ip = s(&op.data, "name");
            ensure!(cgpanel::valid_ip(ip), "Invalid IP address");
            let set = if ip.contains(':') {
                "blocked6"
            } else {
                "blocked4"
            };
            exec(
                "nft",
                args(&["add", "element", "inet", "cgpanel", set, "{", ip, "}"]),
                None,
                30,
            )
            .await?;
            reg.items.insert(
                op.id.clone(),
                Item {
                    tenant: op.tenant.clone(),
                    kind: "blocks".into(),
                    data: op.data.clone(),
                },
            );
            Ok(json!({"blocked":true}))
        }
        action if action.starts_with("delete_") => {
            let kind = match action {
                "delete_app" => "apps",
                "delete_domain" => "domains",
                "delete_database" => "databases",
                "delete_dns" => "dns",
                "delete_schedule" => "schedules",
                "delete_backup" => "backups",
                "delete_block" => "blocks",
                _ => bail!("Unknown deletion"),
            };
            let item = owned(reg, &op, kind)?.clone();
            match kind {
                "apps" => {
                    if item.data["ide_installed"] == true {
                        workspace_agent::ide(
                            reg,
                            &Operation {
                                action: "ide_configure".into(),
                                tenant: op.tenant.clone(),
                                id: op.id.clone(),
                                data: json!({"enabled":false}),
                            },
                        )
                        .await?;
                    }
                    pod(
                        &op.tenant,
                        args(&["rm", "-f", "--ignore", &container(&op.id)]),
                        None,
                        40,
                    )
                    .await?;
                    egress_agent::cleanup(&op.tenant, &op.id).await;
                }
                "domains" => {
                    tokio::fs::remove_file(format!("/etc/nginx/conf.d/cgp_{}.conf", op.id)).await?;
                    run("nginx", &["-t"]).await?;
                    run("systemctl", &["reload", "nginx"]).await?;
                }
                "databases" => {
                    let name = db_name(&op.id);
                    if s(&item.data, "engine") == "mysql" {
                        let mut sql=format!("DROP DATABASE IF EXISTS `{name}`; DROP USER IF EXISTS '{name}'@'localhost';");
                        for ip in exact_ips(&item.data["allowed_ips"])? {
                            sql.push_str(&format!("DROP USER IF EXISTS '{name}'@'{ip}';"));
                        }
                        mysql(&sql).await?;
                    } else {
                        pg(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE); DROP ROLE IF EXISTS {name};")).await?;
                    }
                }
                "backups" => {
                    tokio::fs::remove_file(format!("{ROOT}/backups/{}.tar.gz", op.id)).await?;
                }
                "blocks" => {
                    let ip = s(&item.data, "name");
                    let set = if ip.contains(':') {
                        "blocked6"
                    } else {
                        "blocked4"
                    };
                    exec(
                        "nft",
                        args(&["delete", "element", "inet", "cgpanel", set, "{", ip, "}"]),
                        None,
                        30,
                    )
                    .await?;
                }
                _ => {}
            }
            reg.items.remove(&op.id);
            if ["domains", "dns"].contains(&kind) {
                dns_sync(reg).await?;
            }
            if kind == "databases" {
                db_access(reg).await?;
            }
            Ok(json!({"deleted":true,"workspace_retained":kind=="apps"}))
        }
        _ => bail!("Operation is not allowed"),
    }
}
async fn save(reg: &Registry) -> Result<()> {
    atomic(
        &format!("{ROOT}/registry.json"),
        &serde_json::to_string_pretty(reg)?,
        0o600,
    )
    .await
}
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().init();
    ensure!(unsafe { libc::geteuid() } == 0, "Agent must run as root");
    tokio::fs::create_dir_all(format!("{ROOT}/backups")).await?;
    tokio::fs::set_permissions(ROOT, std::fs::Permissions::from_mode(0o700)).await?;
    let registry: Registry = match tokio::fs::read_to_string(format!("{ROOT}/registry.json")).await
    {
        Ok(s) => serde_json::from_str(&s)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Registry::default(),
        Err(e) => return Err(e.into()),
    };
    // Reapply persisted database filters and block lists after firewall/service restart.
    db_access(&registry).await?;
    for item in registry.items.values().filter(|i| i.kind == "blocks") {
        let ip = s(&item.data, "name");
        let set = if ip.contains(':') {
            "blocked6"
        } else {
            "blocked4"
        };
        let _ = exec(
            "nft",
            args(&["add", "element", "inet", "cgpanel", set, "{", ip, "}"]),
            None,
            30,
        )
        .await;
    }
    // Refresh generated vhosts during upgrades so existing domains gain ACME and telemetry routes.
    for (id, item) in registry.items.iter().filter(|(_, i)| i.kind == "domains") {
        tls_agent::render_domain(&registry, id, item).await?;
    }
    workspace_agent::firewall(&registry).await?;
    let reg = Arc::new(Mutex::new(registry));
    cron_agent::start(reg.clone());
    let socket = std::env::var("CGPANEL_AGENT_SOCKET").unwrap_or("/run/cgpanel/agent.sock".into());
    if Path::new(&socket).exists() {
        tokio::fs::remove_file(&socket).await?;
    }
    let listener = tokio::net::UnixListener::bind(&socket)?;
    run("chown", &["root:cgpanel", &socket]).await?;
    tokio::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o660)).await?;
    let panel_uid: u32 = run("id", &["-u", "cgpanel"]).await?.trim().parse()?;
    loop {
        let (stream, _) = listener.accept().await?;
        let peer = stream.peer_cred()?;
        if peer.uid() != panel_uid && peer.uid() != 0 {
            continue;
        }
        let reg = reg.clone();
        tokio::spawn(async move {
            let (mut reader, mut writer) = stream.into_split();
            let mut line = String::new();
            let read = tokio::time::timeout(
                Duration::from_secs(10),
                BufReader::new((&mut reader).take(600_000)).read_line(&mut line),
            )
            .await;
            if !matches!(read, Ok(Ok(_))) {
                return;
            }
            let result = match serde_json::from_str::<Operation>(&line) {
                Ok(op)
                    if [
                        "telegram_send",
                        "integration_list",
                        "integration_test",
                        "site_seo",
                        "schedule_status",
                        "tls_status",
                        "zone_export",
                        "egress_status",
                        "workspace_files",
                        "runtime_status",
                        "runtime_catalog",
                        "ide_status",
                        "ide_password",
                        "update_status",
                    ]
                    .contains(&op.action.as_str()) =>
                {
                    // Read-only operations use the last atomically saved registry, so a backup
                    // cannot block alerts, monitoring, or integration status requests.
                    match tokio::fs::read_to_string(format!("{ROOT}/registry.json")).await {
                        Ok(data) => match serde_json::from_str::<Registry>(&data) {
                            Ok(mut snapshot) => execute(&mut snapshot, op).await,
                            Err(error) => Err(anyhow!(error)),
                        },
                        Err(error) => Err(anyhow!(error)),
                    }
                }
                Ok(op) => {
                    let mut reg = reg.lock().await;
                    let result = execute(&mut reg, op).await;
                    if let Err(e) = save(&reg).await {
                        Err(e)
                    } else {
                        result
                    }
                }
                Err(e) => Err(anyhow!(e)),
            };
            let result = match result {
                Ok(v) => json!({"ok":true,"result":v}),
                Err(e) => {
                    tracing::warn!(error=%e,"operation failed");
                    json!({"ok":false,"error":e.to_string()})
                }
            };
            let _ = writer.write_all(format!("{result}\n").as_bytes()).await;
        });
    }
}
