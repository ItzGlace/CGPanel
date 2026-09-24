use super::*;
use axum::{body::Body, extract::Query};
use tokio_util::io::ReaderStream;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v2/integrations", get(integrations).post(save_integration))
        .route("/v2/integrations/{id}", delete(delete_integration))
        .route("/v2/jobs", get(jobs).post(queue))
        .route("/v2/jobs/{id}/retry", post(retry))
        .route("/v2/cron-preview", post(cron_preview))
        .route("/v2/backups", get(backups))
        .route("/v2/backups/{id}", delete(delete_backup))
        .route("/v2/backups/{id}/download", get(download))
        .route("/v2/backup-plans", get(plans).post(save_plan))
        .route("/v2/backup-plans/{id}", delete(delete_plan))
        .route("/v2/domains/{id}/tls", get(tls_status))
        .route("/v2/domains/{id}/zone", get(zone_export))
        .route("/v2/domains/{id}/cdn", post(cdn))
        .route("/v2/egress/{id}", get(egress_status))
}
pub async fn owner(app: &App, user: &Identity, requested: &str) -> Result<String, Error> {
    let owner = if user.role == "admin" && !requested.is_empty() {
        requested
    } else {
        &user.id
    };
    if sqlx::query("SELECT 1 FROM users WHERE id=? AND enabled=1")
        .bind(owner)
        .fetch_optional(&app.db)
        .await?
        .is_none()
    {
        return Err(bad("Active account not found"));
    }
    Ok(owner.into())
}
async fn integrations(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Query(query): Query<HashMap<String, String>>,
) -> Api<Value> {
    let owner = owner(
        &app,
        &user,
        query.get("owner").map(String::as_str).unwrap_or(""),
    )
    .await?;
    Ok(Json(
        host(&app, &user, "integration_list", &owner, &owner, json!({})).await?,
    ))
}
async fn save_integration(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(mut value): Json<Value>,
) -> Api<Value> {
    let owner = owner(&app, &user, text(&value, "owner")).await?;
    let rid = if text(&value, "id").is_empty() {
        id()
    } else {
        text(&value, "id").into()
    };
    value["administrator"] = json!(user.role == "admin");
    Ok(Json(
        host(&app, &user, "integration_save", &owner, &rid, value).await?,
    ))
}
async fn delete_integration(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Api<Value> {
    let owner = owner(
        &app,
        &user,
        query.get("owner").map(String::as_str).unwrap_or(""),
    )
    .await?;
    let configs: Vec<String> = sqlx::query_scalar("SELECT config FROM monitor_settings WHERE owner=? UNION ALL SELECT config FROM backup_plans WHERE owner=?")
        .bind(&owner).bind(&owner).fetch_all(&app.db).await?;
    if configs.iter().any(|s| s.contains(&rid)) {
        return Err(bad(
            "Remove this integration from monitoring and backup plans first",
        ));
    }
    Ok(Json(
        host(&app, &user, "integration_delete", &owner, &rid, json!({})).await?,
    ))
}
pub async fn enqueue(
    app: &App,
    owner: &str,
    kind: &str,
    target: &str,
    payload: Value,
) -> Result<String, Error> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE owner=? AND status IN ('queued','running')",
    )
    .bind(owner)
    .fetch_one(&app.db)
    .await?;
    if count >= if kind == "monitor_alert" { 100 } else { 10 } {
        return Err(bad("This account has reached its pending job limit"));
    }
    let rid = id();
    sqlx::query("INSERT INTO jobs(id,owner,kind,target,payload,created) VALUES(?,?,?,?,?,?)")
        .bind(&rid)
        .bind(owner)
        .bind(kind)
        .bind(target)
        .bind(payload.to_string())
        .bind(chrono::Utc::now().timestamp())
        .execute(&app.db)
        .await?;
    Ok(rid)
}
async fn queue(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(mut value): Json<Value>,
) -> Api<Value> {
    let kind = text(&value, "kind").to_owned();
    let target = text(&value, "target").to_owned();
    let requested_owner = text(&value, "owner").to_owned();
    let owner = if kind == "panel_tls" {
        if user.role != "admin" {
            return Err(forbidden());
        }
        value["administrator"] = json!(true);
        user.id.clone()
    } else if ["integration_test", "backup_deliver"].contains(&kind.as_str()) {
        owner(&app, &user, &requested_owner).await?
    } else {
        let resource = owned(&app, &user, &target).await?;
        let expected = match kind.as_str() {
            "full_backup" | "restore_full" | "egress" | "runtime" | "ide" => "apps",
            "tls" | "seo" => "domains",
            _ => return Err(bad("Unsupported background job")),
        };
        if resource.kind != expected {
            return Err(bad("Wrong resource type"));
        }
        resource.owner
    };
    if kind == "egress" {
        value["administrator"] = json!(user.role == "admin");
    }
    // Jobs reference protected integrations; credentials must never be included in queued payloads.
    for key in ["token", "password", "secret_key", "private_key", "url"] {
        if value.get(key).is_some() {
            return Err(bad("Save credentials in Integrations first"));
        }
    }
    let _guard = app.writes.lock().await;
    let rid = enqueue(&app, &owner, &kind, &target, value).await?;
    audit(&app, &user.id, "job:queued", &rid).await;
    Ok(Json(json!({"id":rid,"status":"queued"})))
}
async fn jobs(State(app): State<App>, Extension(user): Extension<Identity>) -> Api<Value> {
    let rows = sqlx::query(
        "SELECT * FROM jobs WHERE owner=? OR ?='admin' ORDER BY created DESC LIMIT 100",
    )
    .bind(&user.id)
    .bind(&user.role)
    .fetch_all(&app.db)
    .await?;
    Ok(Json(json!(rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"owner":r.get::<String,_>("owner"),"kind":r.get::<String,_>("kind"),"target":r.get::<String,_>("target"),"status":r.get::<String,_>("status"),"result":serde_json::from_str::<Value>(&r.get::<String,_>("result")).unwrap_or(json!({})),"error":r.get::<String,_>("error"),"created":r.get::<i64,_>("created"),"started":r.get::<Option<i64>,_>("started"),"finished":r.get::<Option<i64>,_>("finished")})).collect::<Vec<_>>())))
}
async fn retry(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    let r = sqlx::query("SELECT * FROM jobs WHERE id=? AND (owner=? OR ?='admin') AND status IN ('failed','interrupted')")
        .bind(&rid).bind(&user.id).bind(&user.role).fetch_optional(&app.db).await?.ok_or_else(|| bad("Failed job not found"))?;
    let kind: String = r.get("kind");
    if [
        "restore_full",
        "tls",
        "panel_tls",
        "egress",
        "runtime",
        "ide",
    ]
    .contains(&kind.as_str())
    {
        return Err(bad("Review the operation and submit a new job"));
    }
    let next = enqueue(
        &app,
        &r.get::<String, _>("owner"),
        &kind,
        &r.get::<String, _>("target"),
        serde_json::from_str(&r.get::<String, _>("payload")).unwrap_or(json!({})),
    )
    .await?;
    Ok(Json(json!({"id":next,"status":"queued"})))
}
async fn cron_preview(Json(value): Json<Value>) -> Api<Value> {
    let mut next = chrono::Utc::now().timestamp();
    let mut dates = Vec::new();
    for _ in 0..5 {
        next = cgpanel::schedule::next(text(&value, "schedule"), text(&value, "timezone"), next)
            .map_err(|e| bad(e.to_string()))?;
        dates.push(next);
    }
    Ok(Json(json!({"next":dates})))
}
async fn backups(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Query(query): Query<HashMap<String, String>>,
) -> Api<Value> {
    let owner = owner(
        &app,
        &user,
        query.get("owner").map(String::as_str).unwrap_or(""),
    )
    .await?;
    Ok(Json(
        host(&app, &user, "full_backup_list", &owner, &owner, json!({})).await?,
    ))
}
async fn download(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, Error> {
    if !identifier(&rid) || rid.len() != 32 {
        return Err(bad("Invalid archive ID"));
    }
    let owner = owner(
        &app,
        &user,
        query.get("owner").map(String::as_str).unwrap_or(""),
    )
    .await?;
    host(&app, &user, "full_backup_export", &owner, &rid, json!({})).await?;
    let file = tokio::fs::File::open(format!("/var/lib/cgpanel-exports/{rid}.cgp"))
        .await
        .map_err(|_| bad("Archive is not available"))?;
    let len = file
        .metadata()
        .await
        .map_err(|_| bad("Cannot read archive"))?
        .len();
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (header::CONTENT_LENGTH, len.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{rid}.cgp\""),
            ),
        ],
        Body::from_stream(ReaderStream::new(file)),
    )
        .into_response())
}
async fn delete_backup(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Api<Value> {
    let owner = owner(
        &app,
        &user,
        query.get("owner").map(String::as_str).unwrap_or(""),
    )
    .await?;
    Ok(Json(
        host(&app, &user, "full_backup_delete", &owner, &rid, json!({})).await?,
    ))
}
pub async fn start(app: App) -> Result<(), Error> {
    sqlx::query("UPDATE jobs SET status='interrupted',error='Panel restarted while this job was running; inspect the target before retrying',finished=? WHERE status='running'")
        .bind(chrono::Utc::now().timestamp()).execute(&app.db).await?;
    for alerts in [false, true] {
        let app = app.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = run_one(&app, alerts).await {
                    tracing::warn!(error=%error.1,"background job dispatch failed");
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
    Ok(())
}
async fn run_one(app: &App, alerts: bool) -> Result<(), Error> {
    let Some(row) = sqlx::query("SELECT j.* FROM jobs j JOIN users u ON u.id=j.owner WHERE j.status='queued' AND (j.kind='monitor_alert')=? AND u.enabled=1 ORDER BY j.created LIMIT 1").bind(alerts).fetch_optional(&app.db).await? else { return Ok(()); };
    let rid: String = row.get("id");
    let owner: String = row.get("owner");
    let kind: String = row.get("kind");
    let target: String = row.get("target");
    sqlx::query("UPDATE jobs SET status='running',started=? WHERE id=?")
        .bind(chrono::Utc::now().timestamp())
        .bind(&rid)
        .execute(&app.db)
        .await?;
    let mut data: Value =
        serde_json::from_str(&row.get::<String, _>("payload")).unwrap_or(json!({}));
    data["job_id"] = json!(rid);
    let action = match kind.as_str() {
        "full_backup" => "full_backup_create",
        "backup_deliver" => "full_backup_deliver",
        "restore_full" => "full_backup_restore",
        "integration_test" => "integration_test",
        "tls" => "tls_issue",
        "panel_tls" => "tls_panel_ip",
        "seo" => "site_seo",
        "egress" => "egress_configure",
        "runtime" => "runtime_configure",
        "ide" => "ide_configure",
        "monitor_alert" => "telegram_send",
        _ => "invalid_job",
    };
    let result = cgpanel::agent_call_timeout(
        &app.agent,
        Operation {
            action: action.into(),
            tenant: owner,
            id: target,
            data,
        },
        3600,
    )
    .await;
    let (status, result, error) = match result {
        Ok(value) => ("succeeded", value, String::new()),
        Err(error) => (
            "failed",
            json!({}),
            error.to_string().chars().take(800).collect(),
        ),
    };
    sqlx::query("UPDATE jobs SET status=?,result=?,error=?,finished=? WHERE id=?")
        .bind(status)
        .bind(result.to_string())
        .bind(error)
        .bind(chrono::Utc::now().timestamp())
        .bind(&rid)
        .execute(&app.db)
        .await?;
    Ok(())
}

async fn plans(State(app): State<App>, Extension(user): Extension<Identity>) -> Api<Value> {
    let rows = sqlx::query("SELECT * FROM backup_plans WHERE owner=? OR ?='admin'")
        .bind(&user.id)
        .bind(&user.role)
        .fetch_all(&app.db)
        .await?;
    Ok(Json(json!(rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"owner":r.get::<String,_>("owner"),"app_id":r.get::<String,_>("app_id"),"config":serde_json::from_str::<Value>(&r.get::<String,_>("config")).unwrap_or(json!({})),"next_run":r.get::<i64,_>("next_run"),"enabled":r.get::<bool,_>("enabled"),"last_job":r.get::<String,_>("last_job")})).collect::<Vec<_>>())))
}
async fn save_plan(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(mut value): Json<Value>,
) -> Api<Value> {
    let resource = owned(&app, &user, text(&value, "app_id")).await?;
    if resource.kind != "apps" {
        return Err(bad("Choose an application"));
    }
    let next = cgpanel::schedule::next(
        text(&value, "schedule"),
        text(&value, "timezone"),
        chrono::Utc::now().timestamp(),
    )
    .map_err(|e| bad(e.to_string()))?;
    for db in value["database_ids"]
        .as_array()
        .cloned()
        .unwrap_or_default()
    {
        let database = owned(&app, &user, db.as_str().unwrap_or("")).await?;
        if database.kind != "databases" || database.owner != resource.owner {
            return Err(forbidden());
        }
    }
    let integrations = host(
        &app,
        &user,
        "integration_list",
        &resource.owner,
        &resource.owner,
        json!({}),
    )
    .await?;
    for dest in value["destinations"]
        .as_array()
        .cloned()
        .unwrap_or_default()
    {
        if !integrations.as_array().is_some_and(|all| {
            all.iter()
                .any(|i| i["id"] == dest && ["telegram", "s3", "ssh"].contains(&text(i, "type")))
        }) {
            return Err(bad("Choose a storage integration owned by this account"));
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM backup_plans WHERE owner=?")
        .bind(&resource.owner)
        .fetch_one(&app.db)
        .await?;
    if count >= 20 {
        return Err(bad("Maximum 20 backup plans per account"));
    }
    value = json!({"database_ids":value["database_ids"],"destinations":value["destinations"],"quiesce":value["quiesce"]!=false,"schedule":text(&value,"schedule"),"timezone":text(&value,"timezone"),"retention":value["retention"].as_i64().unwrap_or(7).clamp(1,30),"enabled":value["enabled"]!=false});
    let rid = id();
    value["plan_id"] = json!(rid);
    sqlx::query(
        "INSERT INTO backup_plans(id,owner,app_id,config,next_run,enabled) VALUES(?,?,?,?,?,?)",
    )
    .bind(&rid)
    .bind(&resource.owner)
    .bind(&resource.id)
    .bind(value.to_string())
    .bind(next)
    .bind(value["enabled"] != false)
    .execute(&app.db)
    .await?;
    Ok(Json(json!({"id":rid,"next_run":next})))
}
async fn delete_plan(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    sqlx::query("DELETE FROM backup_plans WHERE id=? AND (owner=? OR ?='admin')")
        .bind(rid)
        .bind(user.id)
        .bind(user.role)
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"deleted":true})))
}
pub fn start_plans(app: App) {
    tokio::spawn(async move {
        loop {
            let _ = plan_tick(&app).await;
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });
}
async fn plan_tick(app: &App) -> Result<(), Error> {
    let now = chrono::Utc::now().timestamp();
    let rows=sqlx::query("SELECT p.* FROM backup_plans p JOIN users u ON u.id=p.owner JOIN resources r ON r.id=p.app_id WHERE p.enabled=1 AND p.next_run<=? AND u.enabled=1 LIMIT 20").bind(now).fetch_all(&app.db).await?;
    for row in rows {
        let plan: String = row.get("id");
        let owner: String = row.get("owner");
        let target: String = row.get("app_id");
        let config: Value =
            serde_json::from_str(&row.get::<String, _>("config")).unwrap_or(json!({}));
        let next =
            cgpanel::schedule::next(text(&config, "schedule"), text(&config, "timezone"), now)
                .map_err(|e| bad(e.to_string()))?;
        let pending:i64=sqlx::query_scalar("SELECT count(*) FROM jobs WHERE target=? AND kind='full_backup' AND status IN ('queued','running')").bind(&target).fetch_one(&app.db).await?;
        if pending == 0 {
            let job = enqueue(app, &owner, "full_backup", &target, config).await?;
            sqlx::query("UPDATE backup_plans SET last_job=? WHERE id=?")
                .bind(job)
                .bind(&plan)
                .execute(&app.db)
                .await?;
        }
        sqlx::query("UPDATE backup_plans SET next_run=? WHERE id=?")
            .bind(next)
            .bind(&plan)
            .execute(&app.db)
            .await?;
    }
    Ok(())
}

async fn tls_status(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "domains" {
        return Err(bad("Select a domain"));
    }
    Ok(Json(
        host(&app, &user, "tls_status", &r.owner, &rid, json!({})).await?,
    ))
}
async fn zone_export(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Result<Response, Error> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "domains" {
        return Err(bad("Select a domain"));
    }
    let data = host(&app, &user, "zone_export", &r.owner, &rid, json!({})).await?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                "text/plain; charset=utf-8".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}.zone\"", r.name),
            ),
        ],
        text(&data, "zone").to_string(),
    )
        .into_response())
}
async fn cdn(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Json(value): Json<Value>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "domains" {
        return Err(bad("Select a domain"));
    }
    Ok(Json(
        host(&app, &user, "cdn_configure", &r.owner, &rid, value).await?,
    ))
}
async fn egress_status(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "apps" {
        return Err(bad("Select an application"));
    }
    Ok(Json(
        host(&app, &user, "egress_status", &r.owner, &rid, json!({})).await?,
    ))
}
