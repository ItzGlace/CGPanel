use super::*;
pub fn routes() -> Router<App> {
    Router::new()
        .route("/databases/{id}/query", post(query))
        .route("/databases/{id}/studio", post(studio))
        .route("/databases/{id}/size", get(size))
}
async fn size(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "databases" {
        return Err(bad("Choose a database"));
    }
    Ok(Json(
        host(
            &app,
            &user,
            "database_studio",
            &r.owner,
            &rid,
            json!({"action":"size"}),
        )
        .await?,
    ))
}
async fn studio(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Json(v): Json<Value>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "databases" {
        return Err(bad("Choose a database"));
    }
    if ![
        "catalog",
        "table",
        "create_table",
        "drop_table",
        "alter_table",
        "insert",
        "update",
        "delete",
        "truncate",
    ]
    .contains(&text(&v, "action"))
    {
        return Err(bad("Choose a database workspace operation"));
    }
    Ok(Json(
        host(&app, &user, "database_studio", &r.owner, &rid, v).await?,
    ))
}
async fn query(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Json(v): Json<Value>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "databases" {
        return Err(bad("Choose a database"));
    }
    let sql = text(&v, "sql");
    if sql.trim().is_empty() || sql.len() > 262144 {
        return Err(bad("SQL must be between 1 and 262144 bytes"));
    }
    // SQL intentionally executes with the database-specific role, never a host administrator.
    Ok(Json(
        host(
            &app,
            &user,
            "database_query",
            &r.owner,
            &rid,
            json!({"sql":sql}),
        )
        .await?,
    ))
}
