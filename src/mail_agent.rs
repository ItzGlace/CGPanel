use super::*;
fn settings(reg: &Registry) -> Value {
    reg.items
        .get("mail_settings")
        .map(|i| i.data.clone())
        .unwrap_or(json!({"hostname":"","enabled":false,"certificate_domain":""}))
}
async fn apply(reg: &Registry) -> Result<Value> {
    let cfg = settings(reg);
    let hostname = s(&cfg, "hostname");
    ensure!(
        cgpanel::domain(hostname),
        "Configure the server mail hostname first"
    );
    let certificate = s(&cfg, "certificate_domain");
    let cert = if certificate.is_empty() {
        String::new()
    } else {
        let item = reg
            .items
            .get(certificate)
            .filter(|i| i.kind == "domains" && s(&i.data, "name") == hostname)
            .context("Certificate must belong to the mail hostname")?;
        let name = s(&item.data, "cert_name");
        ensure!(
            name == format!("cgp_{certificate}") || name == hostname,
            "Issue a certificate for the mail hostname first"
        );
        name.to_owned()
    };
    let domains = reg
        .items
        .values()
        .filter(|i| i.kind == "domains" && i.data["mail_enabled"] == true)
        .map(|i| i.data["name"].clone())
        .collect::<Vec<_>>();
    let boxes = reg
        .items
        .iter()
        .filter(|(_, i)| {
            i.kind == "mailboxes"
                && i.data["enabled"] == true
                && reg.items.get(s(&i.data, "domain_id")).is_some_and(|d| {
                    d.kind == "domains" && d.tenant == i.tenant && d.data["mail_enabled"] == true
                })
        })
        .map(|(id, i)| {
            let mut v = i.data.clone();
            v["id"] = json!(id);
            v["owner"] = json!(i.tenant);
            v
        })
        .collect::<Vec<_>>();
    let result = exec(
        "python3",
        args(&["/usr/local/lib/cgpanel/mail-config.py"]),
        Some(
            json!({"hostname":hostname,"cert":cert,"domains":domains,"mailboxes":boxes})
                .to_string(),
        ),
        180,
    )
    .await?;
    let rules = if cfg["enabled"] == true {
        "flush set inet cgpanel mail_ports\nadd element inet cgpanel mail_ports { 25, 465, 587, 993 }\n"
    } else {
        "flush set inet cgpanel mail_ports\n"
    };
    exec("nft", args(&["-f", "-"]), Some(rules.into()), 30).await?;
    Ok(serde_json::from_str(&result)?)
}
pub async fn execute(reg: &mut Registry, op: &Operation) -> Result<Value> {
    match op.action.as_str() {
        "mail_status" => {
            let cfg = settings(reg);
            let mut domains = vec![];
            for (id, item) in reg.items.iter().filter(|(_, i)| {
                (op.data["all"] == true || i.tenant == op.tenant) && i.kind == "domains"
            }) {
                let name = s(&item.data, "name");
                let dkim = tokio::fs::read_to_string(format!("/var/lib/rspamd/dkim/{name}.txt"))
                    .await
                    .unwrap_or_default();
                domains.push(json!({"id":id,"name":name,"enabled":item.data["mail_enabled"]==true,"dkim":dkim}));
            }
            let boxes=reg.items.iter().filter(|(_,i)|(op.data["all"]==true||i.tenant==op.tenant)&&i.kind=="mailboxes").map(|(id,i)|json!({"id":id,"address":i.data["address"],"quota_mb":i.data["quota_mb"],"enabled":i.data["enabled"],"domain_id":i.data["domain_id"]})).collect::<Vec<_>>();
            Ok(
                json!({"settings":cfg,"domains":domains,"mailboxes":boxes,"server_ip":std::env::var("CGPANEL_PUBLIC_IP").unwrap_or_default(),"installed":Path::new("/usr/local/lib/cgpanel/mail-config.py").exists()}),
            )
        }
        "mail_configure" => {
            ensure!(
                cgpanel::domain(s(&op.data, "hostname")) && op.data["enabled"].is_boolean(),
                "Enter a valid mail hostname and service state"
            );
            let old = reg.items.get("mail_settings").cloned();
            reg.items.insert(
                "mail_settings".into(),
                Item {
                    tenant: op.tenant.clone(),
                    kind: "mail_settings".into(),
                    data: op.data.clone(),
                },
            );
            match apply(reg).await {
                Ok(v) => Ok(v),
                Err(e) => {
                    if let Some(old) = old {
                        reg.items.insert("mail_settings".into(), old);
                    } else {
                        reg.items.remove("mail_settings");
                    }
                    Err(e)
                }
            }
        }
        "mail_domain" => {
            let mut item = owned(reg, op, "domains")?.clone();
            ensure!(op.data["enabled"].is_boolean(), "Select mail state");
            if op.data["enabled"] != true {
                ensure!(
                    !reg.items.values().any(|i| i.kind == "mailboxes"
                        && i.data["domain_id"] == op.id
                        && i.data["enabled"] == true),
                    "Disable this domain's mailboxes first"
                );
            }
            let old = item.clone();
            item.data["mail_enabled"] = op.data["enabled"].clone();
            reg.items.insert(op.id.clone(), item);
            if let Err(e) = apply(reg).await {
                reg.items.insert(op.id.clone(), old);
                return Err(e);
            }
            Ok(json!({"saved":true}))
        }
        "mailbox_save" => {
            let existing = reg.items.get(&op.id).cloned();
            if existing.is_some() {
                owned(reg, op, "mailboxes")?;
            }
            ensure!(op.id.len() == 32, "Invalid mailbox ID");
            let domain = reference(reg, s(&op.data, "domain_id"), &op.tenant, "domains")?;
            ensure!(
                domain.data["mail_enabled"] == true,
                "Enable email for this domain first"
            );
            let local = s(&op.data, "local");
            ensure!(
                !local.is_empty()
                    && local.len() <= 64
                    && local.bytes().all(|c| c.is_ascii_lowercase()
                        || c.is_ascii_digit()
                        || b"._-".contains(&c))
                    && local.as_bytes()[0].is_ascii_alphanumeric(),
                "Use a simple lowercase mailbox name"
            );
            let address = format!("{local}@{}", s(&domain.data, "name"));
            ensure!(
                !reg.items.iter().any(|(id, i)| id != &op.id
                    && i.kind == "mailboxes"
                    && i.data["address"] == address),
                "Mailbox already exists"
            );
            let quota = op.data["quota_mb"].as_u64().unwrap_or(0);
            ensure!(
                (64..=102400).contains(&quota),
                "Mailbox quota must be 64–102400 MiB"
            );
            ensure!(op.data["enabled"].is_boolean(), "Select mailbox state");
            ensure!(
                existing.is_some()
                    || reg
                        .items
                        .values()
                        .filter(|i| i.tenant == op.tenant && i.kind == "mailboxes")
                        .count()
                        < 30,
                "Account mailbox limit reached (30)"
            );
            let password = s(&op.data, "password");
            let hash = if password.is_empty() {
                existing
                    .as_ref()
                    .map(|i| s(&i.data, "hash").to_owned())
                    .context("Set an initial mailbox password")?
            } else {
                ensure!(
                    (14..=128).contains(&password.len()) && !password.contains(['\n', '\r', '\0']),
                    "Use a 14–128 character mailbox password"
                );
                format!(
                    "{{SHA512-CRYPT}}{}",
                    exec(
                        "openssl",
                        args(&["passwd", "-6", "-stdin"]),
                        Some(format!("{password}\n")),
                        20
                    )
                    .await?
                    .trim()
                )
            };
            let data = json!({"address":address,"domain_id":op.data["domain_id"],"quota_mb":quota,"enabled":op.data["enabled"],"hash":hash});
            reg.items.insert(
                op.id.clone(),
                Item {
                    tenant: op.tenant.clone(),
                    kind: "mailboxes".into(),
                    data,
                },
            );
            if let Err(e) = apply(reg).await {
                if let Some(old) = existing {
                    reg.items.insert(op.id.clone(), old);
                } else {
                    reg.items.remove(&op.id);
                }
                return Err(e);
            }
            Ok(json!({"saved":true,"id":op.id,"address":address}))
        }
        _ => bail!("Unknown mail operation"),
    }
}

pub async fn refresh(reg: &Registry) -> Result<()> {
    if !s(&settings(reg), "hostname").is_empty() {
        apply(reg).await?;
    }
    Ok(())
}
pub async fn suspend(reg: &mut Registry, tenant: &str) -> Result<()> {
    for item in reg
        .items
        .values_mut()
        .filter(|i| i.tenant == tenant && i.kind == "mailboxes")
    {
        item.data["enabled"] = json!(false);
    }
    refresh(reg).await
}
