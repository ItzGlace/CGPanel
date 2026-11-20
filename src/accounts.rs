use super::*;
pub fn routes() -> Router<App> {
    Router::new()
        .route("/users/{id}/deletion", get(preview))
        .route("/users/{id}", delete(remove))
}
async fn target(app: &App, user: &Identity, uid: &str) -> Result<String, Error> {
    if user.role != "admin" || uid == user.id {
        return Err(forbidden());
    }
    if uid.len() != 32
        || !uid
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(bad("Use the complete account ID"));
    }
    let row = sqlx::query("SELECT username,role FROM users WHERE id=?")
        .bind(uid)
        .fetch_optional(&app.db)
        .await?
        .ok_or_else(|| bad("User not found"))?;
    if row.get::<String, _>("role") != "user" {
        return Err(forbidden());
    }
    Ok(row.get("username"))
}
pub async fn ensure_active(app: &App, uid: &str) -> Result<(), Error> {
    if sqlx::query("SELECT 1 FROM account_deletions WHERE user_id=? AND state!='completed'")
        .bind(uid)
        .fetch_optional(&app.db)
        .await?
        .is_some()
    {
        return Err(bad(
            "Account deletion is pending or failed; finish it before changing this account",
        ));
    }
    Ok(())
}
async fn preview(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(uid): Path<String>,
) -> Api<Value> {
    let username = target(&app, &user, &uid).await?;
    let mut plan = host(
        &app,
        &user,
        "tenant_delete_preview",
        &uid,
        &uid,
        json!({"administrator":true}),
    )
    .await?;
    plan["username"] = json!(username);
    if let Some(row) =
        sqlx::query("SELECT state,job_id,error FROM account_deletions WHERE user_id=?")
            .bind(&uid)
            .fetch_optional(&app.db)
            .await?
    {
        plan["deletion"] = json!({"state":row.get::<String,_>("state"),"job_id":row.get::<String,_>("job_id"),"error":row.get::<String,_>("error")});
    }
    Ok(Json(plan))
}
async fn remove(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(uid): Path<String>,
    Json(value): Json<Value>,
) -> Api<Value> {
    let _guard = app.writes.lock().await;
    let username = target(&app, &user, &uid).await?;
    if value["purge_data"] != true || text(&value, "confirm_username") != username {
        return Err(bad(
            "Type the exact username and acknowledge permanent data removal",
        ));
    }
    if sqlx::query(
        "SELECT 1 FROM account_deletions WHERE user_id=? AND state IN ('queued','running')",
    )
    .bind(&uid)
    .fetch_optional(&app.db)
    .await?
    .is_some()
    {
        return Err(bad("Account deletion is already running; follow its job"));
    }
    let plan = host(
        &app,
        &user,
        "tenant_delete_preview",
        &uid,
        &uid,
        json!({"administrator":true}),
    )
    .await?;
    if !plan["blockers"].as_array().is_some_and(|b| b.is_empty()) {
        return Err(bad(
            "Resolve the ownership or storage blockers shown in the deletion preview",
        ));
    }
    if text(&value, "fingerprint").len() != 64 || value["fingerprint"] != plan["fingerprint"] {
        return Err(bad(
            "The account changed. Refresh the deletion preview before confirming",
        ));
    }
    let running:i64=sqlx::query_scalar("SELECT count(*) FROM jobs WHERE status='running' AND (owner=? OR target IN (SELECT id FROM resources WHERE owner=?))").bind(&uid).bind(&uid).fetch_one(&app.db).await?;
    if running > 0 {
        return Err(bad(
            "Wait for this account's running jobs to finish before deleting it",
        ));
    }
    let job = id();
    let mut tx = app.db.begin().await?;
    sqlx::query("UPDATE users SET enabled=0 WHERE id=?")
        .bind(&uid)
        .execute(&mut *tx)
        .await?;
    for table in ["sessions", "api_tokens", "mcp_tokens", "oauth_codes"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE user_id=?"))
            .bind(&uid)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE monitor_settings SET enabled=0 WHERE owner=?")
        .bind(&uid)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE backup_plans SET enabled=0 WHERE owner=?")
        .bind(&uid)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE jobs SET status='interrupted',error='Account scheduled for permanent deletion',finished=unixepoch() WHERE status='queued' AND (owner=? OR target IN (SELECT id FROM resources WHERE owner=?))").bind(&uid).bind(&uid).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO account_deletions(user_id,job_id,actor,username,state,updated) VALUES(?,?,?,?,'queued',unixepoch()) ON CONFLICT(user_id) DO UPDATE SET job_id=excluded.job_id,actor=excluded.actor,state='queued',error='',updated=unixepoch()").bind(&uid).bind(&job).bind(&user.id).bind(&username).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO jobs(id,owner,kind,target,payload,created) VALUES(?,?,'account_delete',?,?,unixepoch())").bind(&job).bind(&user.id).bind(&uid).bind(json!({"administrator":true,"fingerprint":value["fingerprint"]}).to_string()).execute(&mut *tx).await?;
    tx.commit().await?;
    audit(&app, &user.id, "user:delete-queued", &uid).await;
    Ok(Json(
        json!({"id":job,"status":"queued","account_suspended":true}),
    ))
}
pub async fn run(app: &App, job: &str, uid: &str, data: Value) -> anyhow::Result<Value> {
    sqlx::query("UPDATE account_deletions SET state='running',updated=unixepoch() WHERE user_id=? AND job_id=?").bind(uid).bind(job).execute(&app.db).await?;
    let result=async {
        let physical=cgpanel::agent_call_timeout(&app.agent,Operation{action:"tenant_delete".into(),tenant:uid.into(),id:uid.into(),data},3600).await?;
        anyhow::ensure!(physical["purged"]==true,"Host did not acknowledge completed data removal");
        let mut tx=app.db.begin().await?;
        for table in ["monitor_checks","analytics_events","website_challenges","website_passes","security_alerts"]{
            sqlx::query(&format!("DELETE FROM {table} WHERE domain_id IN (SELECT id FROM resources WHERE owner=?)")).bind(uid).execute(&mut *tx).await?;
        }
        for table in ["monitor_settings","backup_plans","jobs"]{sqlx::query(&format!("DELETE FROM {table} WHERE owner=?")).bind(uid).execute(&mut *tx).await?;}
        for table in ["sessions","api_tokens","mcp_tokens","oauth_codes"]{sqlx::query(&format!("DELETE FROM {table} WHERE user_id=?")).bind(uid).execute(&mut *tx).await?;}
        sqlx::query("DELETE FROM resources WHERE owner=?").bind(uid).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM users WHERE id=? AND role='user'").bind(uid).execute(&mut *tx).await?;
        sqlx::query("UPDATE account_deletions SET state='completed',error='',updated=unixepoch() WHERE user_id=?").bind(uid).execute(&mut *tx).await?;
        tx.commit().await?;Ok::<Value,anyhow::Error>(physical)
    }.await;
    if let Err(error) = &result {
        sqlx::query("UPDATE account_deletions SET state='failed',error=?,updated=unixepoch() WHERE user_id=?").bind(error.to_string().chars().take(800).collect::<String>()).bind(uid).execute(&app.db).await?;
    }
    result
}
