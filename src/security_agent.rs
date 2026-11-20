use super::*;
fn email(value: &str) -> bool {
    value.len() <= 254
        && value.split_once('@').is_some_and(|(local, host)| {
            !local.is_empty()
                && local
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
                && cgpanel::domain(host)
        })
}
pub async fn send_email(reg: &Registry, op: &Operation) -> Result<Value> {
    let domain = owned(reg, op, "domains")?;
    let from = s(&op.data, "from");
    let to = s(&op.data, "to");
    let message = s(&op.data, "message");
    ensure!(
        email(from) && email(to) && message.len() <= 2000 && !message.contains('\r'),
        "Invalid alert email"
    );
    ensure!(
        reg.items.values().any(|i| i.tenant == op.tenant
            && i.kind == "mailboxes"
            && s(&i.data, "address") == from
            && i.data["enabled"] == true
            && reg
                .items
                .get(s(&i.data, "domain_id"))
                .is_some_and(|d| d.data["mail_enabled"] == true)),
        "Choose an enabled mailbox owned by this account for alert email"
    );
    ensure!(
        from.split_once('@')
            .is_some_and(|(_, host)| cgpanel::within_domain(s(&domain.data, "name"), host)),
        "Alert sender must belong to this monitored domain"
    );
    let content=format!("From: {from}\nTo: {to}\nSubject: CGPanel service alert\nAuto-Submitted: auto-generated\nContent-Type: text/plain; charset=UTF-8\n\n{message}\n");
    exec(
        "/usr/sbin/sendmail",
        args(&["-i", "-f", from, "--", to]),
        Some(content),
        30,
    )
    .await?;
    Ok(json!({"queued":true,"note":"Accepted by local MTA; remote delivery is not yet confirmed"}))
}
pub async fn status(reg: &Registry, op: &Operation) -> Result<Value> {
    let mut traffic = protection_agent::stats(reg, op).await?;
    let output = exec(
        "journalctl",
        args(&[
            "-u",
            "mariadb",
            "-u",
            "postgresql",
            "--since",
            "5 minutes ago",
            "--no-pager",
            "-n",
            "5000",
            "-o",
            "cat",
        ]),
        None,
        15,
    )
    .await;
    let names: Vec<_> = reg
        .items
        .iter()
        .filter(|(_, i)| i.kind == "databases" && i.tenant == op.tenant)
        .map(|(id, _)| db_name(id))
        .collect();
    let failures = output
        .as_ref()
        .map(|o| {
            o.lines()
                .filter(|line| {
                    let lower = line.to_ascii_lowercase();
                    (lower.contains("access denied") || lower.contains("authentication failed"))
                        && names.iter().any(|n| line.contains(n))
                })
                .count()
        })
        .unwrap_or(0);
    traffic["database_auth_failures"] = json!(failures);
    traffic["database_journal_available"] = json!(output.is_ok());
    traffic["database_scope"]=json!("Failed-login messages mentioning this account's database roles in the last five minutes. Database logging must be enabled; successful unauthorized queries cannot be inferred from these logs.");
    Ok(traffic)
}
