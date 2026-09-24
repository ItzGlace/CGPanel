use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::PathBuf,
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

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
    command.args(arguments).current_dir("/").kill_on_drop(true);
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
    command.stderr(Stdio::null());
    if let Some((key, value)) = env {
        command.env(key, value);
    }
    let mut child = command.spawn()?;
    let pid = child.id();
    match tokio::time::timeout(Duration::from_secs(600), child.wait()).await {
        Ok(status) => ensure!(status?.success(), "Archive or database operation failed"),
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
    if was_running {
        pod(
            &op.tenant,
            args(&["stop", "--time", "15", &container(&op.id)]),
            None,
            40,
        )
        .await?;
    }
    let result = restore_data(reg, op, records, &staging).await;
    if was_running {
        let _ = pod(&op.tenant, args(&["start", &container(&op.id)]), None, 40).await;
    }
    let _ = tokio::fs::remove_dir_all(&staging).await;
    result?;
    Ok(
        json!({"restored":true,"scope":"workspace and selected databases","domain_configuration":"included in archive manifest; existing domain assignments retained"}),
    )
}
async fn restore_data(
    reg: &Registry,
    op: &Operation,
    records: &[Value],
    staging: &Path,
) -> Result<()> {
    // Extraction is performed under the tenant's Linux UID, never under host root.
    stream(
        "runuser",
        args(&[
            "-u",
            &user(&op.tenant),
            "--",
            "tar",
            "-xzf",
            "-",
            "--no-same-owner",
            "--no-same-permissions",
            "-C",
            &appdir(&op.tenant, &op.id),
        ]),
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
            stream(
                "runuser",
                args(&[
                    "-u",
                    &user(&op.tenant),
                    "--",
                    "mariadb",
                    "--binary-mode",
                    "--local-infile=0",
                    "--user",
                    &name,
                    &name,
                ]),
                None,
                Some(&input),
                Some(("MYSQL_PWD", password)),
            )
            .await?;
        } else {
            stream(
                "runuser",
                args(&[
                    "-u",
                    &user(&op.tenant),
                    "--",
                    "psql",
                    "-X",
                    "--set",
                    "ON_ERROR_STOP=1",
                    "--host",
                    "127.0.0.1",
                    "--username",
                    &name,
                    "--dbname",
                    &name,
                ]),
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
