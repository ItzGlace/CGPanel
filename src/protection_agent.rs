use super::*;

pub fn validate(v: &Value) -> Result<()> {
    ensure!(
        (1..=500).contains(&v["requests_per_second"].as_u64().unwrap_or(0)),
        "Rate must be 1–500 requests per second"
    );
    ensure!(
        (1..=200).contains(&v["connections"].as_u64().unwrap_or(0)),
        "Connections must be 1–200 per IP"
    );
    ensure!(
        ["off", "detect", "enforce"].contains(&s(v, "waf")),
        "Invalid WAF mode"
    );
    ensure!(
        ["allow", "block_ai", "block_all"].contains(&s(v, "crawlers")),
        "Invalid crawler policy"
    );
    ensure!(
        ["off", "local", "turnstile", "hcaptcha", "recaptcha"].contains(&s(v, "captcha")),
        "Invalid CAPTCHA provider"
    );
    let paths = v["sitemap_paths"]
        .as_array()
        .context("Sitemap paths must be an array")?;
    ensure!(paths.len() <= 1000, "Maximum 1000 sitemap paths");
    for p in paths {
        let p = p.as_str().context("Invalid sitemap path")?;
        ensure!(
            p.starts_with('/')
                && !p.starts_with("//")
                && p.len() < 300
                && p.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"/._-".contains(&c)),
            "Use simple website paths without query strings"
        );
    }
    Ok(())
}
pub fn nginx(id: &str, item: &Item) -> Result<(String, String, String)> {
    let p = &item.data["protection"];
    if p.is_null() {
        return Ok((
            String::new(),
            "limit_req zone=cgp_web burst=40 nodelay; limit_conn cgp_conn 30;".into(),
            String::new(),
        ));
    }
    validate(p)?;
    let rate = p["requests_per_second"].as_u64().unwrap();
    let connections = p["connections"].as_u64().unwrap();
    let global=format!("limit_req_zone $binary_remote_addr zone=cgp_{id}:1m rate={rate}r/s;\nlog_format cgp_log_{id} escape=json '{{\"at\":\"$time_iso8601\",\"ip\":\"$remote_addr\",\"status\":$status,\"seconds\":$request_time,\"path\":\"$uri\"}}';\n");
    let mut server=format!("access_log /var/log/nginx/cgp_{id}.access.log cgp_log_{id}; limit_req zone=cgp_{id} burst=40 nodelay; limit_req_status 429; limit_conn cgp_conn {connections}; limit_conn_status 429; ");
    let bots=match s(p,"crawlers") {"block_ai"=>"GPTBot|ChatGPT-User|OAI-SearchBot|ClaudeBot|Claude-User|Claude-SearchBot|CCBot|Bytespider|Amazonbot|Google-Extended|Applebot-Extended|PerplexityBot","block_all"=>"bot|crawler|spider|slurp",_=>""};
    if !bots.is_empty() {
        server.push_str(&format!(
            "if ($http_user_agent ~* \"{bots}\") {{ return 403; }} "
        ));
    }
    if s(p, "waf") != "off" {
        server.push_str(&format!(
            "modsecurity on; modsecurity_rules_file /etc/cgpanel/waf-{}.conf; ",
            s(p, "waf")
        ));
    }
    server.push_str(&format!(
        "location = /robots.txt {{ alias /srv/cgpanel/policies/{id}/robots.txt; }} "
    ));
    if p["sitemap"] == true {
        if p["sitemap_auto"] == true {
            server.push_str(&format!("location = /sitemap.xml {{ auth_request off; proxy_pass http://127.0.0.1:2082/guard/{id}/sitemap; proxy_set_header Host $host; }} "));
        } else {
            server.push_str(&format!(
                "location = /sitemap.xml {{ alias /srv/cgpanel/policies/{id}/sitemap.xml; }} "
            ));
        }
    }
    let gate = if s(p, "captcha") != "off" {
        server.push_str(&format!("location = /__cgpanel/challenge {{ auth_request off; modsecurity off; proxy_pass http://127.0.0.1:2082/guard/{id}/challenge; proxy_set_header Host $host; proxy_set_header X-CGPanel-Client-IP $remote_addr; proxy_set_header X-Forwarded-Proto $scheme; client_max_body_size 8k; }} location = /__cgpanel/guard-style.css {{ auth_request off; proxy_pass http://127.0.0.1:2082/style.css; }} location = /__cgpanel/guard-check {{ internal; auth_request off; proxy_pass http://127.0.0.1:2082/guard/{id}/verify; proxy_pass_request_body off; proxy_set_header Content-Length \"\"; proxy_set_header X-CGPanel-Client-IP $remote_addr; proxy_set_header Host $host; }} "));
        "auth_request /__cgpanel/guard-check; error_page 401 =302 /__cgpanel/challenge;".into()
    } else {
        String::new()
    };
    Ok((global, server, gate))
}
pub async fn stats(reg: &Registry, op: &Operation) -> Result<Value> {
    owned(reg, op, "domains")?;
    let file = format!("/var/log/nginx/cgp_{}.access.log", op.id);
    if !Path::new(&file).exists() {
        return Ok(
            json!({"samples":0,"note":"Save protection settings to start collecting traffic summaries"}),
        );
    }
    let output = run("tail", &["-n", "2000", "--", &file]).await?;
    let mut counts: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
    let mut total = 0;
    let mut rejected = 0;
    let mut failed = 0;
    for line in output.lines() {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            let status = v["status"].as_u64().unwrap_or(0);
            let ip = s(&v, "ip");
            if !cgpanel::valid_ip(ip) {
                continue;
            }
            total += 1;
            let count = counts.entry(ip.into()).or_default();
            count.0 += 1;
            if [403, 429].contains(&status) {
                rejected += 1;
                count.1 += 1;
            }
            if status >= 500 {
                failed += 1;
                count.2 += 1;
            }
        }
    }
    let mut sources = counts.into_iter().collect::<Vec<_>>();
    sources.sort_by_key(|(_, c)| std::cmp::Reverse(c.0));
    sources.truncate(20);
    Ok(
        json!({"samples":total,"rejected":rejected,"server_errors":failed,"sources":sources.iter().map(|(ip,c)|json!({"ip":ip,"requests":c.0,"rejected":c.1,"errors":c.2,"unusual":c.1>=20||c.2>=10})).collect::<Vec<_>>(),"note":"Most recent 2,000 logged requests, not a fixed time window. Repeated rejections or errors are investigation signals, not proof of an attack."}),
    )
}
pub async fn configure(reg: &mut Registry, op: &Operation) -> Result<Value> {
    let mut item = owned(reg, op, "domains")?.clone();
    validate(&op.data)?;
    if s(&op.data, "waf") != "off" {
        ensure!(
            Path::new("/etc/cgpanel/waf-enforce.conf").exists(),
            "Install the CGPanel WAF package first"
        );
    }
    let folder = format!("/srv/cgpanel/policies/{}", op.id);
    tokio::fs::create_dir_all(&folder).await?;
    for path in ["/srv/cgpanel/policies", folder.as_str()] {
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).await?;
    }
    let old_robots = tokio::fs::read_to_string(format!("{folder}/robots.txt"))
        .await
        .ok();
    let old_sitemap = tokio::fs::read_to_string(format!("{folder}/sitemap.xml"))
        .await
        .ok();
    let name = s(&item.data, "name");
    let mut robots = match s(&op.data, "crawlers") {
        "block_all" => "User-agent: *\nDisallow: /\n".into(),
        "block_ai" => [
            "GPTBot",
            "ChatGPT-User",
            "OAI-SearchBot",
            "ClaudeBot",
            "Claude-User",
            "Claude-SearchBot",
            "CCBot",
            "Bytespider",
            "Amazonbot",
            "Google-Extended",
            "Applebot-Extended",
            "PerplexityBot",
        ]
        .iter()
        .map(|a| format!("User-agent: {a}\nDisallow: /\n\n"))
        .collect::<String>(),
        _ => "User-agent: *\nAllow: /\n".into(),
    };
    if op.data["sitemap"] == true {
        robots.push_str(&format!("\nSitemap: https://{name}/sitemap.xml\n"));
    }
    atomic(&format!("{folder}/robots.txt"), &robots, 0o644).await?;
    let urls = op.data["sitemap_paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            format!(
                "<url><loc>https://{name}{}</loc></url>",
                p.as_str().unwrap()
            )
        })
        .collect::<String>();
    atomic(&format!("{folder}/sitemap.xml"),&format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">{urls}</urlset>"),0o644).await?;
    item.data["protection"] = op.data.clone();
    if let Err(error) = tls_agent::render_domain(reg, &op.id, &item).await {
        for (name, old) in [("robots.txt", old_robots), ("sitemap.xml", old_sitemap)] {
            let path = format!("{folder}/{name}");
            if let Some(old) = old {
                let _ = atomic(&path, &old, 0o644).await;
            } else {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
        return Err(error);
    }
    reg.items.insert(op.id.clone(), item);
    Ok(json!({"saved":true}))
}
