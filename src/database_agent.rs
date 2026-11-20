use super::*;
pub async fn query(reg: &Registry, op: &Operation) -> Result<Value> {
    let item = owned(reg, op, "databases")?;
    let sql = s(&op.data, "sql");
    ensure!(
        op.action == "database_studio" || (!sql.trim().is_empty() && sql.len() <= 262144),
        "Invalid SQL size"
    );
    let mut value = if op.action == "database_studio" {
        op.data.clone()
    } else {
        json!({"action":"sql","sql":sql})
    };
    value["database"] = json!(db_name(&op.id));
    value["engine"] = item.data["engine"].clone();
    value["password"] = item.data["password"].clone();
    let result = exec(
        "python3",
        args(&["/usr/local/lib/cgpanel/database-query.py"]),
        Some(value.to_string()),
        35,
    )
    .await?;
    Ok(serde_json::from_str(&result)?)
}
