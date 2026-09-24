use super::*;

fn account(id: &str) -> String {
    format!("cgsftp_{}", &id[..16])
}
fn location(id: &str, item: &Item) -> Result<(String, String, String)> {
    let root = if s(&item.data, "transfer_root").is_empty() {
        format!("/srv/sftp/cgp_{id}")
    } else {
        s(&item.data, "transfer_root").into()
    };
    let directory = if s(&item.data, "transfer_directory").is_empty() {
        "/workspace".to_owned()
    } else {
        s(&item.data, "transfer_directory").into()
    };
    ensure!(
        root.strip_prefix("/srv/sftp/")
            .is_some_and(|v| !v.is_empty()
                && v.len() <= 64
                && v.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)))
            && directory.strip_prefix('/').is_some_and(identifier),
        "Invalid transfer location"
    );
    let config = if s(&item.data, "transfer_config").is_empty() {
        format!("/etc/cgpanel/sftp/{id}.conf")
    } else {
        s(&item.data, "transfer_config").into()
    };
    ensure!(
        !config.contains("..")
            && !config.contains(['\n', '\r'])
            && (config.starts_with("/etc/cgpanel/sftp/")
                || config.starts_with("/etc/ssh/sshd_config.d/"))
            && config.ends_with(".conf"),
        "Invalid transfer configuration path"
    );
    Ok((root, directory, config))
}
pub async fn status(reg: &Registry, op: &Operation) -> Result<Value> {
    let item = owned(reg, op, "apps")?;
    let name = if s(&item.data, "transfer_user").is_empty() {
        account(&op.id)
    } else {
        s(&item.data, "transfer_user").into()
    };
    Ok(
        json!({"username":name,"sftp":item.data["sftp_enabled"]==true,"ftps":item.data["ftps_enabled"]==true,
        "configured":item.data["transfer_user"].is_string(),"host":std::env::var("CGPANEL_PUBLIC_IP").unwrap_or_default(),
        "sftp_port":22,"ftps_port":21,"directory":location(&op.id,item)?.1,"allowed_ips":item.data["transfer_ips"].as_array().cloned().unwrap_or_default(),
        "note":"SFTP uses SSH. FTPS uses explicit TLS; unencrypted FTP is disabled. Both share this workspace account and password."}),
    )
}
async fn ftps_policy(reg: &Registry) -> Result<()> {
    let mut users = String::new();
    let mut access = String::from("# CGPanel managed FTPS access\n");
    tokio::fs::create_dir_all("/etc/cgpanel/ftps-users").await?;
    for (id, item) in reg
        .items
        .iter()
        .filter(|(_, i)| i.kind == "apps" && i.data["ftps_enabled"] == true)
    {
        let name = s(&item.data, "transfer_user");
        ensure!(identifier(name), "Invalid transfer username");
        users.push_str(&format!("{name}\n"));
        let ips = exact_ips(&item.data["transfer_ips"])?;
        access.push_str(&format!(
            "+|{name}|{}\n-|{name}|ALL\n",
            if ips.is_empty() {
                "ALL".into()
            } else {
                ips.join(" ")
            }
        ));
        let (root, directory, _) = location(id, item)?;
        atomic(
            &format!("/etc/cgpanel/ftps-users/{name}"),
            &format!("local_root={root}{directory}\n"),
            0o600,
        )
        .await?;
    }
    access.push_str("-|ALL|ALL\n");
    atomic("/etc/cgpanel/ftps-users.list", &users, 0o600).await?;
    atomic("/etc/cgpanel/ftps-access.conf", &access, 0o600).await?;
    // The firewall only exposes FTPS while at least one workspace enables it.
    let mut rules = String::from("flush set inet cgpanel ftps_ports\n");
    if !users.is_empty() {
        rules.push_str("add element inet cgpanel ftps_ports { 21, 60000-60100 }\n");
    }
    exec("nft", args(&["-f", "-"]), Some(rules), 30).await?;
    Ok(())
}
pub async fn configure(reg: &mut Registry, op: &Operation) -> Result<Value> {
    let mut item = owned(reg, op, "apps")?.clone();
    let ips = exact_ips(&op.data["allowed_ips"])?;
    ensure!(
        op.data["sftp"].is_boolean() && op.data["ftps"].is_boolean(),
        "Select service states"
    );
    ensure!(
        Path::new("/etc/cgpanel/ftps-users.list").exists(),
        "Install CGPanel transfer service support first"
    );
    let name = if s(&item.data, "transfer_user").is_empty() {
        account(&op.id)
    } else {
        s(&item.data, "transfer_user").into()
    };
    ensure!(identifier(&name), "Invalid transfer username");
    let (root, directory, file) = location(&op.id, &item)?;
    let mount = format!("{root}{directory}");
    tokio::fs::create_dir_all(&mount).await?;
    run("chown", &["root:root", "/srv/sftp"]).await?;
    run("chmod", &["0711", "/srv/sftp"]).await?;
    run("chown", &["root:root", &root]).await?;
    run("chmod", &["0755", &root]).await?;
    if run("mountpoint", &["-q", &mount]).await.is_err() {
        run("mount", &["--bind", &appdir(&op.tenant, &op.id), &mount]).await?;
    }
    let fstab = tokio::fs::read_to_string("/etc/fstab").await?;
    let workspace = appdir(&op.tenant, &op.id);
    let record = format!(
        "{workspace} {mount} none bind,nofail,x-systemd.requires-mounts-for={workspace} 0 0"
    );
    if !fstab
        .lines()
        .any(|l| l.split_whitespace().nth(1) == Some(&mount))
    {
        atomic(
            "/etc/fstab",
            &format!("{}\n{record}\n", fstab.trim_end()),
            0o644,
        )
        .await?;
    }
    let exists = run("id", &["-u", &name]).await.is_ok();
    if !exists {
        run(
            "useradd",
            &[
                "--no-create-home",
                "--home-dir",
                &mount,
                "--shell",
                "/usr/sbin/nologin",
                "--gid",
                &user(&op.tenant),
                &name,
            ],
        )
        .await?;
    }
    if exists {
        ensure!(
            run("id", &["-u", &name]).await?.trim().parse::<u32>()? >= 1000,
            "Refusing a privileged transfer account"
        );
        run("usermod", &["--home", &mount, &name]).await?;
    }
    permissions(op).await?;
    let mut config=format!("Match User {name}\n    ChrootDirectory {root}\n    ForceCommand internal-sftp -d {directory} -u 0007\n    PasswordAuthentication yes\n    DisableForwarding yes\n    AllowStreamLocalForwarding no\n    PermitTTY no\n    PermitTunnel no\n    PermitUserRC no\n");
    if op.data["sftp"] != true {
        config.push_str(&format!("    DenyUsers {name}\n"));
    } else if !ips.is_empty() {
        config.push_str(&format!(
            "    AllowUsers {}\n",
            ips.iter()
                .map(|ip| format!("{name}@{ip}"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    config.push_str("Match all\n");
    tokio::fs::create_dir_all("/etc/cgpanel/sftp").await?;

    let old = tokio::fs::read_to_string(&file).await.ok();
    atomic(&file, &config, 0o600).await?;
    // Include explicitly because some providers comment out the distribution's Include.
    let main = tokio::fs::read_to_string("/etc/ssh/sshd_config").await?;
    let include = format!("Include {file}");
    if !main.lines().any(|l| l.trim() == include) {
        atomic(
            "/etc/ssh/sshd_config",
            &format!("{}\nMatch all\n{include}\n", main.trim_end()),
            0o600,
        )
        .await?;
    }
    if let Err(e) = run("sshd", &["-t"]).await {
        if let Some(old) = old {
            atomic(&file, &old, 0o600).await?
        } else {
            let _ = tokio::fs::remove_file(&file).await;
        }
        return Err(e);
    }
    let password = if !exists || op.data["rotate"] == true {
        let password = random_secret();
        exec("chpasswd", vec![], Some(format!("{name}:{password}\n")), 20).await?;
        Some(password)
    } else {
        None
    };
    item.data["transfer_user"] = json!(name);
    item.data["transfer_ips"] = json!(ips);
    item.data["sftp_enabled"] = op.data["sftp"].clone();
    item.data["ftps_enabled"] = op.data["ftps"].clone();
    reg.items.insert(op.id.clone(), item);
    ftps_policy(reg).await?;
    run("systemctl", &["reload", "ssh"]).await?;
    if op.data["rotate"] == true || op.data["sftp"] != true || op.data["ftps"] != true {
        let _ = run("pkill", &["-KILL", "-u", &name]).await;
    }
    Ok(json!({"saved":true,"username":name,"password":password}))
}
pub async fn refresh(reg: &Registry) -> Result<()> {
    if Path::new("/etc/cgpanel/ftps-users.list").exists() {
        ftps_policy(reg).await?;
    }
    Ok(())
}
pub async fn disable(reg: &mut Registry, op: &Operation) -> Result<()> {
    if s(&owned(reg, op, "apps")?.data, "transfer_user").is_empty() {
        return Ok(());
    }
    let mut request = Operation {
        action: "services_configure".into(),
        tenant: op.tenant.clone(),
        id: op.id.clone(),
        data: json!({"sftp":false,"ftps":false,"allowed_ips":[]}),
    };
    request.data["rotate"] = json!(false);
    configure(reg, &request).await?;
    Ok(())
}

pub async fn permissions(op: &Operation) -> Result<()> {
    // Tenant UID performs permission changes, so a workspace symlink race cannot
    // make this step change root-owned host files.
    run(
        "runuser",
        &[
            "-u",
            &user(&op.tenant),
            "--",
            "find",
            &appdir(&op.tenant, &op.id),
            "-user",
            &user(&op.tenant),
            "-type",
            "d",
            "-exec",
            "chmod",
            "g+rwxs",
            "--",
            "{}",
            "+",
        ],
    )
    .await?;
    run(
        "runuser",
        &[
            "-u",
            &user(&op.tenant),
            "--",
            "find",
            &appdir(&op.tenant, &op.id),
            "-user",
            &user(&op.tenant),
            "-type",
            "f",
            "-exec",
            "chmod",
            "g+rw",
            "--",
            "{}",
            "+",
        ],
    )
    .await?;
    Ok(())
}
