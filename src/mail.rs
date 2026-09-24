use super::*;
pub fn routes() -> Router<App> {
    Router::new()
        .route("/v5/mail", get(status))
        .route("/v5/mail/server", post(configure))
        .route("/v5/mail/domains/{id}", post(domain_state))
        .route("/v5/mail/mailboxes", post(save_mailbox))
}
async fn status(State(app): State<App>, Extension(user): Extension<Identity>) -> Api<Value> {
    Ok(Json(
        host(
            &app,
            &user,
            "mail_status",
            &user.id,
            &user.id,
            json!({"all":user.role=="admin"}),
        )
        .await?,
    ))
}
async fn configure(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(value): Json<Value>,
) -> Api<Value> {
    admin_api::require_admin(&user)?;
    let _guard = app.writes.lock().await;
    Ok(Json(
        host(&app, &user, "mail_configure", &user.id, &user.id, value).await?,
    ))
}
async fn domain_state(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Json(value): Json<Value>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "domains" {
        return Err(bad("Choose a domain"));
    }
    let _guard = app.writes.lock().await;
    Ok(Json(
        host(&app, &user, "mail_domain", &r.owner, &rid, value).await?,
    ))
}
async fn save_mailbox(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(value): Json<Value>,
) -> Api<Value> {
    let r = owned(&app, &user, text(&value, "domain_id")).await?;
    if r.kind != "domains" {
        return Err(bad("Choose a domain"));
    }
    let rid = if text(&value, "id").is_empty() {
        id()
    } else {
        text(&value, "id").to_owned()
    };
    let _guard = app.writes.lock().await;
    Ok(Json(
        host(&app, &user, "mailbox_save", &r.owner, &rid, value).await?,
    ))
}
