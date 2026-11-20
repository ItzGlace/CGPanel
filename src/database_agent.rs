use super::*;
pub async fn query(reg: &Registry, op: &Operation) -> Result<Value> {
    let item = owned(reg, op, "databases")?;
    let sql = s(&op.data, "sql");
    ensure!(
        !sql.trim().is_empty() && sql.len() <= 262144,
        "Invalid SQL size"
    );
    let result=exec("python3",args(&["/usr/local/lib/cgpanel/database-query.py"]),Some(json!({"database":db_name(&op.id),"engine":item.data["engine"],"password":item.data["password"],"sql":sql}).to_string()),35).await?;
    Ok(serde_json::from_str(&result)?)
}
