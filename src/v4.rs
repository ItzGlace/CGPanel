use super::*;
pub fn routes() -> Router<App> {
    Router::new()
        .route("/v4/files/{id}", post(files))
        .route("/v4/runtimes", get(catalog))
        .route("/v4/runtimes/{id}", get(runtime))
        .route("/v4/ide/{id}", get(ide))
        .route("/v4/ide/{id}/password", post(password))
        .route("/v4/updates", get(updates).post(update))
}
async fn files(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Json(v): Json<Value>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "apps" {
        return Err(bad("Choose an application"));
    }
    Ok(Json(
        host(&app, &user, "workspace_files", &r.owner, &rid, v).await?,
    ))
}
async fn catalog(State(app): State<App>, Extension(user): Extension<Identity>) -> Api<Value> {
    Ok(Json(
        host(
            &app,
            &user,
            "runtime_catalog",
            &user.id,
            &user.id,
            json!({}),
        )
        .await?,
    ))
}
async fn runtime(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "apps" {
        return Err(bad("Choose an application"));
    }
    Ok(Json(
        host(&app, &user, "runtime_status", &r.owner, &rid, json!({})).await?,
    ))
}
async fn ide(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "apps" {
        return Err(bad("Choose an application"));
    }
    Ok(Json(
        host(&app, &user, "ide_status", &r.owner, &rid, json!({})).await?,
    ))
}
async fn password(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    if user.api_token.is_some() {
        return Err(forbidden());
    }
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "apps" {
        return Err(bad("Choose an application"));
    }
    Ok(Json(
        host(&app, &user, "ide_password", &r.owner, &rid, json!({})).await?,
    ))
}
async fn updates(State(app): State<App>, Extension(user): Extension<Identity>) -> Api<Value> {
    admin_api::require_admin(&user)?;
    Ok(Json(
        host(&app, &user, "update_status", &user.id, &user.id, json!({})).await?,
    ))
}
async fn update(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(v): Json<Value>,
) -> Api<Value> {
    admin_api::require_admin(&user)?;
    Ok(Json(
        host(&app, &user, "update_configure", &user.id, &user.id, v).await?,
    ))
}
