use super::*;
use axum::extract::Query;
use std::sync::OnceLock;

pub fn routes() -> Router<App> {
    Router::new()
        .route("/v2/monitor/{id}", get(status).post(configure))
        .route("/v2/analytics/{id}", get(analytics))
}
pub fn public_routes() -> Router<App> {
    Router::new()
        .route(
            "/telemetry/tracker.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/tracker.js"),
                )
            }),
        )
        .route(
            "/telemetry/collect/{id}",
            post(collect).layer(DefaultBodyLimit::max(8192)),
        )
}
async fn domain_resource(app: &App, user: &Identity, id: &str) -> Result<Resource, Error> {
    let resource = owned(app, user, id).await?;
    if resource.kind != "domains" {
        return Err(bad("Choose an assigned domain"));
    }
    Ok(resource)
}
fn configuration(value: &Value) -> Result<Value, Error> {
    let scheme = text(value, "scheme");
    if !["http", "https"].contains(&scheme) {
        return Err(bad("Choose HTTP or HTTPS"));
    }
    let path = text(value, "path");
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.len() > 300
        || path.contains(['?', '#', '\\'])
        || path.chars().any(char::is_control)
    {
        return Err(bad("Use a page path without a query string or fragment"));
    }
    Ok(
        json!({"scheme":scheme,"path":path,"interval":value["interval"].as_i64().unwrap_or(60).clamp(60,3600),"telegram_id":text(value,"telegram_id"),"analytics":value["analytics"]==true,"clicks":value["clicks"]==true,"retention_days":value["retention_days"].as_i64().unwrap_or(30).clamp(1,30)}),
    )
}
async fn configure(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Json(value): Json<Value>,
) -> Api<Value> {
    let resource = domain_resource(&app, &user, &rid).await?;
    let config = configuration(&value)?;
    if !text(&config, "telegram_id").is_empty() {
        let connections = host(
            &app,
            &user,
            "integration_list",
            &resource.owner,
            &resource.owner,
            json!({}),
        )
        .await?;
        if !connections.as_array().is_some_and(|a| {
            a.iter()
                .any(|v| v["id"] == config["telegram_id"] && v["type"] == "telegram")
        }) {
            return Err(bad("Choose this account's Telegram integration"));
        }
    }
    let now = chrono::Utc::now().timestamp();
    sqlx::query("INSERT INTO monitor_settings(domain_id,owner,enabled,config,next_check,analytics_key,salt) VALUES(?,?,?,?,?,?,?) ON CONFLICT(domain_id) DO UPDATE SET enabled=excluded.enabled,config=excluded.config,next_check=excluded.next_check")
        .bind(&rid).bind(&resource.owner).bind(value["enabled"]==true).bind(config.to_string()).bind(now).bind(id()).bind(format!("{}{}",id(),id())).execute(&app.db).await?;
    audit(&app, &user.id, "monitor:configure", &rid).await;
    Ok(Json(json!({"saved":true})))
}
async fn status(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
) -> Api<Value> {
    let resource = domain_resource(&app, &user, &rid).await?;
    let setting = sqlx::query("SELECT * FROM monitor_settings WHERE domain_id=?")
        .bind(&rid)
        .fetch_optional(&app.db)
        .await?;
    let settings = setting.map(|r|json!({"enabled":r.get::<bool,_>("enabled"),"config":serde_json::from_str::<Value>(&r.get::<String,_>("config")).unwrap_or(json!({})),"state":r.get::<String,_>("state"),"next_check":r.get::<i64,_>("next_check"),"analytics_key":r.get::<String,_>("analytics_key")})).unwrap_or(json!({"enabled":false,"state":"not configured","config":{"scheme":"https","path":"/","interval":60,"retention_days":30}}));
    let rows = sqlx::query("SELECT checked,status,latency_ms,error FROM monitor_checks WHERE domain_id=? ORDER BY checked DESC LIMIT 120").bind(&rid).fetch_all(&app.db).await?;
    let checks: Vec<_> = rows.iter().map(|r|json!({"at":r.get::<i64,_>("checked"),"status":r.get::<i64,_>("status"),"latency_ms":r.get::<i64,_>("latency_ms"),"error":r.get::<String,_>("error")})).collect();
    let counts = sqlx::query("SELECT count(*) total,coalesce(sum(status>=200 AND status<400),0) up FROM monitor_checks WHERE domain_id=? AND checked>=?").bind(&rid).bind(chrono::Utc::now().timestamp()-86400).fetch_one(&app.db).await?;
    Ok(Json(
        json!({"domain":resource.name,"settings":settings,"checks":checks,"last_day":{"samples":counts.get::<i64,_>("total"),"up":counts.get::<i64,_>("up")}}),
    ))
}
type TelemetryLimits = Mutex<HashMap<String, (Instant, u32)>>;
static LIMITS: OnceLock<TelemetryLimits> = OnceLock::new();
async fn collect(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(rid): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<StatusCode, Error> {
    // text/plain JSON permits sendBeacon across the website/panel origins without
    // a CORS preflight. Authentication still requires the configured site and Origin.
    let value: Value = serde_json::from_slice(&body).map_err(|_| bad("Invalid event JSON"))?;
    if !identifier(&rid) || rid.len() != 32 {
        return Err(bad("Invalid site"));
    }
    let row = sqlx::query("SELECT m.config,m.analytics_key,m.salt,r.name FROM monitor_settings m JOIN resources r ON r.id=m.domain_id JOIN users u ON u.id=m.owner WHERE m.domain_id=? AND u.enabled=1")
        .bind(&rid).fetch_optional(&app.db).await?.ok_or_else(||bad("Tracking is not enabled"))?;
    let config: Value = serde_json::from_str(&row.get::<String, _>("config")).unwrap_or(json!({}));
    if config["analytics"] != true || text(&value, "key") != row.get::<String, _>("analytics_key") {
        return Err(forbidden());
    }
    let domain: String = row.get("name");
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| reqwest::Url::parse(v).ok());
    if !origin
        .is_some_and(|v| ["http", "https"].contains(&v.scheme()) && v.host_str() == Some(&domain))
    {
        return Err(forbidden());
    }
    if headers.get("dnt").is_some_and(|v| v == "1") {
        return Ok(StatusCode::NO_CONTENT);
    }
    let kind = text(&value, "kind");
    if !["view", "click"].contains(&kind) || (kind == "click" && config["clicks"] != true) {
        return Err(bad("Event is not enabled"));
    }
    let path = text(&value, "path").split(['?', '#']).next().unwrap_or("");
    if !path.starts_with('/') || path.len() > 300 || path.chars().any(char::is_control) {
        return Err(bad("Invalid page path"));
    }
    let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let visitor = digest(&format!(
        "{}:{rid}:{day}:{}",
        row.get::<String, _>("salt"),
        client_ip(peer.ip(), &headers)
    ));
    {
        let mut limits = LIMITS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .await;
        if limits.len() > 10000 {
            limits.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(60));
        }
        if limits.len() > 10000 {
            return Err(Error(
                StatusCode::TOO_MANY_REQUESTS,
                "Analytics capacity reached".into(),
            ));
        }
        let entry = limits.entry(visitor.clone()).or_insert((Instant::now(), 0));
        if entry.0.elapsed() >= Duration::from_secs(60) {
            *entry = (Instant::now(), 0);
        }
        entry.1 += 1;
        if entry.1 > 120 {
            return Err(Error(
                StatusCode::TOO_MANY_REQUESTS,
                "Analytics rate limit".into(),
            ));
        }
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM analytics_events WHERE domain_id=? AND day=?")
            .bind(&rid)
            .bind(&day)
            .fetch_one(&app.db)
            .await?;
    if count >= 100000 {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Daily analytics limit reached".into(),
        ));
    }
    let referrer = text(&value, "referrer");
    let referrer = if cgpanel::domain(referrer) {
        referrer
    } else {
        ""
    }; // Hostname only; never retain a full referrer URL.
    let target = text(&value, "target")
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect::<String>();
    sqlx::query("INSERT INTO analytics_events(domain_id,day,at,visitor,path,kind,referrer,x,y,viewport,target) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&rid).bind(day).bind(chrono::Utc::now().timestamp()).bind(visitor).bind(path).bind(kind).bind(referrer)
        .bind(value["x"].as_i64().unwrap_or(0).clamp(0,999)).bind(value["y"].as_i64().unwrap_or(0).clamp(0,999))
        .bind(if value["viewport"]=="mobile" {"mobile"} else {"desktop"}).bind(target).execute(&app.db).await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn analytics(
    State(app): State<App>,
    Extension(user): Extension<Identity>,
    Path(rid): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Api<Value> {
    domain_resource(&app, &user, &rid).await?;
    let since = chrono::Utc::now().timestamp() - 30 * 86400;
    let daily = sqlx::query("SELECT day,count(*) views,count(DISTINCT visitor) unique_ips FROM analytics_events WHERE domain_id=? AND kind='view' AND at>=? GROUP BY day ORDER BY day")
        .bind(&rid).bind(since).fetch_all(&app.db).await?;
    let pages = sqlx::query("SELECT path,count(*) views,count(DISTINCT visitor) unique_ip_days FROM analytics_events WHERE domain_id=? AND kind='view' AND at>=? GROUP BY path ORDER BY views DESC LIMIT 30")
        .bind(&rid).bind(since).fetch_all(&app.db).await?;
    let page = query
        .get("path")
        .cloned()
        .unwrap_or_else(|| pages.first().map(|r| r.get("path")).unwrap_or("/".into()));
    let viewport = if query.get("viewport").is_some_and(|v| v == "mobile") {
        "mobile"
    } else {
        "desktop"
    };
    let clicks=sqlx::query("SELECT x/100 col,y/50 row,count(*) count FROM analytics_events WHERE domain_id=? AND path=? AND kind='click' AND viewport=? AND at>=? GROUP BY col,row")
        .bind(&rid).bind(&page).bind(viewport).bind(since).fetch_all(&app.db).await?;
    let targets=sqlx::query("SELECT target,count(*) count FROM analytics_events WHERE domain_id=? AND path=? AND kind='click' AND at>=? GROUP BY target ORDER BY count DESC LIMIT 15")
        .bind(&rid).bind(&page).bind(since).fetch_all(&app.db).await?;
    let referrers=sqlx::query("SELECT referrer,count(*) count FROM analytics_events WHERE domain_id=? AND kind='view' AND at>=? GROUP BY referrer ORDER BY count DESC LIMIT 15")
        .bind(&rid).bind(since).fetch_all(&app.db).await?;
    Ok(Json(
        json!({"daily":daily.iter().map(|r|json!({"day":r.get::<String,_>("day"),"views":r.get::<i64,_>("views"),"unique_ips":r.get::<i64,_>("unique_ips")})).collect::<Vec<_>>(),
        "pages":pages.iter().map(|r|json!({"path":r.get::<String,_>("path"),"views":r.get::<i64,_>("views"),"unique_ip_days":r.get::<i64,_>("unique_ip_days")})).collect::<Vec<_>>(),
        "heatmap":{"path":page,"viewport":viewport,"columns":10,"rows":20,"cells":clicks.iter().map(|r|json!({"col":r.get::<i64,_>("col"),"row":r.get::<i64,_>("row"),"count":r.get::<i64,_>("count")})).collect::<Vec<_>>()},
        "targets":targets.iter().map(|r|json!({"label":r.get::<String,_>("target"),"count":r.get::<i64,_>("count")})).collect::<Vec<_>>(),
        "referrers":referrers.iter().map(|r|json!({"host":r.get::<String,_>("referrer"),"count":r.get::<i64,_>("count")})).collect::<Vec<_>>() }),
    ))
}
pub fn start(app: App) {
    tokio::spawn(async move {
        loop {
            if let Err(error) = tick(&app).await {
                tracing::warn!(error=%error.1,"monitoring cycle failed");
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}
async fn tick(app: &App) -> Result<(), Error> {
    let now = chrono::Utc::now().timestamp();
    let rows=sqlx::query("SELECT m.*,r.name FROM monitor_settings m JOIN resources r ON r.id=m.domain_id JOIN users u ON u.id=m.owner WHERE m.enabled=1 AND m.next_check<=? AND u.enabled=1 LIMIT 20").bind(now).fetch_all(&app.db).await?;
    for row in rows {
        let rid: String = row.get("domain_id");
        let owner: String = row.get("owner");
        let config: Value =
            serde_json::from_str(&row.get::<String, _>("config")).unwrap_or(json!({}));
        sqlx::query("UPDATE monitor_settings SET next_check=? WHERE domain_id=?")
            .bind(now + config["interval"].as_i64().unwrap_or(60))
            .bind(&rid)
            .execute(&app.db)
            .await?;
        let url = format!(
            "{}://{}{}",
            text(&config, "scheme"),
            row.get::<String, _>("name"),
            text(&config, "path")
        );
        let (status, latency, error) = match cgpanel::site::probe(&url).await {
            Ok((status, latency)) => (i64::from(status), latency as i64, String::new()),
            Err(error) => (0, 0, error.to_string().chars().take(250).collect()),
        };
        sqlx::query("INSERT INTO monitor_checks(domain_id,checked,status,latency_ms,error) VALUES(?,?,?,?,?)").bind(&rid).bind(now).bind(status).bind(latency).bind(error).execute(&app.db).await?;
        let up = (200..400).contains(&status);
        let old: String = row.get("state");
        let failures = if up {
            0
        } else {
            row.get::<i64, _>("failures") + 1
        };
        let state = if up {
            "up"
        } else if failures >= 2 {
            "down"
        } else {
            &old
        };
        let mut transaction = app.db.begin().await?;
        sqlx::query("UPDATE monitor_settings SET state=?,failures=? WHERE domain_id=?")
            .bind(state)
            .bind(failures)
            .bind(&rid)
            .execute(&mut *transaction)
            .await?;
        let alert = text(&config, "telegram_id");
        if !alert.is_empty() && state != old && (state == "down" || old == "down") {
            let message = format!(
                "CGPanel: {} is {}. HTTP {}. Checked from your hosting server.",
                row.get::<String, _>("name"),
                state.to_uppercase(),
                status
            );
            sqlx::query("INSERT INTO jobs(id,owner,kind,target,payload,created) VALUES(?,?,'monitor_alert',?,?,?)")
                .bind(id()).bind(&owner).bind(alert).bind(json!({"message":message}).to_string()).bind(now).execute(&mut *transaction).await?;
        }
        transaction.commit().await?;
    }
    sqlx::query("DELETE FROM monitor_checks WHERE checked<?")
        .bind(now - 30 * 86400)
        .execute(&app.db)
        .await?;
    sqlx::query("DELETE FROM analytics_events WHERE at<? OR at < ? - 86400*coalesce((SELECT json_extract(config,'$.retention_days') FROM monitor_settings WHERE domain_id=analytics_events.domain_id),30)")
        .bind(now-30*86400).bind(now).execute(&app.db).await?;
    Ok(())
}
