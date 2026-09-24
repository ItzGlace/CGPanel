use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::PathBuf,
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

struct RestoreStaging(PathBuf);
impl Drop for RestoreStaging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn checksum(path: &Path) -> Result<(u64, String)> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut count = 0;
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        hash.update(&buf[..n]);
    }
    Ok((count, format!("{:x}", hash.finalize())))
}
async fn stream(
    program: &str,
    arguments: Vec<String>,
    output: Option<&Path>,
    input: Option<&Path>,
    env: Option<(&str, &str)>,
) -> Result<()> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .current_dir("/")
        .kill_on_drop(true)
        .env_clear()
        .env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        )
        .env("LC_ALL", "C");
    command.as_std_mut().process_group(0);
    if output.is_some() {
        // Bound each dump even if the source grows continuously while it is being read.
        unsafe {
            command.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: 20 * 1024 * 1024 * 1024,
                    rlim_max: 20 * 1024 * 1024 * 1024,
                };
                if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    command.stdout(if let Some(path) = output {
        Stdio::from(
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?,
        )
    } else {
        Stdio::null()
    });
    command.stdin(if let Some(path) = input {
        Stdio::from(std::fs::File::open(path)?)
    } else {
        Stdio::null()
    });
    command.stderr(Stdio::piped());
    if let Some((key, value)) = env {
        command.env(key, value);
    }
    let mut child = command.spawn()?;
    let pid = child.id();
    let mut stderr = child.stderr.take().context("Missing diagnostic stream")?;
    let diagnostics = tokio::spawn(async move {
        let mut saved = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let n = stderr.read(&mut buffer).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            let keep = n.min(4096usize.saturating_sub(saved.len()));
            saved.extend_from_slice(&buffer[..keep]);
        }
        String::from_utf8_lossy(&saved).into_owned()
    });
    match tokio::time::timeout(Duration::from_secs(600), child.wait()).await {
        Ok(status) => {
            let mut diagnostic = diagnostics.await.unwrap_or_default();
            if let Some((_, secret)) = env {
                if !secret.is_empty() {
                    diagnostic = diagnostic.replace(secret, "[redacted]");
                }
            }
            ensure!(
                status?.success(),
                "Archive or database operation failed: {}",
                diagnostic.trim()
            );
        }
        Err(_) => {
            if let Some(pid) = pid {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
            let _ = child.wait().await;
            bail!("Backup operation exceeded 10 minutes");
        }
    }
    Ok(())
}
pub async fn create(reg: &mut Registry, op: &Operation) -> Result<Value> {
    let app = owned(reg, op, "apps")?.clone();
    let backup_id = s(&op.data, "job_id");
    ensure!(
        identifier(backup_id) && backup_id.len() == 32,
        "Invalid backup job ID"
    );
    if let Some(existing) = reg.items.get(backup_id) {
        ensure!(
            existing.kind == "full_backups" && existing.tenant == op.tenant,
            "Archive ID collision"
        );
        return Ok(existing.data.clone());
    }
    let selected = op.data["database_ids"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    ensure!(selected.len() <= 20, "Select at most 20 databases");
    let mut database_ids = BTreeSet::new();
    for value in selected {
        let id = value.as_str().context("Invalid database ID")?;
        reference(reg, id, &op.tenant, "databases")?;
        database_ids.insert(id.to_string());
    }
    let destinations = op.data["destinations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    ensure!(destinations.len() <= 5, "Select at most five destinations");
    for value in &destinations {
        let i = reference(
            reg,
            value.as_str().context("Invalid destination")?,
            &op.tenant,
            "integrations",
        )?;
        ensure!(
            ["telegram", "s3", "ssh"].contains(&s(&i.data, "type")),
            "Unsupported backup destination"
        );
    }
    let staging = PathBuf::from(format!("{ROOT}/staging/{backup_id}"));
    tokio::fs::create_dir_all(staging.join("files")).await?;
    tokio::fs::create_dir_all(staging.join("sql")).await?;
    let was_running = pod(
        &op.tenant,
        args(&[
            "inspect",
            "--format",
            "{{.State.Running}}",
            &container(&op.id),
        ]),
        None,
        20,
    )
    .await?
    .trim()
        == "true";
    let pause = op.data["quiesce"] != false && was_running;
    if pause {
        pod(
            &op.tenant,
            args(&["stop", "--time", "15", &container(&op.id)]),
            None,
            40,
        )
        .await?;
    }
    let result = build(reg, op, &app, &database_ids, &staging).await;
    let resumed = if pause {
        pod(&op.tenant, args(&["start", &container(&op.id)]), None, 40)
            .await
            .map(|_| ())
    } else {
        Ok(())
    };
    if let Err(error) = result {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err(error);
    }
    resumed.context("Backup created but application restart failed")?;
    let final_file = PathBuf::from(format!("{ROOT}/backups/{backup_id}.cgp"));
    tokio::fs::rename(staging.join("archive.cgp"), &final_file).await?;
    let bytes = tokio::fs::metadata(&final_file).await?.len();
    let data = json!({"id":backup_id,"app_id":op.id,"plan_id":s(&op.data,"plan_id"),"name":s(&app.data,"name"),"created":chrono::Utc::now().timestamp(),"bytes":bytes,"database_count":database_ids.len(),"format":"cgp-v1","delivery":[]});
    reg.items.insert(
        backup_id.into(),
        Item {
            tenant: op.tenant.clone(),
            kind: "full_backups".into(),
            data,
        },
    );
    save(reg).await?;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    // Persist the local copy before any external transfer. Failed deliveries remain recoverable.
    let mut delivery = Vec::new();
    let mut failed = false;
    for destination in destinations {
        let id = destination.as_str().unwrap();
        let result =
            super::transfers_agent::upload(reg, &op.tenant, id, &final_file, backup_id).await;
        failed |= result.is_err();
        delivery.push(json!({"destination":id,"success":result.is_ok(),"error":result.err().map(|e|e.to_string()).unwrap_or_default()}));
    }
    let item = reg.items.get_mut(backup_id).unwrap();
    item.data["delivery"] = json!(delivery);
    let result = item.data.clone();
    save(reg).await?;
    ensure!(!failed,"Local .cgp backup succeeded, but a remote delivery failed. Inspect Backups and retry delivery.");
    let plan = s(&op.data, "plan_id");
    if !plan.is_empty() {
        let keep = op.data["retention"].as_u64().unwrap_or(7).clamp(1, 30) as usize;
        let mut old: Vec<_> = reg
            .items
            .iter()
            .filter(|(_, i)| {
                i.tenant == op.tenant && i.kind == "full_backups" && s(&i.data, "plan_id") == plan
            })
            .map(|(id, i)| (id.clone(), i.data["created"].as_i64().unwrap_or(0)))
            .collect();
        old.sort_by_key(|(_, created)| std::cmp::Reverse(*created));
        for (id, _) in old.into_iter().skip(keep) {
            if let Err(error) = tokio::fs::remove_file(format!("{ROOT}/backups/{id}.cgp")).await {
                if error.kind() != std::io::ErrorKind::NotFound {
                    continue;
                }
            }
            let _ = tokio::fs::remove_file(format!("/var/lib/cgpanel-exports/{id}.cgp")).await;
            reg.items.remove(&id);
        }
    }
    Ok(result)
}
async fn build(
    reg: &Registry,
    op: &Operation,
    app: &Item,
    databases: &BTreeSet<String>,
    staging: &Path,
) -> Result<()> {
    stream(
        "runuser",
        args(&[
            "-u",
            &user(&op.tenant),
            "--",
            "tar",
            "-czf",
            "-",
            "--one-file-system",
            "-C",
            &appdir(&op.tenant, &op.id),
            ".",
        ]),
        Some(&staging.join("files/workspace.tar.gz")),
        None,
        None,
    )
    .await?;
    let env = pod(
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
    let mut app_data = app.data.clone();
    app_data["environment"] =
        serde_json::from_str::<Value>(env.trim()).context("Cannot read application environment")?;
    let mut records = vec![json!({"id":op.id,"kind":"apps","data":app_data})];
    let domain_ids: BTreeSet<_> = reg
        .items
        .iter()
        .filter(|(_, i)| {
            i.tenant == op.tenant && i.kind == "domains" && s(&i.data, "app_id") == op.id
        })
        .map(|(id, _)| id.clone())
        .collect();
    for (id, item) in &reg.items {
        if item.tenant != op.tenant {
            continue;
        }
        if domain_ids.contains(id)
            || (item.kind == "dns" && domain_ids.contains(s(&item.data, "domain_id")))
            || (item.kind == "schedules" && s(&item.data, "app_id") == op.id)
        {
            records.push(json!({"id":id,"kind":item.kind,"data":item.data}));
        }
    }
    let mut paths = vec!["files/workspace.tar.gz".to_string()];
    for id in databases {
        let item = reference(reg, id, &op.tenant, "databases")?;
        let name = db_name(id);
        let path = format!("sql/{id}.sql");
        if s(&item.data, "engine") == "mysql" {
            stream(
                "mariadb-dump",
                args(&[
                    "--single-transaction",
                    "--quick",
                    "--routines",
                    "--events",
                    "--triggers",
                    "--hex-blob",
                    "--skip-comments",
                    &name,
                ]),
                Some(&staging.join(&path)),
                None,
                None,
            )
            .await?;
        } else {
            stream(
                "runuser",
                args(&[
                    "-u",
                    "postgres",
                    "--",
                    "pg_dump",
                    "--clean",
                    "--if-exists",
                    "--no-owner",
                    "--no-privileges",
                    "--format=plain",
                    &name,
                ]),
                Some(&staging.join(&path)),
                None,
                None,
            )
            .await?;
        }
        records.push(json!({"id":id,"kind":"databases","data":item.data}));
        paths.push(path);
        let mut total = 0;
        for path in &paths {
            total += tokio::fs::metadata(staging.join(path)).await?.len();
        }
        ensure!(
            total <= 20 * 1024 * 1024 * 1024,
            "Archive input exceeds the 20 GiB safety limit"
        );
    }
    let staging = staging.to_path_buf();
    let tenant = op.tenant.clone();
    let app_id = op.id.clone();
    tokio::task::spawn_blocking(move||->Result<()>{
        let mut files=Vec::new();let mut total=0u64;
        for path in &paths{let (bytes,hash)=checksum(&staging.join(path))?;total+=bytes;files.push(json!({"path":path,"bytes":bytes,"sha256":hash}));}
        ensure!(total<=20*1024*1024*1024,"Archive input exceeds the 20 GiB safety limit");
        let manifest=json!({"format":"CGPanel","version":1,"panel_version":env!("CARGO_PKG_VERSION"),"owner":tenant,"app_id":app_id,"created":chrono::Utc::now().to_rfc3339(),"resources":records,"files":files,"contains_secrets":true,"consistency":"Application stopped unless explicitly disabled; each SQL dump has its own consistency boundary."});
        let output=std::fs::OpenOptions::new().create_new(true).write(true).open(staging.join("archive.cgp"))?;
        let mut zip=ZipWriter::new(output);let options=SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).unix_permissions(0o600);
        zip.start_file("manifest.json",options)?;zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        for path in paths{zip.start_file(&path,options)?;std::io::copy(&mut std::fs::File::open(staging.join(path))?,&mut zip)?;}
        zip.finish()?.sync_all()?;Ok(())
    }).await??;
    Ok(())
}
pub async fn restore(reg: &mut Registry, op: &Operation) -> Result<Value> {
    owned(reg, op, "apps")?;
    ensure!(
        op.data["confirm"] == "RESTORE",
        "Restore requires explicit RESTORE confirmation"
    );
    let backup_id = s(&op.data, "backup_id");
    let backup = reference(reg, backup_id, &op.tenant, "full_backups")?;
    ensure!(
        s(&backup.data, "app_id") == op.id,
        "Archive belongs to a different application"
    );
    let staging = PathBuf::from(format!("{ROOT}/restore/{}", s(&op.data, "job_id")));
    ensure!(
        identifier(s(&op.data, "job_id")) && s(&op.data, "job_id").len() == 32,
        "Invalid restore job ID"
    );
    tokio::fs::create_dir_all(&staging).await?;
    let _cleanup = RestoreStaging(staging.clone());
    let archive = PathBuf::from(format!("{ROOT}/backups/{backup_id}.cgp"));
    let folder = staging.clone();
    let manifest = tokio::task::spawn_blocking(move || -> Result<Value> {
        let mut zip = ZipArchive::new(std::fs::File::open(archive)?)?;
        ensure!(zip.len() <= 24, "Too many archive members");
        let mut text = String::new();
        zip.by_name("manifest.json")?
            .take(1024 * 1024)
            .read_to_string(&mut text)?;
        let manifest: Value = serde_json::from_str(&text)?;
        ensure!(
            manifest["format"] == "CGPanel" && manifest["version"] == 1,
            "Unsupported archive format"
        );
        let files = manifest["files"]
            .as_array()
            .context("Missing file manifest")?;
        let total = files.iter().try_fold(0u64, |sum, file| {
            sum.checked_add(file["bytes"].as_u64().unwrap_or(u64::MAX))
                .context("Invalid archive sizes")
        })?;
        ensure!(
            total <= 20 * 1024 * 1024 * 1024
                && total.saturating_add(512 * 1024 * 1024) < backup_import::free_bytes()?,
            "Insufficient space to stage this archive safely"
        );
        for entry in files {
            let path = s(entry, "path");
            let valid = path == "files/workspace.tar.gz"
                || (path.starts_with("sql/")
                    && path.ends_with(".sql")
                    && identifier(&path[4..path.len() - 4])
                    && path.len() == 40);
            ensure!(valid, "Invalid archive member path");
            let bytes = entry["bytes"].as_u64().context("Invalid member size")?;
            ensure!(bytes <= 20 * 1024 * 1024 * 1024, "Archive member too large");
            let dest = folder.join(path);
            std::fs::create_dir_all(dest.parent().unwrap())?;
            let mut output = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&dest)?;
            let copied = std::io::copy(&mut zip.by_name(path)?.take(bytes + 1), &mut output)?;
            ensure!(
                copied == bytes && checksum(&dest)?.1 == s(entry, "sha256"),
                "Archive checksum mismatch"
            );
        }
        Ok(manifest)
    })
    .await??;
    ensure!(
        manifest["owner"] == op.tenant && manifest["app_id"] == op.id,
        "Archive ownership mismatch"
    );
    let records = manifest["resources"]
        .as_array()
        .context("Missing resource manifest")?;
    for record in records.iter().filter(|r| r["kind"] == "databases") {
        let item = reference(reg, s(record, "id"), &op.tenant, "databases")?;
        ensure!(
            item.data["engine"] == record["data"]["engine"],
            "Database engine changed; manual migration required"
        );
    }
    let recovery = uuid::Uuid::new_v4().simple().to_string();
    create(reg,&Operation{action:"full_backup_create".into(),tenant:op.tenant.clone(),id:op.id.clone(),data:json!({"job_id":recovery,"database_ids":records.iter().filter(|r|r["kind"]=="databases").map(|r|r["id"].clone()).collect::<Vec<_>>(),"quiesce":true})}).await.context("Could not create a pre-restore recovery backup; restore cancelled")?;
    let was_running = pod(
        &op.tenant,
        args(&[
            "inspect",
            "--format",
            "{{.State.Running}}",
            &container(&op.id),
        ]),
        None,
        20,
    )
    .await?
    .trim()
        == "true";
    let app = owned(reg, op, "apps")?.clone();
    let transfer = !s(&app.data, "transfer_user").is_empty();
    let ide_name = format!("cgp_ide_{}", op.id);
    let ide_running = if app.data["ide_enabled"] == true {
        pod(
            &op.tenant,
            args(&["inspect", "--format", "{{.State.Running}}", &ide_name]),
            None,
            20,
        )
        .await?
        .trim()
            == "true"
    } else {
        false
    };
    let mut result = async {
        if transfer {
            services_agent::disable(reg, op).await?;
        }
        if ide_running {
            pod(
                &op.tenant,
                args(&["stop", "--time", "10", &ide_name]),
                None,
                30,
            )
            .await?;
        }
        if was_running {
            pod(
                &op.tenant,
                args(&["stop", "--time", "15", &container(&op.id)]),
                None,
                40,
            )
            .await?;
        }
        restore_data(reg, op, records, &staging).await?;
        if transfer {
            services_agent::permissions(op).await?;
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    // Restore writers even when extraction, database loading or permission repair fails.
    if was_running {
        let resumed = pod(&op.tenant, args(&["start", &container(&op.id)]), None, 40).await;
        if result.is_ok() {
            result = resumed.map(|_| ());
        }
    }
    if ide_running {
        let resumed = pod(&op.tenant, args(&["start", &ide_name]), None, 40).await;
        if result.is_ok() {
            result = resumed.map(|_| ());
        }
    }
    if transfer {
        let restored=services_agent::configure(reg,&Operation{action:"services_configure".into(),tenant:op.tenant.clone(),id:op.id.clone(),data:json!({"sftp":app.data["sftp_enabled"]==true,"ftps":app.data["ftps_enabled"]==true,"rotate":false,"allowed_ips":app.data["transfer_ips"].as_array().cloned().unwrap_or_default()})}).await;
        if result.is_ok() {
            result = restored.map(|_| ());
        }
    }
    let _ = tokio::fs::remove_dir_all(&staging).await;
    result.map_err(|error| {
        anyhow::anyhow!("Restore failed: {error:#}. Pre-restore recovery archive: {recovery}")
    })?;
    Ok(
        json!({"restored":true,"recovery_backup":recovery,"scope":"workspace and selected databases","domain_configuration":"included in archive manifest; existing domain assignments retained"}),
    )
}
async fn restore_data(
    reg: &Registry,
    op: &Operation,
    records: &[Value],
    staging: &Path,
) -> Result<()> {
    // Root constructs the mount namespace, then drops to the tenant UID. No host
    // network or other workspaces are visible, including to SQL client metacommands.
    let uid = uid(&op.tenant).await?;
    let gid = run("id", &["-g", &user(&op.tenant)]).await?;
    let sandbox = || {
        args(&[
            "--unshare-pid",
            "--unshare-net",
            "--unshare-ipc",
            "--unshare-uts",
            "--die-with-parent",
            "--new-session",
            "--cap-add",
            "CAP_SETUID",
            "--cap-add",
            "CAP_SETGID",
            "--cap-add",
            "CAP_SETPCAP",
            "--ro-bind",
            "/usr",
            "/usr",
            "--ro-bind",
            "/lib",
            "/lib",
            "--ro-bind",
            "/lib64",
            "/lib64",
            "--symlink",
            "usr/bin",
            "/bin",
            "--ro-bind",
            "/etc/passwd",
            "/etc/passwd",
            "--ro-bind",
            "/etc/group",
            "/etc/group",
            "--ro-bind",
            "/run/mysqld",
            "/run/mysqld",
            "--ro-bind",
            "/run/postgresql",
            "/run/postgresql",
            "--chmod",
            "0755",
            "/etc",
            "--chmod",
            "0755",
            "/run",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
            "--chmod",
            "1777",
            "/tmp",
            "--bind",
            &appdir(&op.tenant, &op.id),
            "/workspace",
            "--chdir",
            "/workspace",
            "setpriv",
            "--reuid",
            &uid,
            "--regid",
            gid.trim(),
            "--clear-groups",
            "--no-new-privs",
            "--bounding-set=-all",
            "--inh-caps=-all",
            "--ambient-caps=-all",
        ])
    };
    let mut verify = sandbox();
    verify.extend(args(&["tar", "-tzf", "-"]));
    stream(
        "bwrap",
        verify,
        None,
        Some(&staging.join("files/workspace.tar.gz")),
        None,
    )
    .await
    .context("Invalid workspace archive; existing files retained")?;
    // Recreate archived entries with the tenant's identity, including files originally
    // uploaded through SFTP. Only the bound workspace is writable in this namespace.
    let mut clear = sandbox();
    clear.extend(args(&[
        "find",
        "/workspace",
        "-xdev",
        "-mindepth",
        "1",
        "-delete",
    ]));
    stream("bwrap", clear, None, None, None)
        .await
        .context("Could not clear workspace for restore")?;
    let mut extract = sandbox();
    extract.extend(args(&[
        "tar",
        "-xzf",
        "-",
        "--no-same-owner",
        "--no-same-permissions",
        "-C",
        "/workspace",
    ]));
    stream(
        "bwrap",
        extract,
        None,
        Some(&staging.join("files/workspace.tar.gz")),
        None,
    )
    .await?;
    for record in records.iter().filter(|r| r["kind"] == "databases") {
        let id = s(record, "id");
        let item = reference(reg, id, &op.tenant, "databases")?;
        let name = db_name(id);
        let password = s(&item.data, "password");
        let input = staging.join(format!("sql/{id}.sql"));
        if s(&item.data, "engine") == "mysql" {
            let mut command = sandbox();
            command.extend(args(&[
                "mariadb",
                "--binary-mode",
                "--local-infile=0",
                "--socket=/run/mysqld/mysqld.sock",
                "--user",
                &name,
                &name,
            ]));
            stream(
                "bwrap",
                command,
                None,
                Some(&input),
                Some(("MYSQL_PWD", password)),
            )
            .await?;
        } else {
            let mut command = sandbox();
            command.extend(args(&[
                "/usr/lib/postgresql/16/bin/psql",
                "-X",
                "--set",
                "ON_ERROR_STOP=1",
                "--host",
                "/run/postgresql",
                "--username",
                &name,
                "--dbname",
                &name,
            ]));
            stream(
                "bwrap",
                command,
                None,
                Some(&input),
                Some(("PGPASSWORD", password)),
            )
            .await?;
        }
    }
    Ok(())
}
pub async fn execute(reg: &mut Registry, op: &Operation) -> Result<Value> {
    match op.action.as_str() {
        "full_backup_create" => create(reg, op).await,
        "full_backup_restore" => restore(reg, op).await,
        "full_backup_list" => Ok(json!(reg
            .items
            .values()
            .filter(|i| i.tenant == op.tenant && i.kind == "full_backups")
            .map(|i| i.data.clone())
            .collect::<Vec<_>>())),
        "full_backup_delete" => {
            owned(reg, op, "full_backups")?;
            for file in [
                format!("{ROOT}/backups/{}.cgp", op.id),
                format!("/var/lib/cgpanel-exports/{}.cgp", op.id),
            ] {
                if let Err(error) = tokio::fs::remove_file(file).await {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        return Err(error.into());
                    }
                }
            }
            reg.items.remove(&op.id);
            Ok(json!({"deleted":true}))
        }
        "full_backup_export" => {
            owned(reg, op, "full_backups")?;
            let directory = "/var/lib/cgpanel-exports";
            tokio::fs::create_dir_all(directory).await?;
            run("chown", &["root:cgpanel", directory]).await?;
            tokio::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o750)).await?;
            let target = format!("{directory}/{}.cgp", op.id);
            tokio::fs::copy(format!("{ROOT}/backups/{}.cgp", op.id), &target).await?;
            run("chown", &["root:cgpanel", &target]).await?;
            tokio::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).await?;
            Ok(json!({"ready":true}))
        }
        "full_backup_deliver" => {
            owned(reg, op, "full_backups")?;
            let destination = s(&op.data, "destination");
            let result = super::transfers_agent::upload(
                reg,
                &op.tenant,
                destination,
                &PathBuf::from(format!("{ROOT}/backups/{}.cgp", op.id)),
                &op.id,
            )
            .await;
            let item = reg.items.get_mut(&op.id).unwrap();
            let mut deliveries = item.data["delivery"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            deliveries.retain(|v| s(v, "destination") != destination);
            deliveries.push(json!({"destination":destination,"success":result.is_ok(),"error":if result.is_ok(){""}else{"Delivery failed"}}));
            item.data["delivery"] = json!(deliveries);
            result
        }
        _ => bail!("Unknown full backup operation"),
    }
}
