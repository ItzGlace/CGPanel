use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
};
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Path, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post},
    Extension, Json, Router,
};
use cgpanel::{agent_call, domain, identifier, within_domain, Operation};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone)]
struct App {
    db: SqlitePool,
    agent: String,
    secure: bool,
    limits: Arc<Mutex<HashMap<IpAddr, (Instant, u32)>>>,
    writes: Arc<Mutex<()>>,
}
#[derive(Clone, Serialize)]
struct Identity {
    id: String,
    username: String,
    role: String,
    csrf: String,
}
#[derive(Debug)]
struct Error(StatusCode, String);
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}
impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(error=%e,"database error");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database operation failed".into(),
        )
    }
}
type Api<T> = Result<Json<T>, Error>;
include!(concat!(env!("OUT_DIR"), "/assets.rs"));
async fn asset(Path(path): Path<String>) -> Response {
    match embedded_asset(&path) {
        Some((mime, bytes)) => ([(header::CONTENT_TYPE, mime)], bytes).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
fn bad(s: impl Into<String>) -> Error {
    Error(StatusCode::BAD_REQUEST, s.into())
}
fn forbidden() -> Error {
    Error(StatusCode::FORBIDDEN, "Access denied".into())
}
fn id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
fn digest(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
async fn password_hash(password: String) -> Result<String, Error> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|v| v.to_string())
    })
    .await
    .map_err(|_| bad("Hashing failed"))?
    .map_err(|_| bad("Hashing failed"))
}
async fn audit(app: &App, actor: &str, action: &str, target: &str) {
    let _ = sqlx::query("INSERT INTO audit(actor,action,target) VALUES(?,?,?)")
        .bind(actor)
        .bind(action)
        .bind(target)
        .execute(&app.db)
        .await;
}
async fn host(
    app: &App,
    actor: &Identity,
    action: &str,
    owner: &str,
    rid: &str,
    data: Value,
) -> Result<Value, Error> {
    let result = agent_call(
        &app.agent,
        Operation {
            action: action.into(),
            tenant: owner.into(),
            id: rid.into(),
            data,
        },
    )
    .await;
    audit(
        app,
        &actor.id,
        &format!("{action}:{}", if result.is_ok() { "ok" } else { "failed" }),
        rid,
    )
    .await;
    result.map_err(|e| {
        tracing::warn!(error=%e,action,"agent operation failed");
        bad(e.to_string().chars().take(1200).collect::<String>())
    })
}

async fn authenticate(State(app): State<App>, mut req: Request, next: Next) -> Response {
    let cookie = req
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = cookie
        .split(';')
        .map(str::trim)
        .find_map(|s| s.strip_prefix("cg_session="))
        .unwrap_or("");
    if token.len() != 64 {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"Sign in required"})),
        )
            .into_response();
    }
    let row = sqlx::query("SELECT u.id,u.username,u.role,u.allowed_ips,s.csrf FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=? AND s.expires>unixepoch() AND u.enabled=1").bind(digest(token)).fetch_optional(&app.db).await;
    let row = match row {
        Ok(Some(r)) => r,
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error":"Session expired"})),
            )
                .into_response()
        }
    };
    let allowed: String = row.get("allowed_ips");
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|x| client_ip(x.0.ip(), req.headers()));
    if !ip_allowed(&allowed, peer) {
        return forbidden().into_response();
    }
    let identity = Identity {
        id: row.get("id"),
        username: row.get("username"),
        role: row.get("role"),
        csrf: row.get("csrf"),
    };
    if req.method() != "GET"
        && req
            .headers()
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            != Some(identity.csrf.as_str())
    {
        return forbidden().into_response();
    }
    req.extensions_mut().insert(identity);
    next.run(req).await
}
fn ip_allowed(allowed: &str, ip: Option<IpAddr>) -> bool {
    let nets: Vec<String> =
        serde_json::from_str(allowed).unwrap_or_else(|_| vec!["invalid".into()]);
    nets.is_empty()
        || ip.is_some_and(|ip| {
            nets.iter()
                .any(|n| n.parse::<ipnet::IpNet>().is_ok_and(|n| n.contains(&ip)))
        })
}
fn client_ip(peer: IpAddr, headers: &HeaderMap) -> IpAddr {
    // Only the loopback Nginx listener is trusted to set this header. Nginx replaces it.
    if peer.is_loopback() {
        headers
            .get("x-cgpanel-client-ip")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(peer)
    } else {
        peer
    }
}
async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    for (k,v) in [("x-content-type-options","nosniff"),("x-frame-options","DENY"),("referrer-policy","no-referrer"),("cache-control","no-store"),("content-security-policy","default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"),("permissions-policy","camera=(), microphone=(), geolocation=()")] { res.headers_mut().insert(axum::http::HeaderName::from_static(k),v.parse().unwrap()); }
    res
}

#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}
async fn login(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> Result<Response, Error> {
    let peer = SocketAddr::new(client_ip(peer.ip(), &headers), peer.port());
    let mut limits = app.limits.lock().await;
    limits.retain(|_, (t, _)| t.elapsed() < Duration::from_secs(300));
    if limits.len() > 10000 && !limits.contains_key(&peer.ip()) {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Try again later".into(),
        ));
    }
    let entry = limits.entry(peer.ip()).or_insert((Instant::now(), 0));
    entry.1 += 1;
    if entry.1 > 10 {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many attempts; wait five minutes".into(),
        ));
    }
    drop(limits);
    if input.password.len() > 256 || input.username.len() > 32 {
        return Err(bad("Invalid credentials"));
    }
    let row =
        sqlx::query("SELECT id,password,allowed_ips FROM users WHERE username=? AND enabled=1")
            .bind(&input.username)
            .fetch_optional(&app.db)
            .await?;
    let dummy="$argon2id$v=19$m=19456,t=2,p=1$c29tZXJhbmRvbXNhbHQ$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned();
    let hash = row
        .as_ref()
        .map(|r| r.get::<String, _>("password"))
        .unwrap_or(dummy);
    let valid = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|h| {
            Argon2::default()
                .verify_password(input.password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false);
    if !valid || row.is_none() {
        audit(&app, "anonymous", "login:failed", &peer.ip().to_string()).await;
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "Invalid credentials".into(),
        ));
    }
    let row = row.unwrap();
    if !ip_allowed(&row.get::<String, _>("allowed_ips"), Some(peer.ip())) {
        return Err(forbidden());
    }
    let token = format!("{}{}", id(), id());
    let csrf = id();
    let uid: String = row.get("id");
    sqlx::query("DELETE FROM sessions WHERE expires<unixepoch()")
        .execute(&app.db)
        .await?;
    sqlx::query(
        "INSERT INTO sessions(token_hash,user_id,csrf,expires) VALUES(?,?,?,unixepoch()+28800)",
    )
    .bind(digest(&token))
    .bind(&uid)
    .bind(&csrf)
    .execute(&app.db)
    .await?;
    audit(&app, &uid, "login:ok", &peer.ip().to_string()).await;
    let cookie = format!(
        "cg_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=28800{}",
        if app.secure { "; Secure" } else { "" }
    );
    Ok(([(header::SET_COOKIE, cookie)], Json(json!({"csrf":csrf}))).into_response())
}
async fn me(Extension(user): Extension<Identity>) -> Json<Identity> {
    Json(user)
}
async fn logout(
    State(app): State<App>,
    headers: HeaderMap,
    Extension(user): Extension<Identity>,
) -> Result<Response, Error> {
    let token = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .map(str::trim)
        .find_map(|s| s.strip_prefix("cg_session="))
        .unwrap_or("");
    sqlx::query("DELETE FROM sessions WHERE token_hash=?")
        .bind(digest(token))
        .execute(&app.db)
        .await?;
    audit(&app, &user.id, "logout", "session").await;
    Ok((
        [(
            header::SET_COOKIE,
            "cg_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        )],
        Json(json!({"ok":true})),
    )
        .into_response())
}
async fn overview(State(app): State<App>, Extension(user): Extension<Identity>) -> Api<Value> {
    let rows = sqlx::query(
        "SELECT kind,count(*) AS total FROM resources WHERE owner=? OR ?='admin' GROUP BY kind",
    )
    .bind(&user.id)
    .bind(&user.role)
    .fetch_all(&app.db)
    .await?;
    let counts: serde_json::Map<String, Value> = rows
        .iter()
        .map(|r| (r.get::<String, _>("kind"), json!(r.get::<i64, _>("total"))))
        .collect();
    let events=sqlx::query("SELECT a.action,a.target,a.created,coalesce(u.username,a.actor) AS actor FROM audit a LEFT JOIN users u ON u.id=a.actor WHERE a.actor=? OR ?='admin' ORDER BY a.id DESC LIMIT 20").bind(&user.id).bind(&user.role).fetch_all(&app.db).await?;
    let events:Vec<Value>=events.iter().map(|r|json!({"action":r.get::<String,_>("action"),"target":r.get::<String,_>("target"),"created":r.get::<String,_>("created"),"actor":r.get::<String,_>("actor")})).collect();
    let stats = if user.role == "admin" {
        agent_call(
            &app.agent,
            Operation {
                action: "health".into(),
                tenant: user.id.clone(),
                id: "health".into(),
                data: json!({}),
            },
        )
        .await
        .unwrap_or(json!({"agent":"unavailable"}))
    } else {
        json!({"agent":"restricted"})
    };
    Ok(Json(
        json!({"counts":counts,"events":events,"host":stats,"version":env!("CARGO_PKG_VERSION")}),
    ))
}
async fn users(State(app): State<App>, Extension(user): Extension<Identity>) -> Api<Value> {
    if user.role != "admin" {
        return Err(forbidden());
    }
    let rows = sqlx::query(
        "SELECT id,username,role,enabled,allowed_ips,quota,created FROM users ORDER BY created",
    )
    .fetch_all(&app.db)
    .await?;
    Ok(Json(json!(rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"username":r.get::<String,_>("username"),"role":r.get::<String,_>("role"),"enabled":r.get::<i64,_>("enabled"),"allowed_ips":serde_json::from_str::<Value>(&r.get::<String,_>("allowed_ips")).unwrap_or(json!([])),"quota":r.get::<i64,_>("quota"),"created":r.get::<String,_>("created")})).collect::<Vec<_>>())))
}
async fn create_user(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(v): Json<Value>,
) -> Api<Value> {
    if user.role != "admin" {
        return Err(forbidden());
    }
    let _guard = app.writes.lock().await;
    let name = text(&v, "username");
    let pw = text(&v, "password");
    if !identifier(name) || name.len() < 3 || pw.len() < 14 || pw.len() > 256 {
        return Err(bad(
            "Use a lowercase username (3–32 characters) and a password of 14–256 characters",
        ));
    }
    let ips = validated_ips(&v["allowed_ips"])?;
    let quota = v["quota"].as_i64().unwrap_or(10).clamp(1, 100);
    if sqlx::query("SELECT 1 FROM users WHERE username=?")
        .bind(name)
        .fetch_optional(&app.db)
        .await?
        .is_some()
    {
        return Err(bad("Username already exists"));
    }
    let uid = id();
    let hash = password_hash(pw.into()).await?;
    host(&app, &user, "create_tenant", &uid, &uid, json!({})).await?;
    sqlx::query(
        "INSERT INTO users(id,username,password,role,allowed_ips,quota) VALUES(?,?,?,'user',?,?)",
    )
    .bind(&uid)
    .bind(name)
    .bind(hash)
    .bind(ips.to_string())
    .bind(quota)
    .execute(&app.db)
    .await?;
    audit(&app, &user.id, "user:create", &uid).await;
    Ok(Json(json!({"id":uid,"username":name})))
}
fn validated_ips(v: &Value) -> Result<Value, Error> {
    let a = if v.is_null() {
        vec![]
    } else {
        v.as_array()
            .ok_or_else(|| bad("IP filters must be an array of CIDRs"))?
            .clone()
    };
    if a.len() > 32
        || a.iter().any(|x| {
            x.as_str()
                .is_none_or(|s| s.parse::<ipnet::IpNet>().is_err())
        })
    {
        return Err(bad("Use valid IPv4/IPv6 CIDRs, at most 32"));
    }
    Ok(json!(a))
}
async fn update_user(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(uid): Path<String>,
    Json(v): Json<Value>,
) -> Api<Value> {
    if user.role != "admin" || uid == user.id {
        return Err(bad(
            "Administrator accounts cannot modify their own access here",
        ));
    }
    let _guard = app.writes.lock().await;
    let target = sqlx::query("SELECT role FROM users WHERE id=?")
        .bind(&uid)
        .fetch_optional(&app.db)
        .await?
        .ok_or_else(|| bad("User not found"))?;
    if target.get::<String, _>("role") == "admin" {
        return Err(forbidden());
    }
    let ips = validated_ips(&v["allowed_ips"])?;
    let enabled = v["enabled"].as_bool().unwrap_or(true);
    if let Some(pw) = v["password"].as_str().filter(|s| !s.is_empty()) {
        if pw.len() < 14 || pw.len() > 256 {
            return Err(bad("Password must be 14–256 characters"));
        }
    }
    sqlx::query("UPDATE users SET allowed_ips=?,enabled=?,quota=? WHERE id=?")
        .bind(ips.to_string())
        .bind(enabled)
        .bind(v["quota"].as_i64().unwrap_or(10).clamp(1, 100))
        .bind(&uid)
        .execute(&app.db)
        .await?;
    if let Some(pw) = v["password"].as_str().filter(|s| !s.is_empty()) {
        if pw.len() < 14 || pw.len() > 256 {
            return Err(bad("Password must be 14–256 characters"));
        }
        let h = password_hash(pw.into()).await?;
        sqlx::query("UPDATE users SET password=? WHERE id=?")
            .bind(h)
            .bind(&uid)
            .execute(&app.db)
            .await?;
    }
    sqlx::query("DELETE FROM sessions WHERE user_id=?")
        .bind(&uid)
        .execute(&app.db)
        .await?;
    audit(&app, &user.id, "user:update", &uid).await;
    Ok(Json(json!({"ok":true})))
}

#[derive(Clone, Serialize)]
struct Resource {
    id: String,
    owner: String,
    kind: String,
    name: String,
    data: Value,
    created: String,
}
fn resource(r: sqlx::sqlite::SqliteRow) -> Resource {
    Resource {
        id: r.get("id"),
        owner: r.get("owner"),
        kind: r.get("kind"),
        name: r.get("name"),
        data: serde_json::from_str(&r.get::<String, _>("data")).unwrap_or(json!({})),
        created: r.get("created"),
    }
}
async fn owned(app: &App, user: &Identity, rid: &str) -> Result<Resource, Error> {
    let r = sqlx::query("SELECT * FROM resources WHERE id=? AND (owner=? OR ?='admin')")
        .bind(rid)
        .bind(&user.id)
        .bind(&user.role)
        .fetch_optional(&app.db)
        .await?
        .ok_or_else(|| Error(StatusCode::NOT_FOUND, "Resource not found".into()))?;
    Ok(resource(r))
}
async fn list_resources(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(kind): Path<String>,
) -> Api<Value> {
    let rows = sqlx::query(
        "SELECT * FROM resources WHERE kind=? AND (owner=? OR ?='admin') ORDER BY created DESC",
    )
    .bind(kind)
    .bind(&user.id)
    .bind(&user.role)
    .fetch_all(&app.db)
    .await?;
    Ok(Json(json!(rows
        .into_iter()
        .map(resource)
        .collect::<Vec<_>>())))
}
async fn create_resource(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(kind): Path<String>,
    Json(mut v): Json<Value>,
) -> Api<Value> {
    let _guard = app.writes.lock().await;
    let owner = if user.role == "admin" && !text(&v, "owner").is_empty() {
        text(&v, "owner").to_owned()
    } else {
        user.id.clone()
    };
    let u = sqlx::query("SELECT quota FROM users WHERE id=? AND enabled=1")
        .bind(&owner)
        .fetch_optional(&app.db)
        .await?
        .ok_or_else(|| bad("Active owner not found"))?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM resources WHERE kind=? AND owner=?")
        .bind(&kind)
        .bind(&owner)
        .fetch_one(&app.db)
        .await?;
    if count >= u.get::<i64, _>("quota") {
        return Err(bad("Resource quota reached for this account"));
    }
    let name = text(&v, "name").trim().to_lowercase();
    let rid = id();
    let action = match kind.as_str() {
        "domains" => {
            if !domain(&name) {
                return Err(bad("Enter a valid domain name"));
            }
            if user.role != "admin" {
                let roots: Vec<String> = sqlx::query_scalar(
                    "SELECT name FROM resources WHERE owner=? AND kind='domains'",
                )
                .bind(&owner)
                .fetch_all(&app.db)
                .await?;
                if !roots.iter().any(|root| within_domain(&name, root)) {
                    return Err(bad("An administrator must assign your domain first"));
                }
            }
            if sqlx::query("SELECT 1 FROM resources WHERE kind='domains' AND name=?")
                .bind(&name)
                .fetch_optional(&app.db)
                .await?
                .is_some()
            {
                return Err(bad("Domain already assigned"));
            }
            if !text(&v, "app_id").is_empty() {
                let a = owned(&app, &user, text(&v, "app_id")).await?;
                if a.kind != "apps" || a.owner != owner {
                    return Err(bad("Application must belong to the domain owner"));
                }
            }
            "create_domain"
        }
        "apps" => {
            if !identifier(&name) || !cgpanel::runtime(text(&v, "runtime")) {
                return Err(bad("Invalid application name or runtime"));
            }
            let command = text(&v, "command");
            if command.len() > 2000 {
                return Err(bad("Start command is too long"));
            }
            let mode = text(&v, "mode");
            if !["web", "worker"].contains(&mode) {
                return Err(bad("Application mode must be web or worker"));
            }
            "create_app"
        }
        "databases" => {
            if !identifier(&name) || !["mysql", "postgresql"].contains(&text(&v, "engine")) {
                return Err(bad("Use a lowercase database name and MySQL or PostgreSQL"));
            }
            let ips = v["allowed_ips"].as_array().cloned().unwrap_or_default();
            if ips.len() > 16
                || ips
                    .iter()
                    .any(|x| !cgpanel::valid_ip(x.as_str().unwrap_or("")))
            {
                return Err(bad(
                    "Database access requires exact IP addresses, at most 16",
                ));
            }
            v["allowed_ips"] = json!(ips);
            "create_database"
        }
        "dns" => {
            let z = owned(&app, &user, text(&v, "domain_id")).await?;
            if z.kind != "domains" || z.owner != owner {
                return Err(forbidden());
            }
            let label = if name.is_empty() { "@" } else { &name };
            if !cgpanel::dns_record(text(&v, "type"), label, text(&v, "value"), &z.name) {
                return Err(bad("Invalid DNS record"));
            }
            v["zone"] = json!(z.name);
            v["ttl"] = json!(v["ttl"].as_u64().unwrap_or(300).clamp(60, 86400));
            "create_dns"
        }
        "schedules" => {
            let a = owned(&app, &user, text(&v, "app_id")).await?;
            if a.kind != "apps" || a.owner != owner {
                return Err(forbidden());
            }
            if !identifier(&name)
                || text(&v, "command").is_empty()
                || text(&v, "command").len() > 2000
            {
                return Err(bad("Enter a name and command"));
            }
            if !["hourly", "daily", "weekly"].contains(&text(&v, "schedule")) {
                return Err(bad("Choose hourly, daily or weekly"));
            }
            "create_schedule"
        }
        "backups" => {
            let a = owned(&app, &user, text(&v, "app_id")).await?;
            if a.kind != "apps" || a.owner != owner {
                return Err(forbidden());
            }
            "create_backup"
        }
        "blocks" => {
            if user.role != "admin" {
                return Err(forbidden());
            }
            if !cgpanel::valid_ip(&name) {
                return Err(bad("Enter one exact IPv4 or IPv6 address"));
            }
            "create_block"
        }
        _ => return Err(bad("Unknown resource type")),
    };
    v["name"] = json!(if kind == "dns" && name.is_empty() {
        "@"
    } else {
        &name
    });
    let result = host(&app, &user, action, &owner, &rid, v.clone()).await?;
    let mut saved = v;
    if let Some(m) = saved.as_object_mut() {
        m.remove("env");
        m.remove("password");
    }
    if let Some(obj) = result.as_object() {
        for (k, val) in obj {
            if k != "password" {
                saved[k] = val.clone();
            }
        }
    }
    sqlx::query("INSERT INTO resources(id,owner,kind,name,data) VALUES(?,?,?,?,?)")
        .bind(&rid)
        .bind(&owner)
        .bind(&kind)
        .bind(&name)
        .bind(saved.to_string())
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"id":rid,"result":result})))
}
async fn remove_resource(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    let _guard = app.writes.lock().await;
    let r = owned(&app, &user, &rid).await?;
    if r.kind == "blocks" && user.role != "admin" {
        return Err(forbidden());
    }
    if r.kind == "apps" || r.kind == "domains" {
        let rows = sqlx::query("SELECT data FROM resources WHERE id<>?")
            .bind(&rid)
            .fetch_all(&app.db)
            .await?;
        if rows.iter().any(|x| {
            serde_json::from_str::<Value>(&x.get::<String, _>("data"))
                .is_ok_and(|v| text(&v, "app_id") == rid || text(&v, "domain_id") == rid)
        }) {
            return Err(bad("Remove linked resources first"));
        }
    }
    host(
        &app,
        &user,
        &format!(
            "delete_{}",
            match r.kind.as_str() {
                "domains" => "domain",
                "apps" => "app",
                "databases" => "database",
                "dns" => "dns",
                "schedules" => "schedule",
                "backups" => "backup",
                "blocks" => "block",
                _ => return Err(bad("Unknown resource")),
            }
        ),
        &r.owner,
        &rid,
        r.data,
    )
    .await?;
    sqlx::query("DELETE FROM resources WHERE id=?")
        .bind(&rid)
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"ok":true})))
}
async fn app_action(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path((rid, action)): Path<(String, String)>,
    Json(v): Json<Value>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    let mapped = match (r.kind.as_str(), action.as_str()) {
        ("apps", "start") => "start_app",
        ("apps", "stop") => "stop_app",
        ("apps", "restart") => "restart_app",
        ("apps", "logs") => "logs_app",
        ("apps", "inspect") => "inspect_app",
        ("apps", "terminal") => "terminal",
        ("apps", "files") => "files",
        ("apps", "read") => "read_file",
        ("apps", "write") => "write_file",
        ("domains", "tls") => "domain_tls",
        ("databases", "access") => "database_access",
        ("backups", "restore") => "restore_backup",
        _ => return Err(bad("Unsupported operation")),
    };
    if mapped == "terminal" && (text(&v, "command").is_empty() || text(&v, "command").len() > 4000)
    {
        return Err(bad("Command must contain 1–4000 characters"));
    }
    if ["read_file", "write_file"].contains(&mapped) && !cgpanel::relative_path(text(&v, "path")) {
        return Err(bad("Invalid relative file path"));
    }
    let _guard = app.writes.lock().await;
    let result = host(&app, &user, mapped, &r.owner, &rid, v.clone()).await?;
    if mapped == "database_access" {
        let mut d = r.data;
        d["allowed_ips"] = v["allowed_ips"].clone();
        sqlx::query("UPDATE resources SET data=? WHERE id=?")
            .bind(d.to_string())
            .bind(rid)
            .execute(&app.db)
            .await?;
    }
    Ok(Json(result))
}
async fn change_password(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(v): Json<Value>,
) -> Api<Value> {
    let old = text(&v, "current").to_owned();
    let new = text(&v, "password");
    if new.len() < 14 || new.len() > 256 {
        return Err(bad("Use 14–256 characters"));
    }
    let h: String = sqlx::query_scalar("SELECT password FROM users WHERE id=?")
        .bind(&user.id)
        .fetch_one(&app.db)
        .await?;
    let valid = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&h).is_ok_and(|h| {
            Argon2::default()
                .verify_password(old.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false);
    if !valid {
        return Err(forbidden());
    }
    let h = password_hash(new.into()).await?;
    sqlx::query("UPDATE users SET password=? WHERE id=?")
        .bind(h)
        .bind(&user.id)
        .execute(&app.db)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id=?")
        .bind(&user.id)
        .execute(&app.db)
        .await?;
    audit(&app, &user.id, "password:changed", &user.id).await;
    Ok(Json(json!({"ok":true})))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let path = std::env::var("CGPANEL_DB").unwrap_or("cgpanel.db".into());
    let db = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true)
                .foreign_keys(true)
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                .busy_timeout(Duration::from_secs(10)),
        )
        .await?;
    sqlx::raw_sql(include_str!("schema.sql"))
        .execute(&db)
        .await?;
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|x| x == "bootstrap") {
        let username = std::env::var("CGPANEL_ADMIN").unwrap_or("admin".into());
        let pw = std::env::var("CGPANEL_ADMIN_PASSWORD")?;
        anyhow::ensure!(
            identifier(&username) && pw.len() >= 14 && pw.len() <= 256,
            "Invalid bootstrap credentials"
        );
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE role='admin'")
            .fetch_one(&db)
            .await?;
        anyhow::ensure!(
            n == 0,
            "Administrator already exists; use the password change API"
        );
        let h = password_hash(pw).await.map_err(|e| anyhow::anyhow!(e.1))?;
        sqlx::query("INSERT INTO users(id,username,password,role,quota) VALUES(?,?,?,'admin',100)")
            .bind(id())
            .bind(username)
            .bind(h)
            .execute(&db)
            .await?;
        println!("Administrator created");
        return Ok(());
    }
    let app = App {
        db,
        agent: std::env::var("CGPANEL_AGENT_SOCKET").unwrap_or("/run/cgpanel/agent.sock".into()),
        secure: std::env::var("CGPANEL_INSECURE_LOCAL").unwrap_or_default() != "1",
        limits: Default::default(),
        writes: Default::default(),
    };
    let api = Router::new()
        .route("/me", get(me))
        .route("/logout", post(logout))
        .route("/overview", get(overview))
        .route("/users", get(users).post(create_user))
        .route("/users/{id}", post(update_user))
        .route("/password", post(change_password))
        .route(
            "/resources/{kind}",
            get(list_resources).post(create_resource),
        )
        .route("/resource/{id}", delete(remove_resource))
        .route("/resource/{id}/{action}", post(app_action))
        .route_layer(middleware::from_fn_with_state(app.clone(), authenticate));
    let router = Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../web/index.html")) }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/app.js"),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../web/style.css"),
                )
            }),
        )
        .route(
            "/icons.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/icons.js"),
                )
            }),
        )
        .route("/assets/{*path}", get(asset))
        .route("/healthz", get(|| async { Json(json!({"status":"ok"})) }))
        .route("/api/login", post(login))
        .nest("/api", api)
        .layer(DefaultBodyLimit::max(512 * 1024))
        .layer(middleware::from_fn(security_headers))
        .with_state(app);
    let addr = std::env::var("CGPANEL_BIND").unwrap_or("127.0.0.1:2082".into());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr,"CGPanel listening");
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ip_filters() {
        assert!(ip_allowed("[]", None));
        assert!(ip_allowed(
            "[\"10.0.0.0/24\"]",
            Some("10.0.0.4".parse().unwrap())
        ));
        assert!(!ip_allowed(
            "[\"10.0.0.0/24\"]",
            Some("11.0.0.1".parse().unwrap())
        ));
        assert!(!ip_allowed("malformed", Some("127.0.0.1".parse().unwrap())));
    }
}
