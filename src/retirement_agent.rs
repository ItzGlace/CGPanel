use super::*;
async fn helper(tenant: &str, mode: &str) -> Result<Value> {
    let output = exec(
        "python3",
        args(&["/usr/local/lib/cgpanel/account-purge.py"]),
        Some(json!({"tenant":tenant,"mode":mode}).to_string()),
        if mode == "erase" { 3300 } else { 600 },
    )
    .await?;
    Ok(serde_json::from_str(&output)?)
}
fn validate(op: &Operation) -> Result<()> {
    ensure!(
        op.data["administrator"] == true
            && op.tenant == op.id
            && op.tenant.len() == 32
            && op
                .tenant
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Administrator account deletion required"
    );
    Ok(())
}
pub async fn preview(op: &Operation) -> Result<Value> {
    validate(op)?;
    Ok(helper(&op.tenant, "plan").await?["public"].clone())
}
pub async fn purge(reg: &Arc<Mutex<Registry>>, op: Operation) -> Result<Value> {
    validate(&op)?;
    static ACTIVE: std::sync::LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::LazyLock::new(Default::default);
    struct Lease(String);
    impl Drop for Lease {
        fn drop(&mut self) {
            ACTIVE.lock().unwrap().remove(&self.0);
        }
    }
    {
        ensure!(
            ACTIVE.lock().unwrap().insert(op.tenant.clone()),
            "This account is already being erased"
        );
    }
    let _lease = Lease(op.tenant.clone());
    let marker = format!("retire_{}", op.tenant);
    {
        let mut state = reg.lock().await;
        let plan = helper(&op.tenant, "plan").await?;
        if plan["completed"] == true {
            return Ok(json!({"purged":true}));
        }
        ensure!(
            plan["public"]["blockers"]
                .as_array()
                .is_some_and(|b| b.is_empty()),
            "Account ownership or mount blockers must be resolved first"
        );
        ensure!(
            plan["public"]["fingerprint"] == op.data["fingerprint"],
            "Account changed after preview; review it again"
        );
        state
            .items
            .retain(|key, item| item.tenant != op.tenant || key == &marker);
        state.items.insert(
            marker.clone(),
            Item {
                tenant: op.tenant.clone(),
                kind: "retiring_tenant".into(),
                data: json!({"state":"erasing","plan":plan}),
            },
        );
        save(&state).await?;
        helper(&op.tenant, "detach").await?;
        db_access(&state).await?;
        block_access(&state).await?;
        dns_sync(&state).await?;
        services_agent::refresh(&state).await?;
        workspace_agent::firewall(&state).await?;
        mail_agent::refresh(&state).await?;
    }
    // Removing large homes must never hold the registry lock for other accounts.
    let result = helper(&op.tenant, "erase").await?;
    ensure!(result["purged"] == true, "Account erase did not complete");
    let mut state = reg.lock().await;
    let public = state.items[&marker].data["plan"]["public"].clone();
    state.items.get_mut(&marker).unwrap().data = json!({"state":"completed","public":public});
    save(&state).await?;
    Ok(result)
}
