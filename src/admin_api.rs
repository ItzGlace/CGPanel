//! Administrator automation credentials. Raw tokens are returned once and never stored.
use super::*;
use axum::extract::{OriginalUri, Query};

pub fn routes() -> Router<App> {
    Router::new()
        .route("/admin/system", get(system))
        .route("/admin/audit", get(events))
        .route("/admin/tokens", get(tokens).post(create_token))
        .route("/admin/tokens/{id}", delete(revoke_token))
        .route("/admin/openapi.json", get(openapi))
}
pub fn require_admin(user: &Identity) -> Result<(), Error> {
    if user.role != "admin" {
        return Err(forbidden());
    }
    Ok(())
}
fn session_admin(user: &Identity) -> Result<(), Error> {
    require_admin(user)?;
    if user.api_token.is_some() {
        return Err(forbidden());
    }
    Ok(())
}
pub struct TokenRequest {
    authorization: String,
    peer: Option<IpAddr>,
    path: String,
    method: String,
}
impl TokenRequest {
    pub fn from_request(req: &Request) -> Self {
        Self {
            authorization: req
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_owned(),
            peer: req
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|p| client_ip(p.0.ip(), req.headers())),
            path: req
                .extensions()
                .get::<OriginalUri>()
                .map(|u| u.0.path())
                .unwrap_or(req.uri().path())
                .to_owned(),
            method: req.method().to_string(),
        }
    }
}
pub async fn authenticate_token(app: &App, req: TokenRequest) -> Result<Identity, Error> {
    let unauthorized = || {
        Error(
            StatusCode::UNAUTHORIZED,
            "Invalid or expired API token".into(),
        )
    };
    let authorization = &req.authorization;
    let (scheme, token) = authorization.split_once(' ').ok_or_else(unauthorized)?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || token.len() != 68
        || !token.starts_with("cgp_")
        || !token[4..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(unauthorized());
    }
    let row = sqlx::query("SELECT t.id AS token_id,t.scope,t.allowed_ips AS token_ips,u.id,u.username,u.role,u.allowed_ips FROM api_tokens t JOIN users u ON u.id=t.user_id WHERE t.token_hash=? AND t.revoked IS NULL AND t.expires>unixepoch() AND u.enabled=1 AND u.role='admin'")
        .bind(digest(token)).fetch_optional(&app.db).await?.ok_or_else(unauthorized)?;
    let peer = req.peer;
    if !ip_allowed(&row.get::<String, _>("allowed_ips"), peer)
        || !ip_allowed(&row.get::<String, _>("token_ips"), peer)
    {
        return Err(forbidden());
    }
    let path = req.path.as_str();
    let path = path.strip_prefix("/api").unwrap_or(path);
    // Tokens cannot mint other credentials or change the owner's password/session.
    if path == "/admin/tokens"
        || path.starts_with("/admin/tokens/")
        || ["/password", "/logout"].contains(&path)
    {
        return Err(forbidden());
    }
    let scope: String = row.get("scope");
    if scope == "read" && !["GET", "HEAD"].contains(&req.method.as_str()) {
        return Err(forbidden());
    }
    let token_id: String = row.get("token_id");
    sqlx::query("UPDATE api_tokens SET last_used=unixepoch() WHERE id=? AND (last_used IS NULL OR last_used<unixepoch()-60)")
        .bind(&token_id).execute(&app.db).await?;
    let identity = Identity {
        id: row.get("id"),
        username: row.get("username"),
        role: row.get("role"),
        csrf: String::new(),
        api_token: Some(token_id.clone()),
    };
    if !["GET", "HEAD"].contains(&req.method.as_str()) {
        audit(
            app,
            &identity.id,
            &format!("api_token:{token_id}:{}", req.method),
            path,
        )
        .await;
    }
    Ok(identity)
}
async fn system(State(app): State<App>, Extension(user): Extension<Identity>) -> Api<Value> {
    require_admin(&user)?;
    overview(State(app), Extension(user)).await
}
async fn openapi(Extension(user): Extension<Identity>) -> Result<Response, Error> {
    require_admin(&user)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=CGPanel-openapi.json",
            ),
        ],
        include_str!("../docs/openapi.json"),
    )
        .into_response())
}
async fn events(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Query(q): Query<HashMap<String, String>>,
) -> Api<Value> {
    require_admin(&user)?;
    let limit = q
        .get("limit")
        .map(|v| v.parse::<i64>())
        .transpose()
        .map_err(|_| bad("Invalid limit"))?
        .unwrap_or(50);
    let before = q
        .get("before")
        .map(|v| v.parse::<i64>())
        .transpose()
        .map_err(|_| bad("Invalid cursor"))?
        .unwrap_or(i64::MAX);
    if !(1..=200).contains(&limit) || before < 1 {
        return Err(bad("Use limit 1–200 and a positive before cursor"));
    }
    let rows = sqlx::query("SELECT a.id,a.actor,a.action,a.target,a.created,coalesce(u.username,a.actor) AS username FROM audit a LEFT JOIN users u ON u.id=a.actor WHERE a.id<? ORDER BY a.id DESC LIMIT ?")
        .bind(before).bind(limit).fetch_all(&app.db).await?;
    let events:Vec<Value> = rows.iter().map(|r|json!({"id":r.get::<i64,_>("id"),"actor":r.get::<String,_>("actor"),"username":r.get::<String,_>("username"),"action":r.get::<String,_>("action"),"target":r.get::<String,_>("target"),"created":r.get::<String,_>("created")})).collect();
    let next = if rows.len() == limit as usize {
        rows.last().map(|r| r.get::<i64, _>("id"))
    } else {
        None
    };
    Ok(Json(json!({"events":events,"next_before":next})))
}
async fn tokens(State(app): State<App>, Extension(user): Extension<Identity>) -> Api<Value> {
    session_admin(&user)?;
    let rows = sqlx::query("SELECT id,name,prefix,scope,allowed_ips,created,expires,last_used,revoked FROM api_tokens WHERE user_id=? ORDER BY created DESC LIMIT 200")
        .bind(user.id).fetch_all(&app.db).await?;
    Ok(Json(json!(rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"name":r.get::<String,_>("name"),"prefix":r.get::<String,_>("prefix"),"scope":r.get::<String,_>("scope"),"allowed_ips":serde_json::from_str::<Value>(&r.get::<String,_>("allowed_ips")).unwrap_or(json!([])),"created":r.get::<i64,_>("created"),"expires":r.get::<i64,_>("expires"),"last_used":r.get::<Option<i64>,_>("last_used"),"revoked":r.get::<Option<i64>,_>("revoked")})).collect::<Vec<_>>())))
}
async fn create_token(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(v): Json<Value>,
) -> Api<Value> {
    session_admin(&user)?;
    let name = text(&v, "name").trim();
    let scope = v["scope"].as_str().unwrap_or("read");
    let days = v["expires_days"].as_i64().unwrap_or(30);
    if name.is_empty()
        || name.len() > 64
        || name.chars().any(char::is_control)
        || !["read", "admin"].contains(&scope)
        || !(1..=90).contains(&days)
    {
        return Err(bad(
            "Use a name of 1–64 characters, read/admin scope and expiry of 1–90 days",
        ));
    }
    let ips = validated_ips(&v["allowed_ips"])?;
    let _guard = app.writes.lock().await;
    let count:i64 = sqlx::query_scalar("SELECT count(*) FROM api_tokens WHERE user_id=? AND revoked IS NULL AND expires>unixepoch()")
        .bind(&user.id).fetch_one(&app.db).await?;
    if count >= 20 {
        return Err(bad("Maximum 20 active API tokens per administrator"));
    }
    let rid = id();
    let token = format!("cgp_{}{}", id(), id());
    let prefix = &token[..12];
    let now = chrono::Utc::now().timestamp();
    let expires = now + days * 86400;
    sqlx::query("INSERT INTO api_tokens(id,user_id,name,token_hash,prefix,scope,allowed_ips,created,expires) VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(&rid).bind(&user.id).bind(name).bind(digest(&token)).bind(prefix).bind(scope).bind(ips.to_string()).bind(now).bind(expires).execute(&app.db).await?;
    audit(&app, &user.id, "api_token:create", &rid).await;
    Ok(Json(
        json!({"id":rid,"name":name,"token":token,"prefix":prefix,"scope":scope,"allowed_ips":ips,"created":now,"expires":expires}),
    ))
}
async fn revoke_token(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    session_admin(&user)?;
    let result = sqlx::query(
        "UPDATE api_tokens SET revoked=coalesce(revoked,unixepoch()) WHERE id=? AND user_id=?",
    )
    .bind(&rid)
    .bind(&user.id)
    .execute(&app.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(Error(StatusCode::NOT_FOUND, "Token not found".into()));
    }
    audit(&app, &user.id, "api_token:revoke", &rid).await;
    Ok(Json(json!({"revoked":true})))
}
