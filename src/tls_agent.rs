use super::*;
const CERTBOT: &str = "/opt/cgpanel-tools/certbot/bin/certbot";
pub fn telemetry() -> &'static str {
    "location ^~ /__cgpanel/ { proxy_pass http://127.0.0.1:2082/telemetry/; proxy_set_header Host $host; proxy_set_header X-CGPanel-Client-IP $remote_addr; client_max_body_size 8k; }"
}
pub async fn render_domain(reg: &Registry, id: &str, item: &Item) -> Result<()> {
    let name = s(&item.data, "name");
    ensure!(cgpanel::domain(name), "Invalid domain");
    let app = s(&item.data, "app_id");
    let location = if app.is_empty() {
        format!("root /srv/cgpanel/public/{id}; index index.html; location / {{ try_files $uri $uri/ =404; }}")
    } else {
        let app = reference(reg, app, &item.tenant, "apps")?;
        let port = app.data["port"].as_u64().context("Missing port")?;
        format!("location / {{ proxy_pass http://127.0.0.1:{port}; proxy_set_header Host $host; proxy_set_header X-Real-IP $remote_addr; proxy_set_header X-Forwarded-For $remote_addr; proxy_set_header X-Forwarded-Proto $scheme; proxy_http_version 1.1; proxy_set_header Upgrade $http_upgrade; proxy_set_header Connection $cg_connection; proxy_read_timeout 60s; }}")
    };
    let cname = s(&item.data, "cert_name");
    let expected = format!("cgp_{id}");
    let cert = if cname == expected {
        Some(cname.to_owned())
    } else if Path::new(&format!("/etc/letsencrypt/live/{name}/fullchain.pem")).exists() {
        Some(name.to_owned())
    } else {
        None
    };
    let acme = "location ^~ /.well-known/acme-challenge/ { root /srv/cgpanel/acme; }";
    let realip = if item.data["cdn"] == "cloudflare" {
        "include /etc/cgpanel/cloudflare-realip.conf;"
    } else {
        ""
    };
    let common=format!("server_name {name}; {realip} limit_req zone=cgp_web burst=40 nodelay; limit_conn cgp_conn 30; client_max_body_size 32m; {} {location}",telemetry());
    let conf = if let Some(cert) = cert {
        format!("server {{ listen 80; listen [::]:80; server_name {name}; {acme} location / {{ return 301 https://$host$request_uri; }} }}\nserver {{ listen 443 ssl; listen [::]:443 ssl; ssl_certificate /etc/letsencrypt/live/{cert}/fullchain.pem; ssl_certificate_key /etc/letsencrypt/live/{cert}/privkey.pem; ssl_protocols TLSv1.2 TLSv1.3; {common} }}\n")
    } else {
        format!("server {{ listen 80; listen [::]:80; {acme} {common} }}\n")
    };
    let file = format!("/etc/nginx/conf.d/cgp_{id}.conf");
    let old = tokio::fs::read_to_string(&file).await.ok();
    atomic(&file, &conf, 0o644).await?;
    if let Err(error) = run("nginx", &["-t"]).await {
        if let Some(old) = old {
            let _ = atomic(&file, &old, 0o644).await;
        }
        return Err(error);
    }
    run("systemctl", &["reload", "nginx"]).await?;
    Ok(())
}
pub async fn issue(reg: &mut Registry, op: &Operation) -> Result<Value> {
    let item = owned(reg, op, "domains")?.clone();
    let name = s(&item.data, "name");
    ensure!(
        op.data["agree_tos"] == true,
        "Accept the Let's Encrypt subscriber agreement before requesting a certificate"
    );
    let email = s(&op.data, "email");
    ensure!(
        email.contains('@')
            && email.len() < 254
            && !email.starts_with('-')
            && !email.chars().any(char::is_control),
        "Enter a valid certificate email"
    );
    ensure!(
        Path::new(CERTBOT).exists(),
        "Install the CGPanel Certbot environment first"
    );
    let staging = op.data["staging"] == true;
    let cert_name = format!("cgp_{}{}", op.id, if staging { "_staging" } else { "" });
    let mode = s(&op.data, "validation");
    let mut arguments = args(&[
        "certonly",
        "--non-interactive",
        "--agree-tos",
        "--email",
        email,
        "--cert-name",
        &cert_name,
        "-d",
        name,
        "--deploy-hook",
        "/usr/local/lib/cgpanel/reload-nginx",
    ]);
    match mode {
        "http" => {
            render_domain(reg, &op.id, &item).await?;
            arguments.extend(args(&["--webroot", "--webroot-path", "/srv/cgpanel/acme"]));
        }
        "dns_local" => {
            arguments.extend(args(&[
                "--manual",
                "--preferred-challenges",
                "dns",
                "--manual-auth-hook",
                &format!("/usr/local/bin/cgpanel-acme-hook auth {}", op.id),
                "--manual-cleanup-hook",
                &format!("/usr/local/bin/cgpanel-acme-hook cleanup {}", op.id),
            ]));
        }
        "dns_cloudflare" => {
            let integration = super::integrations_agent::connection(
                reg,
                &op.tenant,
                s(&op.data, "integration_id"),
                "cloudflare",
            )?;
            tokio::fs::create_dir_all("/etc/cgpanel/acme").await?;
            let file = format!("/etc/cgpanel/acme/{}.ini", op.id);
            atomic(
                &file,
                &format!(
                    "dns_cloudflare_api_token = {}\n",
                    s(&integration.data, "token")
                ),
                0o600,
            )
            .await?;
            arguments.extend(args(&[
                "--dns-cloudflare",
                "--dns-cloudflare-credentials",
                &file,
                "--dns-cloudflare-propagation-seconds",
                "30",
            ]));
        }
        _ => bail!("Choose HTTP, this server's authoritative DNS, or Cloudflare DNS validation"),
    }
    if op.data["staging"] == true {
        arguments.push("--staging".into());
    }
    exec(CERTBOT,arguments,None,300).await.map_err(|_|anyhow!("Certificate issuance failed. Check DNS, challenge reachability, CAA records and the Certbot log on the server."))?;
    if staging {
        return Ok(json!({"issued":true,"staging":true,"installed":false}));
    }
    let mut updated = item;
    updated.data["cert_name"] = json!(cert_name);
    updated.data["tls_validation"] = json!(mode);
    updated.data["tls_staging"] = json!(op.data["staging"] == true);
    render_domain(reg, &op.id, &updated).await?;
    reg.items.insert(op.id.clone(), updated);
    Ok(json!({"issued":true,"automatic_renewal":true,"staging":op.data["staging"]==true}))
}
pub async fn status(reg: &Registry, op: &Operation) -> Result<Value> {
    let item = owned(reg, op, "domains")?;
    let name = if s(&item.data, "cert_name").is_empty() {
        s(&item.data, "name")
    } else {
        s(&item.data, "cert_name")
    };
    let cert = format!("/etc/letsencrypt/live/{name}/fullchain.pem");
    let details = if Path::new(&cert).exists() {
        run(
            "openssl",
            &[
                "x509", "-in", &cert, "-noout", "-enddate", "-issuer", "-subject",
            ],
        )
        .await
        .unwrap_or_default()
    } else {
        String::new()
    };
    let timer = run("systemctl", &["is-active", "cgpanel-certbot.timer"])
        .await
        .is_ok();
    let renewal = run(
        "systemctl",
        &[
            "show",
            "cgpanel-certbot.service",
            "--property=Result",
            "--property=ExecMainStatus",
            "--property=ExecMainExitTimestamp",
        ],
    )
    .await
    .unwrap_or_default();
    Ok(
        json!({"installed":!details.is_empty(),"certificate":details,"renewal_timer_active":timer,"last_renewal_run":renewal,"validation":item.data["tls_validation"],"cdn":item.data["cdn"]}),
    )
}
pub async fn panel_ip(op: &Operation) -> Result<Value> {
    ensure!(
        op.data["administrator"] == true && op.data["agree_tos"] == true,
        "Administrator agreement is required"
    );
    let ip = std::env::var("CGPANEL_PUBLIC_IP")?;
    ensure!(
        cgpanel::outbound::public_ip(ip.parse()?),
        "A public server IP is required"
    );
    let email = s(&op.data, "email");
    ensure!(
        email.contains('@')
            && email.len() < 254
            && !email.starts_with('-')
            && !email.chars().any(char::is_control),
        "Invalid email"
    );
    atomic("/etc/nginx/conf.d/02-cgpanel-ip.conf",&format!("server {{ listen 80; listen [::]:80; server_name {ip}; location ^~ /.well-known/acme-challenge/ {{ root /srv/cgpanel/acme; }} location / {{ return 301 https://{ip}:2083$request_uri; }} }}\n"),0o644).await?;
    run("nginx", &["-t"]).await?;
    run("systemctl", &["reload", "nginx"]).await?;
    let cert_name = if op.data["staging"] == true {
        "cgp-panel-ip-staging"
    } else {
        "cgp-panel-ip"
    };
    let mut arguments = args(&[
        "certonly",
        "--non-interactive",
        "--agree-tos",
        "--email",
        email,
        "--cert-name",
        cert_name,
        "--preferred-profile",
        "shortlived",
        "--webroot",
        "--webroot-path",
        "/srv/cgpanel/acme",
        "--ip-address",
        &ip,
        "--deploy-hook",
        "/usr/local/lib/cgpanel/reload-nginx",
    ]);
    if op.data["staging"] == true {
        arguments.push("--staging".into());
    }
    exec(CERTBOT, arguments, None, 300).await.map_err(|_| {
        anyhow!("IP certificate issuance failed; inspect Certbot logs and port 80 reachability")
    })?;
    if op.data["staging"] != true {
        let file = "/etc/nginx/conf.d/01-cgpanel.conf";
        let old = tokio::fs::read_to_string(file).await?;
        let config = old
            .replace(
                "/etc/cgpanel/panel.crt",
                "/etc/letsencrypt/live/cgp-panel-ip/fullchain.pem",
            )
            .replace(
                "/etc/cgpanel/panel.key",
                "/etc/letsencrypt/live/cgp-panel-ip/privkey.pem",
            );
        atomic(file, &config, 0o644).await?;
        run("nginx", &["-t"]).await?;
        run("systemctl", &["reload", "nginx"]).await?;
    }
    Ok(
        json!({"issued":true,"short_lived":true,"installed":op.data["staging"]!=true,"automatic_renewal":true}),
    )
}
pub async fn execute(reg: &mut Registry, op: &Operation) -> Result<Value> {
    match op.action.as_str() {
        "tls_issue" => issue(reg, op).await,
        "tls_status" => status(reg, op).await,
        "tls_panel_ip" => panel_ip(op).await,
        "zone_export" => {
            owned(reg, op, "domains")?;
            Ok(
                json!({"zone":tokio::fs::read_to_string(format!("/etc/bind/cgpanel/{}.zone",op.id)).await?}),
            )
        }
        "cdn_configure" => {
            let mut item = owned(reg, op, "domains")?.clone();
            ensure!(
                ["none", "cloudflare"].contains(&s(&op.data, "provider")),
                "Choose a supported CDN"
            );
            item.data["cdn"] = op.data["provider"].clone();
            render_domain(reg, &op.id, &item).await?;
            reg.items.insert(op.id.clone(), item);
            Ok(json!({"saved":true}))
        }
        _ => bail!("Unknown certificate or DNS operation"),
    }
}
