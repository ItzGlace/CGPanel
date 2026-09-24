use super::*;
use totp_rs::{Builder, Secret, Totp};

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v5/mfa", get(status))
        .route("/v5/mfa/enroll", post(enroll))
        .route("/v5/mfa/confirm", post(confirm))
        .route("/v5/mfa/disable", post(disable))
}
fn totp(secret: &str, username: &str) -> Result<Totp, Error> {
    Builder::new()
        .with_secret(
            Secret::try_from_base32(secret).map_err(|_| bad("Invalid authenticator secret"))?,
        )
        .with_account_name(username)
        .with_issuer(Some("CGPanel"))
        .build()
        .map_err(|_| bad("Invalid authenticator configuration"))
}
async fn throttle(app: &App, uid: &str) -> Result<(), Error> {
    let count:i64=sqlx::query_scalar("INSERT INTO mfa_attempts(user_id,started,attempts) VALUES(?,unixepoch(),1) ON CONFLICT(user_id) DO UPDATE SET attempts=CASE WHEN started<unixepoch()-300 THEN 1 ELSE attempts+1 END,started=CASE WHEN started<unixepoch()-300 THEN unixepoch() ELSE started END RETURNING attempts")
        .bind(uid).fetch_one(&app.db).await?;
    if count > 10 {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Wait five minutes before trying another MFA code".into(),
        ));
    }
    Ok(())
}
async fn password(app: &App, user: &Identity, value: &Value) -> Result<(), Error> {
    if user.api_token.is_some() {
        return Err(bad("Use a browser session to manage MFA"));
    }
    throttle(app, &user.id).await?;
    let supplied = text(value, "password").to_owned();
    if supplied.len() > 256 {
        return Err(bad("Invalid password"));
    }
    let hash: String = sqlx::query_scalar("SELECT password FROM users WHERE id=?")
        .bind(&user.id)
        .fetch_one(&app.db)
        .await?;
    let valid = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|h| {
            Argon2::default()
                .verify_password(supplied.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false);
    if !valid {
        return Err(Error(StatusCode::UNAUTHORIZED, "Invalid password".into()));
    }
    Ok(())
}
pub async fn verify(app: &App, uid: &str, code: &str) -> Result<bool, Error> {
    let secret: Option<String> = sqlx::query_scalar("SELECT secret FROM user_mfa WHERE user_id=?")
        .bind(uid)
        .fetch_optional(&app.db)
        .await?;
    let Some(secret) = secret else {
        return Ok(true);
    };
    throttle(app, uid).await?;
    if code.len() > 128 {
        return Ok(false);
    }
    if let Some(step) = totp(&secret, uid)?.check(code, chrono::Utc::now().timestamp() as u64) {
        let changed =
            sqlx::query("UPDATE user_mfa SET last_step=? WHERE user_id=? AND last_step<?")
                .bind(step as i64)
                .bind(uid)
                .bind(step as i64)
                .execute(&app.db)
                .await?
                .rows_affected();
        return Ok(changed == 1);
    }
    let changed = sqlx::query("DELETE FROM mfa_recovery WHERE user_id=? AND code_hash=?")
        .bind(uid)
        .bind(digest(code.trim()))
        .execute(&app.db)
        .await?
        .rows_affected();
    Ok(changed == 1)
}
async fn status(State(app): State<App>, Extension(user): Extension<Identity>) -> Api<Value> {
    let enabled: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_mfa WHERE user_id=?")
        .bind(&user.id)
        .fetch_one(&app.db)
        .await?;
    let recovery: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mfa_recovery WHERE user_id=?")
        .bind(&user.id)
        .fetch_one(&app.db)
        .await?;
    Ok(Json(
        json!({"enabled":enabled==1,"recovery_codes_remaining":recovery}),
    ))
}
async fn enroll(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(value): Json<Value>,
) -> Api<Value> {
    password(&app, &user, &value).await?;
    let _lock = app.writes.lock().await;
    let enabled: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_mfa WHERE user_id=?")
        .bind(&user.id)
        .fetch_one(&app.db)
        .await?;
    if enabled != 0 {
        return Err(bad("MFA is already enabled"));
    }
    let secret = Secret::generate().to_base32();
    let url = totp(&secret, &user.username)?
        .to_url()
        .map_err(|_| bad("Cannot create enrollment link"))?;
    sqlx::query("INSERT INTO mfa_pending(user_id,secret,expires) VALUES(?,?,unixepoch()+600) ON CONFLICT(user_id) DO UPDATE SET secret=excluded.secret,expires=excluded.expires")
        .bind(&user.id).bind(&secret).execute(&app.db).await?;
    Ok(Json(
        json!({"secret":secret,"otpauth_url":url,"expires_in":600}),
    ))
}
async fn confirm(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(value): Json<Value>,
) -> Api<Value> {
    if user.api_token.is_some() {
        return Err(forbidden());
    }
    throttle(&app, &user.id).await?;
    let _lock = app.writes.lock().await;
    let secret: Option<String> = sqlx::query_scalar(
        "SELECT secret FROM mfa_pending WHERE user_id=? AND expires>unixepoch()",
    )
    .bind(&user.id)
    .fetch_optional(&app.db)
    .await?;
    let secret = secret.ok_or_else(|| bad("Enrollment expired; start again"))?;
    let step = totp(&secret, &user.username)?
        .check(text(&value, "code"), chrono::Utc::now().timestamp() as u64)
        .ok_or_else(|| bad("Invalid authenticator code"))?;
    let codes = (0..10).map(|_| id()).collect::<Vec<_>>();
    let mut tx = app.db.begin().await?;
    sqlx::query("INSERT INTO user_mfa(user_id,secret,last_step) VALUES(?,?,?)")
        .bind(&user.id)
        .bind(secret)
        .bind(step as i64)
        .execute(&mut *tx)
        .await?;
    for code in &codes {
        sqlx::query("INSERT INTO mfa_recovery(user_id,code_hash) VALUES(?,?)")
            .bind(&user.id)
            .bind(digest(code))
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM mfa_pending WHERE user_id=?")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id=? AND csrf<>?")
        .bind(&user.id)
        .bind(&user.csrf)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit(&app, &user.id, "mfa:enabled", "").await;
    Ok(Json(json!({"enabled":true,"recovery_codes":codes})))
}
async fn disable(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(value): Json<Value>,
) -> Api<Value> {
    password(&app, &user, &value).await?;
    let _lock = app.writes.lock().await;
    if !verify(&app, &user.id, text(&value, "code")).await? {
        return Err(bad("Invalid MFA or recovery code"));
    }
    let mut tx = app.db.begin().await?;
    for table in ["user_mfa", "mfa_recovery", "mfa_pending"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE user_id=?"))
            .bind(&user.id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM sessions WHERE user_id=? AND csrf<>?")
        .bind(&user.id)
        .bind(&user.csrf)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit(&app, &user.id, "mfa:disabled", "").await;
    Ok(Json(json!({"enabled":false})))
}
