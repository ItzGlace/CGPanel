use super::*;
pub async fn configure(reg: &mut Registry, op: &Operation) -> Result<Value> {
    let old = owned(reg, op, "apps")?.clone();
    let mut item = old.clone();
    let cache = op.data["cache"] == true;
    ensure!(
        !cache || s(&item.data, "mode") == "web",
        "Only web applications can enable page caching"
    );
    ensure!(
        !cache || Path::new("/etc/nginx/conf.d/04-cgpanel-cache.conf").exists(),
        "Install the page cache configuration first"
    );
    item.data["performance"] = json!({"cache":cache});
    reg.items.insert(op.id.clone(), item.clone());
    let domains: Vec<_> = reg
        .items
        .iter()
        .filter(|(_, i)| i.kind == "domains" && s(&i.data, "app_id") == op.id)
        .map(|(id, i)| (id.clone(), i.clone()))
        .collect();
    for (id, domain) in &domains {
        if let Err(error) = tls_agent::render_domain(reg, id, domain).await {
            reg.items.insert(op.id.clone(), old);
            for (id, domain) in &domains {
                let _ = tls_agent::render_domain(reg, id, domain).await;
            }
            return Err(error);
        }
    }
    Ok(json!({"saved":true,"performance":item.data["performance"]}))
}
