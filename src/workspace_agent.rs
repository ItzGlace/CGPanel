use super::*;
pub fn catalog() -> Value {
    json!({"php":["8.2","8.3","8.4","8.5"],"python":["3.11","3.12","3.13","3.14"],"node":["22","24"],"java":["17","21","25"],"rust":["stable"],"static":["3.13","3.14"]})
}
pub fn image(runtime: &str, version: &str) -> Result<String> {
    let official_tag = version.strip_prefix("tag:");
    if let Some(tag) = official_tag {
        ensure!(
            tag.len() <= 100
                && !tag.is_empty()
                && tag.as_bytes()[0].is_ascii_alphanumeric()
                && tag
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c)),
            "Invalid official image tag"
        );
        if runtime == "php" {
            ensure!(
                tag.contains("-fpm"),
                "PHP profiles require an official FPM tag"
            );
        }
    }
    ensure!(
        official_tag.is_some()
            || catalog()[runtime]
                .as_array()
                .is_some_and(|a| a.contains(&json!(version))),
        "Unsupported runtime version"
    );
    if runtime == "php" {
        return Ok(format!(
            "localhost/cgpanel-php:{}",
            version.replace(':', "-")
        ));
    }
    let registry = std::env::var("CGPANEL_IMAGE_REGISTRY").unwrap_or("docker.io".into());
    ensure!(
        ["docker.io", "public.ecr.aws/docker"].contains(&registry.as_str()),
        "Unsupported registry"
    );
    let image = if let Some(tag) = official_tag {
        let repository = match runtime {
            "python" | "static" => "python",
            "node" => "node",
            "java" => "eclipse-temurin",
            "rust" => "rust",
            _ => bail!("Unsupported runtime"),
        };
        format!("{repository}:{tag}")
    } else {
        match runtime {
            "python" | "static" => format!("python:{version}-slim-bookworm"),
            "node" => format!("node:{version}-bookworm-slim"),
            "java" => format!("eclipse-temurin:{version}-jdk"),
            "rust" => "rust:1-bookworm".into(),
            _ => bail!("Unsupported runtime"),
        }
    };
    Ok(format!("{registry}/library/{image}"))
}
pub async fn prepare(t: &str, runtime: &str, version: &str) -> Result<String> {
    let image = image(runtime, version)?;
    if runtime == "php" {
        exec(
            "podman",
            args(&[
                "build",
                "--build-arg",
                &format!("PHP_VERSION={version}"),
                "--build-arg",
                &format!(
                    "PHP_BASE_TAG={}",
                    version
                        .strip_prefix("tag:")
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("{version}-fpm-bookworm"))
                ),
                "--build-arg",
                &format!(
                    "PHP_REGISTRY={}",
                    std::env::var("CGPANEL_IMAGE_REGISTRY").unwrap_or("docker.io".into())
                ),
                "-t",
                &image,
                "/usr/local/lib/cgpanel/php-runtime",
            ]),
            None,
            1800,
        )
        .await?;
        let archive = format!(
            "/usr/local/lib/cgpanel/php-{}.tar",
            version.replace(':', "-")
        );
        exec(
            "podman",
            args(&["save", "--format", "oci-archive", "-o", &archive, &image]),
            None,
            180,
        )
        .await?;
        run("chmod", &["0644", &archive]).await?;
        pod(t, args(&["load", "-i", &archive]), None, 300).await?;
    } else {
        pod(t, args(&["pull", &image]), None, 600).await?;
    }
    Ok(image)
}
pub async fn runtime(reg: &mut Registry, op: &Operation) -> Result<Value> {
    let old = owned(reg, op, "apps")?.clone();
    let version = s(&op.data, "version");
    let runtime = s(&old.data, "runtime");
    let image = prepare(&op.tenant, runtime, version).await?;
    ensure!(
        old.data["egress_proxy_id"]
            .as_str()
            .unwrap_or("")
            .is_empty(),
        "Disable outgoing proxy before changing runtime; re-enable it afterwards"
    );
    let previous = format!("{}_previous", container(&op.id));
    ensure!(
        pod(
            &op.tenant,
            args(&["container", "exists", &previous]),
            None,
            15
        )
        .await
        .is_err(),
        "A previous runtime is retained; resolve it before switching again"
    );
    let was_running = pod(
        &op.tenant,
        args(&[
            "inspect",
            "--format",
            "{{.State.Running}}",
            &container(&op.id),
        ]),
        None,
        15,
    )
    .await?
    .trim()
        == "true";
    pod(
        &op.tenant,
        args(&["stop", "--time", "20", &container(&op.id)]),
        None,
        35,
    )
    .await?;
    pod(
        &op.tenant,
        args(&["rename", &container(&op.id), &previous]),
        None,
        20,
    )
    .await?;
    let mut data = old.data.clone();
    data["version"] = json!(version);
    if op.data.get("command").is_some() {
        data["command"] = op.data["command"].clone();
    }
    let result = create_app(
        reg,
        &Operation {
            action: "runtime_configure".into(),
            tenant: op.tenant.clone(),
            id: op.id.clone(),
            data,
        },
    )
    .await;
    let mut health = if result.is_ok() {
        tokio::time::sleep(Duration::from_secs(4)).await;
        pod(
            &op.tenant,
            args(&[
                "inspect",
                "--format",
                "{{.State.Running}}",
                &container(&op.id),
            ]),
            None,
            15,
        )
        .await
        .is_ok_and(|s| s.trim() == "true")
    } else {
        false
    };
    if health {
        // Older supported Podman releases cannot change restart policy in place.
        // Validate without automatic restarts, then create the normal container.
        let activation = async {
            pod(
                &op.tenant,
                args(&["stop", "--time", "2", &container(&op.id)]),
                None,
                15,
            )
            .await?;
            pod(&op.tenant, args(&["rm", &container(&op.id)]), None, 20).await?;
            let data = reg
                .items
                .get(&op.id)
                .context("Runtime configuration missing")?
                .data
                .clone();
            create_app(
                reg,
                &Operation {
                    action: "runtime_activate".into(),
                    tenant: op.tenant.clone(),
                    id: op.id.clone(),
                    data,
                },
            )
            .await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        health = activation.is_ok();
    }
    if !health {
        let _ = pod(
            &op.tenant,
            args(&["rm", "-f", "--ignore", &container(&op.id)]),
            None,
            20,
        )
        .await;
        pod(
            &op.tenant,
            args(&["rename", &previous, &container(&op.id)]),
            None,
            20,
        )
        .await?;
        if was_running {
            pod(&op.tenant, args(&["start", &container(&op.id)]), None, 30).await?;
        }
        reg.items.insert(op.id.clone(), old);
        bail!("New runtime did not stay running; restored the previous container. Check the startup command and dependencies.");
    }
    if !was_running {
        pod(&op.tenant, args(&["stop", &container(&op.id)]), None, 35).await?;
    }
    pod(&op.tenant, args(&["rm", &previous]), None, 20).await?;
    Ok(
        json!({"version":version,"image":image,"state":if was_running{"running"}else{"stopped"},"validation":"container startup; test application compatibility separately"}),
    )
}
pub async fn files(reg: &Registry, op: &Operation) -> Result<Value> {
    owned(reg, op, "apps")?;
    let output = exec(
        "runuser",
        args(&[
            "-u",
            &user(&op.tenant),
            "--",
            "/usr/local/bin/cgpanel-workspace",
            &appdir(&op.tenant, &op.id),
        ]),
        Some(op.data.to_string()),
        120,
    )
    .await?;
    Ok(serde_json::from_str(output.trim())?)
}
pub async fn git(reg: &Registry, op: &Operation) -> Result<Value> {
    let item = owned(reg, op, "apps")?;
    ensure!(
        item.data["ide_enabled"] == true,
        "Start the workspace IDE to use its Git installation"
    );
    let mut command = args(&[
        "exec",
        "--user",
        "1000:1000",
        "--workdir",
        "/workspace",
        &format!("cgp_ide_{}", op.id),
        "git",
        "-C",
        "/workspace",
    ]);
    match s(&op.data, "operation") {
        "status" => command.extend(args(&["status", "--short", "--branch"])),
        "history" => command.extend(args(&["log", "-20", "--oneline", "--decorate"])),
        "diff" => command.extend(args(&["diff", "--stat"])),
        "init" => command.extend(args(&["init", "--initial-branch=main"])),
        _ => bail!("Choose status, history, diff or init"),
    }
    Ok(json!({"output":pod(&op.tenant,command,None,30).await?}))
}
fn ide_domain(reg: &Registry, op: &Operation) -> Option<String> {
    reg.items
        .values()
        .filter(|i| i.kind == "domains" && i.tenant == op.tenant && s(&i.data, "app_id") == op.id)
        .map(|i| s(&i.data, "name").to_owned())
        .find(|name| cgpanel::domain(name))
}
pub async fn ide_status(reg: &Registry, op: &Operation) -> Result<Value> {
    let item = owned(reg, op, "apps")?;
    let url = if let Some(domain) = ide_domain(reg, op) {
        format!("https://{domain}:8443/")
    } else {
        format!(
            "https://{}:{}/",
            std::env::var("CGPANEL_PUBLIC_IP").unwrap_or_default(),
            item.data["ide_port"].as_u64().unwrap_or(0) + 20000
        )
    };
    Ok(
        json!({"installed":item.data["ide_installed"]==true,"enabled":item.data["ide_enabled"]==true,"url":url,"memory_mb":384,"tools":["Git source control","Integrated terminal","Open VSX extensions"],"routing":"Assigned domain on CDN-compatible HTTPS port 8443, isolated from the panel origin"}),
    )
}
pub async fn render_ide(reg: &Registry, op: &Operation) -> Result<()> {
    let item = owned(reg, op, "apps")?;
    if item.data["ide_enabled"] != true {
        return Ok(());
    }
    let port = item.data["ide_port"].as_u64().context("Missing IDE port")?;
    ensure!((20000..40000).contains(&port), "Invalid IDE port");
    tokio::fs::create_dir_all("/etc/nginx/cgpanel-ide").await?;
    let panel = tokio::fs::read_to_string("/etc/nginx/conf.d/01-cgpanel.conf").await?;
    let cert = panel
        .lines()
        .find_map(|l| l.trim().strip_prefix("ssl_certificate "))
        .context("Panel certificate missing")?;
    let key = panel
        .lines()
        .find_map(|l| l.trim().strip_prefix("ssl_certificate_key "))
        .context("Panel key missing")?;
    let mut config=format!("server {{ listen {} ssl; listen [::]:{} ssl; server_name _; ssl_certificate {cert} ssl_certificate_key {key} ssl_protocols TLSv1.2 TLSv1.3; client_max_body_size 32m; location / {{ proxy_pass http://127.0.0.1:{port}; proxy_http_version 1.1; proxy_set_header Host $http_host; proxy_set_header X-Forwarded-Proto https; proxy_set_header Upgrade $http_upgrade; proxy_set_header Connection $cg_connection; proxy_read_timeout 3600s; proxy_buffering off; }} }}\n",port+20000,port+20000);
    if let Some(domain) = ide_domain(reg, op) {
        let domain_item = reg
            .items
            .iter()
            .find(|(_, i)| i.kind == "domains" && s(&i.data, "name") == domain)
            .context("Domain missing")?;
        let certificate = format!("/etc/letsencrypt/live/cgp_{}/fullchain.pem", domain_item.0);
        let (cert, key) = if Path::new(&certificate).exists() {
            (
                format!("{certificate};"),
                format!("/etc/letsencrypt/live/cgp_{}/privkey.pem;", domain_item.0),
            )
        } else {
            (cert.to_owned(), key.to_owned())
        };
        config.push_str(&format!("server {{ listen 8443 ssl; listen [::]:8443 ssl; server_name {domain}; ssl_certificate {cert} ssl_certificate_key {key} ssl_protocols TLSv1.2 TLSv1.3; client_max_body_size 32m; location / {{ proxy_pass http://127.0.0.1:{port}; proxy_http_version 1.1; proxy_set_header Host $http_host; proxy_set_header X-Forwarded-Proto https; proxy_set_header Upgrade $http_upgrade; proxy_set_header Connection $cg_connection; proxy_read_timeout 3600s; proxy_buffering off; }} }}\n"));
    }
    let file = format!("/etc/nginx/cgpanel-ide/{}.conf", op.id);
    let old = tokio::fs::read_to_string(&file).await.ok();
    atomic(&file, &config, 0o644).await?;
    if let Err(e) = run("nginx", &["-t"]).await {
        if let Some(old) = old {
            atomic(&file, &old, 0o644).await?
        } else {
            let _ = tokio::fs::remove_file(&file).await;
        }
        return Err(e);
    }
    run("systemctl", &["reload", "nginx"]).await?;
    Ok(())
}
pub async fn ide(reg: &mut Registry, op: &Operation) -> Result<Value> {
    let mut item = owned(reg, op, "apps")?.clone();
    let name = format!("cgp_ide_{}", op.id);
    let conf = format!("/etc/nginx/cgpanel-ide/{}.conf", op.id);
    if op.data["enabled"] != true {
        let _ = pod(&op.tenant, args(&["rm", "-f", "--ignore", &name]), None, 30).await;
        let _ = tokio::fs::remove_file(conf).await;
        run("systemctl", &["reload", "nginx"]).await?;
        item.data["ide_enabled"] = json!(false);
        reg.items.insert(op.id.clone(), item);
        firewall(reg).await?;
        return Ok(json!({"enabled":false}));
    }
    let image = "docker.io/codercom/code-server:4.138.0";
    resources_agent::check(
        reg,
        &Operation {
            action: op.action.clone(),
            tenant: op.tenant.clone(),
            id: op.id.clone(),
            data: item.data.clone(),
        },
        true,
    )?;
    pod(&op.tenant, args(&["pull", image]), None, 600).await?;
    let password = if op.data["rotate"] == true || s(&item.data, "ide_password").is_empty() {
        random_secret()
    } else {
        s(&item.data, "ide_password").to_owned()
    };
    let port = item.data["ide_port"].as_u64().unwrap_or_else(|| {
        reg.ports = reg.ports.max(20000) + 1;
        reg.ports as u64
    });
    ensure!(port < 40000, "Port range exhausted");
    let private = format!("{}/ide/{}", home(&op.tenant), op.id);
    tokio::fs::create_dir_all(&private).await?;
    run(
        "chown",
        &[
            "-R",
            &format!("{}:{}", user(&op.tenant), user(&op.tenant)),
            &format!("{}/ide", home(&op.tenant)),
        ],
    )
    .await?;
    let _ = pod(&op.tenant, args(&["rm", "-f", "--ignore", &name]), None, 30).await;
    pod(
        &op.tenant,
        args(&[
            "run",
            "-d",
            "--name",
            &name,
            "--restart",
            "on-failure:3",
            "--userns",
            "keep-id:uid=1000,gid=1000",
            "--user",
            "1000:1000",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--read-only",
            "--tmpfs",
            "/tmp:rw,nosuid,size=128m",
            "--memory",
            "384m",
            "--cpus",
            "0.75",
            "--pids-limit",
            "192",
            "--network",
            "slirp4netns:allow_host_loopback=false",
            "-p",
            &format!("127.0.0.1:{port}:8080"),
            "-v",
            &format!("{}:/workspace:rw", appdir(&op.tenant, &op.id)),
            "-v",
            &format!("{private}:/home/coder:rw"),
            "-e",
            &format!("PASSWORD={password}"),
            "-e",
            "HOME=/home/coder",
            "--log-opt",
            "max-size=10mb",
            image,
            "--bind-addr",
            "0.0.0.0:8080",
            "--auth",
            "password",
            "--disable-telemetry",
            "--disable-update-check",
            "/workspace",
        ]),
        None,
        90,
    )
    .await?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()?;
    let mut ready = false;
    for _ in 0..60 {
        if client
            .get(format!("http://127.0.0.1:{port}/healthz"))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    if !ready {
        let _ = pod(&op.tenant, args(&["rm", "-f", "--ignore", &name]), None, 30).await;
        bail!("Workspace IDE did not become ready. Check available memory and retry.");
    }
    item.data["ide_password"] = json!(password);
    item.data["ide_port"] = json!(port);
    item.data["ide_installed"] = json!(true);
    item.data["ide_enabled"] = json!(true);
    reg.items.insert(op.id.clone(), item);
    render_ide(reg, op).await?;
    firewall(reg).await?;
    // Password is retrieved through a session-protected endpoint, not stored in jobs/SQL.
    Ok(json!({"installed":true,"enabled":true}))
}

pub async fn firewall(reg: &Registry) -> Result<()> {
    let mut ports: Vec<_> = reg
        .items
        .values()
        .filter(|i| i.kind == "apps" && i.data["ide_enabled"] == true)
        .filter_map(|i| i.data["ide_port"].as_u64())
        .map(|p| (p + 20000).to_string())
        .collect();
    if !ports.is_empty() {
        ports.push("8443".into());
    }
    let mut commands = String::from("flush set inet cgpanel ide_ports\n");
    if !ports.is_empty() {
        commands.push_str(&format!(
            "add element inet cgpanel ide_ports {{ {} }}\n",
            ports.join(",")
        ));
    }
    exec("nft", args(&["-f", "-"]), Some(commands), 30).await?;
    Ok(())
}
