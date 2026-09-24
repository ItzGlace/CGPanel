use super::*;
use axum::extract::Form;
use std::sync::LazyLock;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v5/protection/{id}", get(settings).post(configure))
        .route("/v5/protection/{id}/traffic", get(traffic))
}
async fn traffic(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "domains" {
        return Err(bad("Choose a domain"));
    }
    Ok(Json(
        host(&app, &user, "protection_stats", &r.owner, &rid, json!({})).await?,
    ))
}
pub fn public_routes() -> Router<App> {
    Router::new()
        .route(
            "/guard/{id}/challenge",
            get(challenge)
                .post(solve)
                .layer(DefaultBodyLimit::max(8192)),
        )
        .route("/guard/{id}/verify", get(verify))
        .route("/guard/{id}/sitemap", get(sitemap))
}
fn defaults() -> Value {
    json!({"requests_per_second":20,"connections":30,"waf":"off","crawlers":"allow","captcha":"off","site_key":"","secret":"","sitemap":false,"sitemap_auto":false,"sitemap_paths":["/"]})
}
async fn config(app: &App, rid: &str) -> Result<Value, Error> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT config FROM website_protection WHERE domain_id=?")
            .bind(rid)
            .fetch_optional(&app.db)
            .await?;
    Ok(value
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(defaults))
}
async fn settings(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "domains" {
        return Err(bad("Choose a domain"));
    }
    let mut value = config(&app, &rid).await?;
    value["secret_set"] = json!(!text(&value, "secret").is_empty());
    value["secret"] = json!("");
    Ok(Json(value))
}
async fn configure(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Json(mut value): Json<Value>,
) -> Api<Value> {
    let r = owned(&app, &user, &rid).await?;
    if r.kind != "domains" {
        return Err(bad("Choose a domain"));
    }
    let _lock = app.writes.lock().await;
    let old = config(&app, &rid).await?;
    if text(&value, "secret").is_empty() {
        value["secret"] = old["secret"].clone();
    }
    if !["off", "local"].contains(&text(&value, "captcha"))
        && (text(&value, "secret").is_empty() || text(&value, "site_key").is_empty())
    {
        return Err(bad("Enter the provider site key and secret"));
    }
    for key in ["site_key", "secret"] {
        if text(&value, key).len() > 512
            || !text(&value, key)
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        {
            return Err(bad("Invalid provider key"));
        }
    }
    // The broker independently validates all values before generating Nginx configuration.
    let mut public = value.clone();
    public
        .as_object_mut()
        .ok_or_else(|| bad("Invalid settings"))?
        .remove("secret");
    sqlx::query("INSERT INTO website_protection(domain_id,config) VALUES(?,?) ON CONFLICT(domain_id) DO UPDATE SET config=excluded.config").bind(&rid).bind(value.to_string()).execute(&app.db).await?;
    if let Err(e) = host(&app, &user, "protection_configure", &r.owner, &rid, public).await {
        sqlx::query("UPDATE website_protection SET config=? WHERE domain_id=?")
            .bind(old.to_string())
            .bind(&rid)
            .execute(&app.db)
            .await?;
        return Err(e);
    }
    sqlx::query("DELETE FROM website_passes WHERE domain_id=?")
        .bind(&rid)
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"saved":true})))
}
async fn site(app: &App, rid: &str, headers: &HeaderMap) -> Result<String, Error> {
    let name:Option<String>=sqlx::query_scalar("SELECT r.name FROM resources r JOIN users u ON r.owner=u.id WHERE r.id=? AND r.kind='domains' AND u.enabled=1").bind(rid).fetch_optional(&app.db).await?;
    let name = name.ok_or_else(forbidden)?;
    let host = headers
        .get(header::HOST)
        .and_then(|s| s.to_str().ok())
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    if host != name {
        return Err(forbidden());
    }
    Ok(name)
}
type ChallengeLimits = Mutex<HashMap<String, (Instant, u32)>>;
static LIMITS: LazyLock<ChallengeLimits> = LazyLock::new(|| Mutex::new(HashMap::new()));
async fn limit(ip: IpAddr) -> Result<(), Error> {
    let mut map = LIMITS.lock().await;
    map.retain(|_, v| v.0.elapsed() < Duration::from_secs(60));
    if map.len() > 10000 {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Try again later".into(),
        ));
    }
    let v = map.entry(ip.to_string()).or_insert((Instant::now(), 0));
    v.1 += 1;
    if v.1 > 20 {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many challenges; wait a minute".into(),
        ));
    }
    Ok(())
}
async fn verify(
    State(app): State<App>,
    Path(rid): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<StatusCode, Error> {
    site(&app, &rid, &headers).await?;
    let stored: Option<String> =
        sqlx::query_scalar("SELECT config FROM website_protection WHERE domain_id=?")
            .bind(&rid)
            .fetch_optional(&app.db)
            .await?;
    let stored = stored.ok_or_else(forbidden)?;
    let settings: Value = serde_json::from_str(&stored).map_err(|_| forbidden())?;
    if text(&settings, "captcha") == "off" {
        return Ok(StatusCode::NO_CONTENT);
    }
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .split(';')
        .find_map(|s| s.trim().strip_prefix("cgp_site_pass="))
        .unwrap_or("");
    if cookie.len() != 64 {
        return Ok(StatusCode::UNAUTHORIZED);
    }
    let found:Option<i64>=sqlx::query_scalar("SELECT 1 FROM website_passes WHERE token_hash=? AND domain_id=? AND ip=? AND expires>unixepoch()")
        .bind(digest(cookie)).bind(&rid).bind(client_ip(peer.ip(),&headers).to_string()).fetch_optional(&app.db).await?;
    Ok(if found.is_some() {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::UNAUTHORIZED
    })
}
async fn challenge(
    State(app): State<App>,
    Path(rid): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Response, Error> {
    let domain = site(&app, &rid, &headers).await?;
    let ip = client_ip(peer.ip(), &headers);
    limit(ip).await?;
    let value = config(&app, &rid).await?;
    let provider = text(&value, "captcha");
    if provider == "off" {
        return Ok(axum::response::Redirect::to("/").into_response());
    }
    sqlx::query("DELETE FROM website_challenges WHERE expires<unixepoch()")
        .execute(&app.db)
        .await?;
    sqlx::query("DELETE FROM website_passes WHERE expires<unixepoch()")
        .execute(&app.db)
        .await?;
    let nonce = id();
    let a = (rand::random::<u8>() % 20) + 1;
    let b = (rand::random::<u8>() % 20) + 1;
    sqlx::query("INSERT INTO website_challenges(nonce,domain_id,ip,answer,expires) VALUES(?,?,?,?,unixepoch()+300)")
        .bind(&nonce).bind(&rid).bind(ip.to_string()).bind(digest(&format!("{nonce}:{}",a+b))).execute(&app.db).await?;
    let key = text(&value, "site_key");
    let (widget,sources)=match provider {
        "local"=>(format!("<label class=\"block text-lg\">What is {a} + {b}?<input class=\"mt-3 block w-full rounded-xl border border-slate-300 bg-white p-3\" name=\"answer\" inputmode=\"numeric\" autocomplete=\"off\" required autofocus></label>"),""),
        "turnstile"=>(format!("<script src=\"https://challenges.cloudflare.com/turnstile/v0/api.js\" async defer></script><div class=\"cf-turnstile\" data-sitekey=\"{key}\"></div>"),"https://challenges.cloudflare.com"),
        "hcaptcha"=>(format!("<script src=\"https://js.hcaptcha.com/1/api.js\" async defer></script><div class=\"h-captcha\" data-sitekey=\"{key}\"></div>"),"https://hcaptcha.com https://*.hcaptcha.com"),
        "recaptcha"=>(format!("<script src=\"https://www.google.com/recaptcha/api.js\" async defer></script><div class=\"g-recaptcha\" data-sitekey=\"{key}\"></div>"),"https://www.google.com https://www.gstatic.com"),
        _=>return Err(bad("Invalid provider")),
    };
    let html=format!("<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Verify your visit</title><link rel=\"stylesheet\" href=\"/__cgpanel/guard-style.css\"><body class=\"grid min-h-screen place-items-center bg-forest p-6 font-sans\"><main class=\"w-full max-w-md rounded-2xl bg-white p-8 shadow-xl\"><p class=\"text-xs text-emerald-700\">{domain}</p><h1 class=\"my-4 text-2xl font-semibold\">Verify your visit</h1><p class=\"mb-6 text-sm text-slate-500\">Complete this check to continue. The verification expires in five minutes.</p><form method=\"post\" action=\"/__cgpanel/challenge\"><input type=\"hidden\" name=\"nonce\" value=\"{nonce}\">{widget}<button class=\"mt-6 rounded-xl bg-forest px-5 py-3 text-white\">Continue</button></form><p class=\"mt-5 text-xs text-slate-400\">Protected by CGPanel</p></main></body></html>");
    let policy=format!("default-src 'self'; script-src 'self' {sources}; style-src 'self' 'unsafe-inline' {sources}; frame-src {sources}; connect-src 'self' {sources}; img-src 'self' data: {sources}; frame-ancestors 'none'; base-uri 'none'; form-action 'self'");
    Ok(([(header::CONTENT_SECURITY_POLICY, policy)], Html(html)).into_response())
}
async fn solve(
    State(app): State<App>,
    Path(rid): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Result<Response, Error> {
    let domain = site(&app, &rid, &headers).await?;
    let ip = client_ip(peer.ip(), &headers);
    limit(ip).await?;
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|s| s.to_str().ok())
        .and_then(|s| reqwest::Url::parse(s).ok());
    if !origin
        .is_some_and(|o| o.host_str() == Some(&domain) && ["http", "https"].contains(&o.scheme()))
    {
        return Err(forbidden());
    }
    let nonce = form.get("nonce").map(String::as_str).unwrap_or("");
    let answer:Option<String>=sqlx::query_scalar("DELETE FROM website_challenges WHERE nonce=? AND domain_id=? AND ip=? AND expires>unixepoch() RETURNING answer").bind(nonce).bind(&rid).bind(ip.to_string()).fetch_optional(&app.db).await?;
    let answer = answer
        .ok_or_else(|| bad("Challenge expired or already used; reload the verification page"))?;
    let value = config(&app, &rid).await?;
    let provider = text(&value, "captcha");
    let success = if provider == "local" {
        digest(&format!(
            "{nonce}:{}",
            form.get("answer").map(|s| s.trim()).unwrap_or("")
        )) == answer
    } else {
        let (endpoint, field) = match provider {
            "turnstile" => (
                "https://challenges.cloudflare.com/turnstile/v0/siteverify",
                "cf-turnstile-response",
            ),
            "hcaptcha" => ("https://api.hcaptcha.com/siteverify", "h-captcha-response"),
            "recaptcha" => (
                "https://www.google.com/recaptcha/api/siteverify",
                "g-recaptcha-response",
            ),
            _ => return Err(forbidden()),
        };
        let token = form.get(field).map(String::as_str).unwrap_or("");
        if token.is_empty() || token.len() > 4096 {
            return Err(bad("Complete the provider challenge"));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| bad("Verification unavailable"))?;
        let response = client
            .post(endpoint)
            .form(&[
                ("secret", text(&value, "secret")),
                ("response", token),
                ("remoteip", &ip.to_string()),
            ])
            .send()
            .await
            .map_err(|_| bad("Provider unavailable; please retry"))?;
        let body = cgpanel::outbound::bounded_body(response, 32768)
            .await
            .map_err(|_| bad("Invalid provider response"))?;
        let reply: Value =
            serde_json::from_slice(&body).map_err(|_| bad("Invalid provider response"))?;
        reply["success"] == true && reply["hostname"] == domain
    };
    if !success {
        return Err(bad("Verification failed; reload the page and try again"));
    }
    let token = format!("{}{}", id(), id());
    sqlx::query("INSERT INTO website_passes(token_hash,domain_id,ip,expires) VALUES(?,?,?,unixepoch()+3600)").bind(digest(&token)).bind(&rid).bind(ip.to_string()).execute(&app.db).await?;
    let secure = if headers
        .get("x-forwarded-proto")
        .is_some_and(|h| h == "https")
    {
        "; Secure"
    } else {
        ""
    };
    Ok((
        [(
            header::SET_COOKIE,
            format!("cgp_site_pass={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=3600{secure}"),
        )],
        axum::response::Redirect::to("/"),
    )
        .into_response())
}

async fn sitemap(
    State(app): State<App>,
    Path(rid): Path<String>,
    headers: HeaderMap,
) -> Result<Response, Error> {
    let domain = site(&app, &rid, &headers).await?;
    let config = config(&app, &rid).await?;
    if config["sitemap"] != true || config["sitemap_auto"] != true {
        return Err(forbidden());
    }
    let observed:Vec<String>=sqlx::query_scalar("SELECT DISTINCT path FROM analytics_events WHERE domain_id=? AND kind='view' ORDER BY path LIMIT 1000").bind(&rid).fetch_all(&app.db).await?;
    let mut paths = std::collections::BTreeSet::new();
    for path in config["sitemap_paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .chain(observed)
    {
        if path.starts_with('/')
            && !path.starts_with("//")
            && path.len() < 300
            && path
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"/._-".contains(&c))
        {
            paths.insert(path);
        }
    }
    let urls = paths
        .into_iter()
        .take(1000)
        .map(|p| format!("<url><loc>https://{domain}{p}</loc></url>"))
        .collect::<String>();
    Ok(([(header::CONTENT_TYPE,"application/xml; charset=utf-8"),(header::CACHE_CONTROL,"public, max-age=300")],format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">{urls}</urlset>")).into_response())
}
