//! Persistent login throttling and optional one-use local challenge.
use super::*;
use axum::extract::Query;
pub fn routes() -> Router<App> {
    Router::new().route("/login-security", get(settings).post(configure))
}
pub fn public_routes() -> Router<App> {
    Router::new().route("/api/login-challenge", get(challenge))
}
async fn config(app: &App) -> Result<Value, Error> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM panel_settings WHERE key='login_security'")
            .fetch_optional(&app.db)
            .await?;
    Ok(value
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or(json!({"captcha":false,"attempts":10,"window_seconds":300})))
}
async fn settings(State(app): State<App>, Extension(user): Extension<Identity>) -> Api<Value> {
    admin_api::require_admin(&user)?;
    Ok(Json(config(&app).await?))
}
async fn configure(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(v): Json<Value>,
) -> Api<Value> {
    admin_api::require_admin(&user)?;
    if user.api_token.is_some() {
        return Err(forbidden());
    }
    let attempts = v["attempts"].as_i64().unwrap_or(10);
    let window = v["window_seconds"].as_i64().unwrap_or(300);
    if !(3..=30).contains(&attempts) || !(60..=3600).contains(&window) {
        return Err(bad("Use 3–30 attempts and a 60–3600 second window"));
    }
    let v = json!({"captcha":v["captcha"]==true,"attempts":attempts,"window_seconds":window});
    sqlx::query("INSERT INTO panel_settings(key,value) VALUES('login_security',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(v.to_string()).execute(&app.db).await?;
    audit(&app, &user.id, "login_security:configure", "panel").await;
    Ok(Json(v))
}
async fn challenge(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(_q): Query<HashMap<String, String>>,
) -> Api<Value> {
    let config = config(&app).await?;
    if config["captcha"] != true || std::env::var("CGPANEL_LOGIN_RECOVERY").as_deref() == Ok("1") {
        return Ok(Json(json!({"enabled":false})));
    }
    let ip = client_ip(peer.ip(), &headers).to_string();
    let nonce = id();
    let a = rand::random::<u8>() % 20 + 1;
    let b = rand::random::<u8>() % 20 + 1;
    let _guard = app.writes.lock().await;
    sqlx::query("DELETE FROM login_challenges WHERE expires<unixepoch()")
        .execute(&app.db)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM login_challenges WHERE ip=?")
        .bind(&ip)
        .fetch_one(&app.db)
        .await?;
    if count >= 30 {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many challenges; wait five minutes".into(),
        ));
    }
    sqlx::query("INSERT INTO login_challenges VALUES(?,?,?,unixepoch()+300)")
        .bind(&nonce)
        .bind(ip)
        .bind(digest(&format!("{nonce}:{}", u16::from(a) + u16::from(b))))
        .execute(&app.db)
        .await?;
    Ok(Json(
        json!({"enabled":true,"nonce":nonce,"question":format!("What is {a} + {b}? (basic bot check)")}),
    ))
}
pub async fn check(app: &App, ip: IpAddr, input: &Login) -> Result<(), Error> {
    let config = config(app).await?;
    let now = chrono::Utc::now().timestamp();
    let window = config["window_seconds"].as_i64().unwrap_or(300);
    let _guard = app.writes.lock().await;
    sqlx::query("DELETE FROM login_attempts WHERE started<?")
        .bind(now - window)
        .execute(&app.db)
        .await?;
    for key in [
        format!("ip:{ip}"),
        format!("account:{}", digest(&input.username.to_lowercase())),
    ] {
        let count:i64=sqlx::query_scalar("INSERT INTO login_attempts(key,started,attempts) VALUES(?,?,1) ON CONFLICT(key) DO UPDATE SET attempts=attempts+1 RETURNING attempts").bind(key).bind(now).fetch_one(&app.db).await?;
        if count > config["attempts"].as_i64().unwrap_or(10) {
            return Err(Error(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many login attempts; wait for the configured window".into(),
            ));
        }
    }
    if config["captcha"] == true && std::env::var("CGPANEL_LOGIN_RECOVERY").as_deref() != Ok("1") {
        let row =
            sqlx::query("DELETE FROM login_challenges WHERE nonce=? RETURNING ip,answer,expires")
                .bind(&input.challenge)
                .fetch_optional(&app.db)
                .await?;
        if !row.is_some_and(|r| {
            r.get::<String, _>("ip") == ip.to_string()
                && r.get::<i64, _>("expires") > now
                && r.get::<String, _>("answer")
                    == digest(&format!("{}:{}", input.challenge, input.answer.trim()))
        }) {
            return Err(bad("Solve a new login challenge"));
        }
    }
    Ok(())
}
