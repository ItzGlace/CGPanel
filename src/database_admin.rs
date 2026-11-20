use super::*;
pub fn routes() -> Router<App> {
    Router::new().route("/databases/{id}/query", post(query))
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
