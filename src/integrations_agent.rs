use super::*;
use cgpanel::outbound;
use reqwest::Url;

pub fn redacted(id: &str, item: &Item) -> Value {
    let mut value = json!({"id":id,"owner":item.tenant});
    for key in [
        "name",
        "type",
        "proxy_id",
        "endpoint",
        "bucket",
        "region",
        "prefix",
        "host",
        "port",
        "username",
        "remote_dir",
        "chat_id",
    ] {
        if let Some(v) = item.data.get(key) {
            value[key] = v.clone();
        }
    }
    value["configured"] = json!(true);
    value
}
pub fn connection<'a>(reg: &'a Registry, tenant: &str, id: &str, kind: &str) -> Result<&'a Item> {
    let item = reference(reg, id, tenant, "integrations")?;
    ensure!(
        s(&item.data, "type") == kind,
        "Integration type does not match this operation"
    );
    Ok(item)
}
pub fn proxy(reg: &Registry, tenant: &str, item: &Item) -> Result<Option<String>> {
    let id = s(&item.data, "proxy_id");
    if id.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        s(&connection(reg, tenant, id, "proxy")?.data, "url").into(),
    ))
}
fn required<'a>(value: &'a Value, key: &str, max: usize) -> Result<&'a str> {
    let value = s(value, key);
    ensure!(
        !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control),
        "Missing or invalid {key}"
    );
    Ok(value)
}
fn safe_path(path: &str) -> bool {
    path.len() <= 500
        && path
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"/._-".contains(&c))
        && path.split('/').all(|part| part != ".." && part != ".")
}
pub async fn save_integration(reg: &mut Registry, op: &Operation) -> Result<Value> {
    ensure!(op.id.len() == 32, "Invalid integration ID");
    if let Some(old) = reg.items.get(&op.id) {
        ensure!(
            old.kind == "integrations" && old.tenant == op.tenant,
            "Integration access denied"
        );
    }
    let mut data = op.data.clone();
    ensure!(
        op.data["administrator"] == true
            || !reg.items.values().any(|i| i.kind == "apps"
                && i.data["egress_locked"] == true
                && s(&i.data, "egress_proxy_id") == op.id),
        "An administrator has locked this proxy integration"
    );
    if let Some(old) = reg.items.get(&op.id) {
        ensure!(
            s(&old.data, "type") == s(&data, "type"),
            "Integration type cannot be changed"
        );
        for key in [
            "token",
            "url",
            "access_key",
            "secret_key",
            "private_key",
            "host_key",
            "ca_pem",
        ] {
            if s(&data, key).is_empty() {
                if let Some(value) = old.data.get(key) {
                    data[key] = value.clone();
                }
            }
        }
    } else {
        ensure!(
            reg.items
                .values()
                .filter(|i| i.kind == "integrations" && i.tenant == op.tenant)
                .count()
                < 40,
            "Maximum 40 integrations per account"
        );
    }
    required(&data, "name", 80)?;
    let kind = required(&data, "type", 20)?.to_owned();
    let proxy_id = s(&data, "proxy_id");
    if !proxy_id.is_empty() {
        connection(reg, &op.tenant, proxy_id, "proxy")?;
    }
    match kind.as_str() {
        "proxy" => {
            ensure!(proxy_id.is_empty(), "Proxy chaining is not supported");
            outbound::proxy_url(required(&data, "url", 2048)?).await?;
        }
        "telegram" => {
            let token = required(&data, "token", 200)?;
            ensure!(
                token.contains(':')
                    && token
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b":_-".contains(&c)),
                "Invalid Telegram bot token"
            );
            let chat = required(&data, "chat_id", 100)?;
            ensure!(
                chat.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"@_-".contains(&c)),
                "Invalid Telegram chat ID"
            );
        }
        "s3" => {
            let url = Url::parse(required(&data, "endpoint", 500)?)?;
            ensure!(
                url.scheme() == "https"
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none()
                    && url.path() == "/",
                "S3 endpoint must be an HTTPS origin"
            );
            outbound::addresses(
                url.host_str().context("Invalid S3 host")?,
                url.port_or_known_default().unwrap_or(443),
            )
            .await?;
            for key in ["bucket", "region"] {
                ensure!(
                    required(&data, key, 100)?
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c)),
                    "Invalid S3 {key}"
                );
            }
            required(&data, "access_key", 256)?;
            required(&data, "secret_key", 256)?;
            if !s(&data, "ca_pem").is_empty() {
                ensure!(
                    s(&data, "ca_pem").len() <= 20000,
                    "CA certificate is too large"
                );
                reqwest::Certificate::from_pem(s(&data, "ca_pem").as_bytes())
                    .context("Invalid CA certificate")?;
            }
            ensure!(safe_path(s(&data, "prefix")), "Invalid object prefix");
        }
        "ssh" => {
            let host = required(&data, "host", 253)?;
            let port = data["port"].as_u64().unwrap_or(22);
            ensure!((1..=65535).contains(&port), "Invalid SSH port");
            outbound::addresses(host, port as u16).await?;
            ensure!(
                required(&data, "username", 64)?
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
                "Invalid SSH username"
            );
            let path = required(&data, "remote_dir", 500)?;
            ensure!(
                path.starts_with('/') && safe_path(path),
                "Use an absolute SSH destination path without parent segments"
            );
            let key = s(&data, "private_key");
            ensure!(
                key.len() < 20000 && key.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"),
                "Supply an unencrypted OpenSSH private key"
            );
            let host_key = required(&data, "host_key", 2000)?;
            let fields: Vec<_> = host_key.split_whitespace().collect();
            ensure!(
                fields.len() == 2
                    && ["ssh-ed25519", "ssh-rsa", "ecdsa-sha2-nistp256"].contains(&fields[0])
                    && fields[1]
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"+/=".contains(&c)),
                "Supply the verified server host key: key-type base64-key"
            );
            data["port"] = json!(port);
        }
        "cloudflare" => {
            required(&data, "token", 256)?;
        }
        _ => bail!("Unknown integration type"),
    }
    let item = Item {
        tenant: op.tenant.clone(),
        kind: "integrations".into(),
        data,
    };
    let result = redacted(&op.id, &item);
    reg.items.insert(op.id.clone(), item);
    Ok(result)
}
pub async fn telegram(reg: &Registry, tenant: &str, id: &str, message: &str) -> Result<Value> {
    let mut last = anyhow!("Telegram delivery failed");
    for delay in [0, 5, 20] {
        if delay > 0 {
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
        match telegram_once(reg, tenant, id, message).await {
            Ok(value) => return Ok(value),
            Err(error) => last = error,
        }
    }
    Err(last)
}
async fn telegram_once(reg: &Registry, tenant: &str, id: &str, message: &str) -> Result<Value> {
    let item = connection(reg, tenant, id, "telegram")?;
    ensure!(message.len() <= 4000, "Message is too long");
    let url = Url::parse(&format!(
        "https://api.telegram.org/bot{}/sendMessage",
        s(&item.data, "token")
    ))?;
    let proxy = proxy(reg, tenant, item)?;
    let response = outbound::client(&url,proxy.as_deref(),25).await?.post(url)
        .json(&json!({"chat_id":s(&item.data,"chat_id"),"text":message,"disable_web_page_preview":true})).send().await
        .map_err(|_| anyhow!("Telegram connection failed; check the integration and proxy"))?;
    let status = response.status();
    let body = outbound::bounded_body(response, 65536).await?;
    let value: Value = serde_json::from_slice(&body).context("Invalid Telegram response")?;
    ensure!(
        status.is_success() && value["ok"] == true,
        "Telegram rejected the message; check bot access to the configured chat"
    );
    Ok(json!({"sent":true}))
}
pub async fn execute(reg: &mut Registry, op: &Operation) -> Result<Value> {
    match op.action.as_str() {
        "integration_save" => save_integration(reg, op).await,
        "integration_list" => Ok(json!(reg
            .items
            .iter()
            .filter(|(_, i)| i.kind == "integrations" && i.tenant == op.tenant)
            .map(|(id, i)| redacted(id, i))
            .collect::<Vec<_>>())),
        "integration_delete" => {
            owned(reg, op, "integrations")?;
            ensure!(
                !reg.items
                    .values()
                    .any(|i| s(&i.data, "proxy_id") == op.id
                        || s(&i.data, "egress_proxy_id") == op.id),
                "Integration is still referenced"
            );
            reg.items.remove(&op.id);
            Ok(json!({"deleted":true}))
        }
        "telegram_send" => telegram(reg, &op.tenant, &op.id, s(&op.data, "message")).await,
        _ => bail!("Unknown integration operation"),
    }
}
