use super::*;
use axum::body::Body;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v5/apps/{id}/console", post(console))
        .route("/v5/network-range", post(network_range))
        .route("/v5/apps/{id}/git", post(git))
        .route(
            "/v5/apps/{id}/services",
            get(services).post(configure_services),
        )
        .route("/v5/users/{id}/budget", get(budget).post(set_budget))
        .route("/v5/apps/{id}/limits", post(set_limits))
        .route("/v5/apps/{id}/backup-import", post(import_backup))
}
async fn import_backup(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Json(value): Json<Value>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "apps" {
        return Err(bad("Choose an application"));
    }
    Ok(Json(
        host(&app, &user, "backup_import", &r.owner, &rid, value).await?,
    ))
}
async fn budget(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(uid): Path<String>,
) -> Api<Value> {
    if user.role != "admin" && user.id != uid {
        return Err(forbidden());
    }
    Ok(Json(
        host(&app, &user, "budget_status", &uid, &uid, json!({})).await?,
    ))
}
async fn set_budget(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(uid): Path<String>,
    Json(value): Json<Value>,
) -> Api<Value> {
    admin_api::require_admin(&user)?;
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM users WHERE id=?")
        .bind(&uid)
        .fetch_optional(&app.db)
        .await?;
    if exists.is_none() {
        return Err(bad("User not found"));
    }
    let _guard = app.writes.lock().await;
    Ok(Json(
        host(&app, &user, "budget_configure", &uid, &uid, value).await?,
    ))
}
async fn set_limits(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Json(value): Json<Value>,
) -> Api<Value> {
    let mut r = owned(&app, &user, &rid).await?;
    if r.kind != "apps" {
        return Err(bad("Choose an application"));
    }
    let _guard = app.writes.lock().await;
    let result = host(
        &app,
        &user,
        "allocation_configure",
        &r.owner,
        &rid,
        value.clone(),
    )
    .await?;
    for key in ["memory_mb", "cpu_millis", "disk_mb"] {
        r.data[key] = value[key].clone();
    }
    sqlx::query("UPDATE resources SET data=? WHERE id=?")
        .bind(r.data.to_string())
        .bind(&rid)
        .execute(&app.db)
        .await?;
    Ok(Json(result))
}
async fn services(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "apps" {
        return Err(bad("Choose an application"));
    }
    Ok(Json(
        host(&app, &user, "services_status", &r.owner, &rid, json!({})).await?,
    ))
}
async fn configure_services(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Json(value): Json<Value>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "apps" {
        return Err(bad("Choose an application"));
    }
    let _guard = app.writes.lock().await;
    Ok(Json(
        host(&app, &user, "services_configure", &r.owner, &rid, value).await?,
    ))
}
async fn git(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Json(value): Json<Value>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "apps" {
        return Err(bad("Choose an application"));
    }
    Ok(Json(
        host(&app, &user, "workspace_git", &r.owner, &rid, value).await?,
    ))
}

async fn network_range(Json(value): Json<Value>) -> Api<Value> {
    let values = text(&value, "value");
    if values.len() > 4096 {
        return Err(bad("Too many addresses"));
    }
    let ranges = values
        .split([',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|ip| match cgpanel::network::range(ip) {
            Ok((first, last)) => json!({"input":ip,"first":first,"last":last}),
            Err(_) => json!({"input":ip,"error":"Invalid IP address or prefix"}),
        })
        .collect::<Vec<_>>();
    Ok(Json(json!(ranges)))
}

async fn console(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Json(value): Json<Value>,
) -> Result<Response, Error> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "apps" {
        return Err(bad("Choose an application"));
    }
    let command = text(&value, "command");
    if command.is_empty() || command.len() > 4000 || command.contains('\0') {
        return Err(bad("Invalid command"));
    }
    #[cfg(unix)]
    {
        let mut stream = tokio::net::UnixStream::connect(&app.agent)
            .await
            .map_err(|e| bad(e.to_string()))?;
        let op = Operation {
            action: "console_stream".into(),
            tenant: r.owner,
            id: rid.clone(),
            data: value,
        };
        stream
            .write_all(format!("{}\n", serde_json::to_string(&op).unwrap()).as_bytes())
            .await
            .map_err(|e| bad(e.to_string()))?;
        audit(&app, &user.id, "console:start", &rid).await;
        Ok((
            [
                (header::CONTENT_TYPE, "application/x-ndjson"),
                (header::CACHE_CONTROL, "no-store"),
                (header::HeaderName::from_static("x-accel-buffering"), "no"),
            ],
            Body::from_stream(ReaderStream::new(stream)),
        )
            .into_response())
    }
    #[cfg(not(unix))]
    {
        Err(bad("Console requires Linux"))
    }
}
