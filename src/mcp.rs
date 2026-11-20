//! Stateless MCP Streamable HTTP. Credentials are independent of administrator API keys.
use super::*;
use axum::extract::Form;
use base64::Engine;
pub fn routes() -> Router<App> {
    Router::new()
        .route("/mcp/tokens", get(tokens).post(create))
        .route("/mcp/tokens/{id}", delete(revoke))
        .route("/mcp/authorize", post(authorize))
}
pub fn public_routes() -> Router<App> {
    Router::new()
        .route(
            "/mcp",
            post(rpc).get(|| async { StatusCode::METHOD_NOT_ALLOWED }),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            get(resource_metadata),
        )
        .route("/.well-known/oauth-authorization-server", get(metadata))
        .route("/oauth/register", post(register))
        .route("/oauth/authorize", get(authorize_redirect))
        .route("/oauth/token", post(exchange))
}
fn base() -> Result<String, Error> {
    let base = std::env::var("CGPANEL_PUBLIC_URL")
        .map_err(|_| bad("Administrator must set CGPANEL_PUBLIC_URL to the panel HTTPS origin"))?;
    let url = reqwest::Url::parse(&base).map_err(|_| bad("Invalid public URL"))?;
    if url.scheme() != "https"
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(bad("CGPANEL_PUBLIC_URL must be an HTTPS origin"));
    }
    Ok(base.trim_end_matches('/').into())
}
async fn resource_metadata() -> Api<Value> {
    let b = base()?;
    Ok(Json(
        json!({"resource":format!("{b}/mcp"),"authorization_servers":[b],"scopes_supported":["read","manage"]}),
    ))
}
async fn metadata() -> Api<Value> {
    let b = base()?;
    Ok(Json(
        json!({"issuer":b,"authorization_endpoint":format!("{b}/oauth/authorize"),"token_endpoint":format!("{b}/oauth/token"),"registration_endpoint":format!("{b}/oauth/register"),"response_types_supported":["code"],"grant_types_supported":["authorization_code"],"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none"],"scopes_supported":["read","manage"]}),
    ))
}
fn session(user: &Identity) -> Result<(), Error> {
    if user.api_token.is_some() {
        Err(forbidden())
    } else {
        Ok(())
    }
}
async fn issue(
    app: &App,
    user: &Identity,
    name: &str,
    scope: &str,
    days: i64,
    ips: Value,
) -> Api<Value> {
    if name.is_empty()
        || name.len() > 64
        || !["read", "manage"].contains(&scope)
        || !(1..=90).contains(&days)
    {
        return Err(bad("Use a name, read/manage scope and 1–90 days"));
    }
    let _guard = app.writes.lock().await;
    sqlx::query("DELETE FROM mcp_tokens WHERE expires<unixepoch()-604800")
        .execute(&app.db)
        .await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mcp_tokens WHERE user_id=? AND revoked=0 AND expires>unixepoch()",
    )
    .bind(&user.id)
    .fetch_one(&app.db)
    .await?;
    if count >= 20 {
        return Err(bad(
            "Revoke an existing credential first (20 active credentials maximum)",
        ));
    }
    let token = format!("cgm_{}{}", id(), id());
    let rid = id();
    let expires = chrono::Utc::now().timestamp() + days * 86400;
    sqlx::query("INSERT INTO mcp_tokens(id,user_id,name,hash,prefix,scope,ips,expires) VALUES(?,?,?,?,?,?,?,?)").bind(&rid).bind(&user.id).bind(name).bind(digest(&token)).bind(&token[..12]).bind(scope).bind(ips.to_string()).bind(expires).execute(&app.db).await?;
    audit(app, &user.id, "mcp:credential_created", &rid).await;
    Ok(Json(
        json!({"id":rid,"token":token,"scope":scope,"expires":expires}),
    ))
}
async fn tokens(State(app): State<App>, Extension(user): Extension<Identity>) -> Api<Value> {
    session(&user)?;
    let rows=sqlx::query("SELECT id,name,prefix,scope,expires,revoked FROM mcp_tokens WHERE user_id=? ORDER BY expires DESC LIMIT 100").bind(&user.id).fetch_all(&app.db).await?;
    Ok(Json(json!(rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"name":r.get::<String,_>("name"),"prefix":r.get::<String,_>("prefix"),"scope":r.get::<String,_>("scope"),"expires":r.get::<i64,_>("expires"),"revoked":r.get::<bool,_>("revoked")})).collect::<Vec<_>>())))
}
async fn create(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(v): Json<Value>,
) -> Api<Value> {
    session(&user)?;
    issue(
        &app,
        &user,
        text(&v, "name"),
        v["scope"].as_str().unwrap_or("read"),
        v["expires_days"].as_i64().unwrap_or(30),
        validated_ips(&v["allowed_ips"])?,
    )
    .await
}
async fn revoke(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    session(&user)?;
    sqlx::query("UPDATE mcp_tokens SET revoked=1 WHERE id=? AND user_id=?")
        .bind(rid)
        .bind(&user.id)
        .execute(&app.db)
        .await?;
    Ok(Json(json!({"revoked":true})))
}
async fn authenticate(
    app: &App,
    headers: &HeaderMap,
    peer: IpAddr,
) -> Result<(Identity, String), Error> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !token.starts_with("cgm_") || token.len() != 68 {
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "MCP bearer credential required".into(),
        ));
    }
    let r=sqlx::query("SELECT t.id token_id,t.scope,t.ips,u.id,u.username,u.role,u.allowed_ips FROM mcp_tokens t JOIN users u ON u.id=t.user_id WHERE t.hash=? AND t.revoked=0 AND t.expires>unixepoch() AND u.enabled=1").bind(digest(token)).fetch_optional(&app.db).await?.ok_or_else(||Error(StatusCode::UNAUTHORIZED,"MCP credential expired or revoked".into()))?;
    if !ip_allowed(&r.get::<String, _>("ips"), Some(peer))
        || !ip_allowed(&r.get::<String, _>("allowed_ips"), Some(peer))
    {
        return Err(forbidden());
    }
    Ok((
        Identity {
            id: r.get("id"),
            username: r.get("username"),
            role: r.get("role"),
            csrf: String::new(),
            api_token: Some(r.get("token_id")),
        },
        r.get("scope"),
    ))
}
async fn rpc(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(v): Json<Value>,
) -> Result<Response, Error> {
    if let Some(origin) = headers.get(header::ORIGIN) {
        if origin.to_str().ok() != Some(base()?.as_str()) {
            return Err(forbidden());
        }
    }
    // This transport never accepts browser cookies, preventing ambient session CSRF.
    let (user, scope) = match authenticate(&app, &headers, client_ip(peer.ip(), &headers)).await {
        Ok(v) => v,
        Err(e) => {
            let mut response = e.into_response();
            if let Ok(b) = base() {
                response.headers_mut().insert(
                    header::WWW_AUTHENTICATE,
                    format!(
                        "Bearer resource_metadata=\"{b}/.well-known/oauth-protected-resource\""
                    )
                    .parse()
                    .unwrap(),
                );
            }
            return Ok(response);
        }
    };
    if v["jsonrpc"] != "2.0" || !v.is_object() {
        return Err(bad("Expected a JSON-RPC 2.0 object"));
    }
    let request_id = v.get("id").cloned();
    let method = text(&v, "method");
    if request_id.is_none() {
        return if method == "notifications/initialized" {
            Ok(StatusCode::ACCEPTED.into_response())
        } else {
            Err(bad("Request id required"))
        };
    }
    let result:Result<Value,Error>=match method {
        "initialize"=>Ok(json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"CGPanel","version":env!("CARGO_PKG_VERSION")},"instructions":"Only access the signed-in account's permitted resources. Ask the user before restarting an application."})),
        "ping"=>Ok(json!({})),
        "tools/list"=>{let mut tools=vec![json!({"name":"list_resources","description":"List permitted applications, domains or databases; credentials are omitted.","inputSchema":{"type":"object","properties":{"kind":{"type":"string","enum":["apps","domains","databases"]}},"required":["kind"],"additionalProperties":false}}),json!({"name":"website_analytics","description":"Read a permitted website's aggregate analytics.","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false}})];if scope=="manage" {tools.push(json!({"name":"restart_application","description":"Restart an owned application. Ask the user for confirmation before calling.","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false}}))}Ok(json!({"tools":tools}))},
        "tools/call"=>call(&app,&user,&scope,&v["params"]).await,
        _=>return Ok(Json(json!({"jsonrpc":"2.0","id":request_id,"error":{"code":-32601,"message":"Method not found"}})).into_response())
    };
    Ok(Json(match result {Ok(result)=>json!({"jsonrpc":"2.0","id":request_id,"result":result}),Err(e)=>json!({"jsonrpc":"2.0","id":request_id,"result":{"isError":true,"content":[{"type":"text","text":e.1}]}})}).into_response())
}
async fn call(app: &App, user: &Identity, scope: &str, p: &Value) -> Result<Value, Error> {
    let args = &p["arguments"];
    let result = match text(p, "name") {
        "list_resources" => {
            let kind = text(args, "kind");
            if !["apps", "domains", "databases"].contains(&kind) {
                return Err(bad("Invalid resource kind"));
            }
            let Json(v) = list_resources(
                State(app.clone()),
                Extension(user.clone()),
                Path(kind.to_string()),
            )
            .await?;
            {
                let mut value = json!(v);
                if let Some(items) = value.as_array_mut() {
                    for item in items {
                        if let Some(data) = item["data"].as_object_mut() {
                            for key in [
                                "env",
                                "password",
                                "secret",
                                "token",
                                "ide_password",
                                "transfer_password",
                            ] {
                                data.remove(key);
                            }
                        }
                    }
                }
                value
            }
        }
        "website_analytics" => {
            let Json(v) = monitoring::analytics(
                State(app.clone()),
                Extension(user.clone()),
                Path(text(args, "id").into()),
                axum::extract::Query(HashMap::new()),
            )
            .await?;
            v
        }
        "restart_application" if scope == "manage" => {
            let r = owned(app, user, text(args, "id")).await?;
            if r.kind != "apps" {
                return Err(bad("Choose an application"));
            }
            host(app, user, "restart_app", &r.owner, &r.id, json!({})).await?
        }
        _ => return Err(forbidden()),
    };
    Ok(json!({"content":[{"type":"text","text":result.to_string()}],"isError":false}))
}
fn redirect_valid(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|u| {
        u.fragment().is_none()
            && u.username().is_empty()
            && u.password().is_none()
            && (u.scheme() == "https"
                || (u.scheme() == "http"
                    && matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))))
    })
}
async fn register(State(app): State<App>, Json(v): Json<Value>) -> Api<Value> {
    let urls = v["redirect_uris"]
        .as_array()
        .ok_or_else(|| bad("redirect_uris required"))?;
    if urls.is_empty()
        || urls.len() > 5
        || urls
            .iter()
            .any(|v| !redirect_valid(v.as_str().unwrap_or("")))
    {
        return Err(bad("Use HTTPS or loopback redirect URIs without fragments"));
    }
    let _guard = app.writes.lock().await;
    sqlx::query("DELETE FROM oauth_clients WHERE created<unixepoch()-86400")
        .execute(&app.db)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM oauth_clients")
        .fetch_one(&app.db)
        .await?;
    if count >= 1000 {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Client registry full".into(),
        ));
    }
    let client = id();
    sqlx::query("INSERT INTO oauth_clients VALUES(?,?,unixepoch())")
        .bind(&client)
        .bind(json!(urls).to_string())
        .execute(&app.db)
        .await?;
    Ok(Json(
        json!({"client_id":client,"redirect_uris":urls,"token_endpoint_auth_method":"none","grant_types":["authorization_code"],"response_types":["code"]}),
    ))
}
fn oauth_scope(value: &str) -> Result<&'static str, Error> {
    let scopes: Vec<_> = value.split_ascii_whitespace().collect();
    if scopes.iter().any(|s| !["read", "manage"].contains(s)) {
        return Err(bad("Unknown OAuth scope"));
    }
    Ok(if scopes.contains(&"manage") {
        "manage"
    } else {
        "read"
    })
}
async fn validate_auth(app: &App, v: &Value) -> Result<(), Error> {
    let redirects: Option<String> =
        sqlx::query_scalar("SELECT redirects FROM oauth_clients WHERE id=?")
            .bind(text(v, "client_id"))
            .fetch_optional(&app.db)
            .await?;
    let redirects: Vec<String> = redirects
        .and_then(|v| serde_json::from_str(&v).ok())
        .ok_or_else(|| bad("Unknown OAuth client"))?;
    if !redirects.iter().any(|u| u == text(v, "redirect_uri"))
        || text(v, "code_challenge_method") != "S256"
        || text(v, "code_challenge").len() != 43
        || oauth_scope(text(v, "scope")).is_err()
        || text(v, "response_type") != "code"
    {
        return Err(bad(
            "Invalid authorization request; S256 PKCE and read/manage scope required",
        ));
    }
    if v.get("resource").is_some() && text(v, "resource") != format!("{}/mcp", base()?) {
        return Err(bad("Invalid resource audience"));
    }
    Ok(())
}
async fn authorize_redirect(
    State(app): State<App>,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Result<axum::response::Redirect, Error> {
    let v = json!(q);
    validate_auth(&app, &v).await?;
    let query = reqwest::Url::parse_with_params("https://unused.invalid", q.iter())
        .map_err(|_| bad("Invalid request"))?
        .query()
        .unwrap_or("")
        .to_string();
    Ok(axum::response::Redirect::to(&format!("/#mcp?{query}")))
}
async fn authorize(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Json(v): Json<Value>,
) -> Api<Value> {
    session(&user)?;
    validate_auth(&app, &v).await?;
    let code = format!("{}{}", id(), id());
    sqlx::query("DELETE FROM oauth_codes WHERE expires<unixepoch()")
        .execute(&app.db)
        .await?;
    sqlx::query("INSERT INTO oauth_codes VALUES(?,?,?,?,?,?,unixepoch()+120)")
        .bind(digest(&code))
        .bind(&user.id)
        .bind(text(&v, "client_id"))
        .bind(text(&v, "redirect_uri"))
        .bind(text(&v, "code_challenge"))
        .bind(oauth_scope(text(&v, "scope"))?)
        .execute(&app.db)
        .await?;
    let mut url = reqwest::Url::parse(text(&v, "redirect_uri")).map_err(|_| bad("Invalid URI"))?;
    url.query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", text(&v, "state"));
    audit(&app, &user.id, "mcp:oauth_consent", text(&v, "client_id")).await;
    Ok(Json(json!({"redirect":url.as_str()})))
}
async fn exchange(State(app): State<App>, Form(v): Form<HashMap<String, String>>) -> Api<Value> {
    let v = json!(v);
    if text(&v, "grant_type") != "authorization_code" {
        return Err(bad("Unsupported grant type"));
    }
    let verifier = text(&v, "code_verifier");
    if !(43..=128).contains(&verifier.len()) {
        return Err(bad("Invalid PKCE verifier"));
    }
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    let row = sqlx::query("DELETE FROM oauth_codes WHERE hash=? RETURNING *")
        .bind(digest(text(&v, "code")))
        .fetch_optional(&app.db)
        .await?
        .ok_or_else(|| bad("Invalid authorization code"))?;
    if row.get::<i64, _>("expires") < chrono::Utc::now().timestamp()
        || row.get::<String, _>("challenge") != challenge
        || row.get::<String, _>("client_id") != text(&v, "client_id")
        || row.get::<String, _>("redirect") != text(&v, "redirect_uri")
    {
        return Err(bad("Invalid authorization code or PKCE proof"));
    }
    let uid: String = row.get("user_id");
    let scope: String = row.get("scope");
    let user = Identity {
        id: uid,
        username: String::new(),
        role: String::new(),
        csrf: String::new(),
        api_token: None,
    };
    let Json(token) = issue(&app, &user, "OAuth client", &scope, 1, json!([])).await?;
    Ok(Json(
        json!({"access_token":token["token"],"token_type":"Bearer","expires_in":86400,"scope":scope}),
    ))
}
