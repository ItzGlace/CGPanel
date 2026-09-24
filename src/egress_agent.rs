use super::*;

pub fn gateway(id: &str) -> String {
    format!("cgpx_{id}")
}
pub async fn cleanup(tenant: &str, id: &str) {
    let _ = pod(
        tenant,
        args(&["rm", "-f", "--ignore", &gateway(id)]),
        None,
        30,
    )
    .await;
    let _ = pod(tenant, args(&["secret", "rm", &gateway(id)]), None, 20).await;
}
pub async fn prepare(reg: &Registry, op: &Operation, port: u16) -> Result<String> {
    let proxy_id = s(&op.data, "egress_proxy_id");
    if proxy_id.is_empty() {
        return Ok(String::new());
    }
    let item = super::integrations_agent::connection(reg, &op.tenant, proxy_id, "proxy")?;
    let proxy = cgpanel::outbound::proxy_url(s(&item.data, "url")).await?;
    ensure!(
        reqwest::Url::parse(&proxy)?
            .host_str()
            .is_some_and(|host| host.parse::<std::net::Ipv4Addr>().is_ok()),
        "Application egress requires an IPv4 SOCKS endpoint"
    );
    let image = "localhost/cgpanel-egress:0.2.0";
    if pod(&op.tenant, args(&["image", "exists", image]), None, 20)
        .await
        .is_err()
    {
        ensure!(
            Path::new("/usr/local/lib/cgpanel/egress-image.tar").exists(),
            "The administrator must install the CGPanel egress image first"
        );
        pod(
            &op.tenant,
            args(&["load", "--input", "/usr/local/lib/cgpanel/egress-image.tar"]),
            None,
            180,
        )
        .await?;
    }
    cleanup(&op.tenant, &op.id).await;
    pod(
        &op.tenant,
        args(&["secret", "create", &gateway(&op.id), "-"]),
        Some(json!({"url":proxy}).to_string()),
        20,
    )
    .await?;
    let mut options = args(&[
        "run",
        "-d",
        "--name",
        &gateway(&op.id),
        "--restart",
        "on-failure:3",
        "--userns=keep-id:uid=1000,gid=1000",
        "--user",
        "0:0",
        "--cap-drop=ALL",
        "--cap-add=NET_ADMIN",
        "--cap-add=NET_BIND_SERVICE",
        "--security-opt=no-new-privileges",
        "--read-only",
        "--tmpfs",
        "/tmp:rw,nosuid,noexec,size=4m",
        "--memory",
        "192m",
        "--cpus",
        "0.5",
        "--pids-limit",
        "64",
        "--network",
        "slirp4netns:allow_host_loopback=false",
        "--dns",
        "127.0.0.1",
        "--secret",
        &format!("{},target=proxy.json", gateway(&op.id)),
        "--log-opt",
        "max-size=1mb",
    ]);
    if s(&op.data, "mode") == "web" {
        options.extend(args(&["-p", &format!("127.0.0.1:{port}:8080")]));
    }
    options.push(image.into());
    pod(&op.tenant, options, None, 60).await?;
    for _ in 0..20 {
        if pod(
            &op.tenant,
            args(&["exec", &gateway(&op.id), "test", "-f", "/tmp/ready"]),
            None,
            10,
        )
        .await
        .is_ok()
        {
            return Ok(format!("container:{}", gateway(&op.id)));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    bail!("The egress gateway did not become ready; the application remains stopped")
}
pub async fn configure(reg: &mut Registry, op: &Operation) -> Result<Value> {
    let app = owned(reg, op, "apps")?.clone();
    let admin = op.data["administrator"] == true;
    ensure!(
        app.data["egress_locked"] != true || admin,
        "An administrator controls this application's proxy"
    );
    let proxy_id = s(&op.data, "proxy_id");
    if !proxy_id.is_empty() {
        super::integrations_agent::connection(reg, &op.tenant, proxy_id, "proxy")?;
    }
    let environment = pod(
        &op.tenant,
        args(&[
            "inspect",
            "--format",
            "{{json .Config.Env}}",
            &container(&op.id),
        ]),
        None,
        20,
    )
    .await?;
    let values: Vec<String> = serde_json::from_str(environment.trim())?;
    let mut data = app.data.clone();
    let mut env = serde_json::Map::new();
    for value in values {
        if let Some((key, value)) = value.split_once('=') {
            if ![
                "HOME",
                "PATH",
                "LD_PRELOAD",
                "LD_LIBRARY_PATH",
                "CARGO_HOME",
                "RUSTUP_HOME",
            ]
            .contains(&key)
            {
                env.insert(key.into(), json!(value));
            }
        }
    }
    data["env"] = json!(env);
    data["egress_proxy_id"] = json!(proxy_id);
    if admin {
        data["egress_locked"] = json!(op.data["locked"] == true);
    }
    // Recreate only the container. The workspace, application port, and environment are retained.
    pod(
        &op.tenant,
        args(&["rm", "-f", &container(&op.id)]),
        None,
        40,
    )
    .await?;
    cleanup(&op.tenant, &op.id).await;
    let result = create_app(
        reg,
        &Operation {
            action: "egress_configure".into(),
            tenant: op.tenant.clone(),
            id: op.id.clone(),
            data,
        },
    )
    .await?;
    Ok(
        json!({"configured":true,"proxy_enabled":!proxy_id.is_empty(),"locked":reg.items[&op.id].data["egress_locked"],"state":result["state"],"protocols":"TCP and DNS through SOCKS5; other UDP and IPv6 blocked"}),
    )
}
