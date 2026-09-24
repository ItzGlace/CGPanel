use super::*;
use base64::Engine;
use std::io::Read;
pub fn free_bytes() -> Result<u64> {
    let path = std::ffi::CString::new(ROOT)?;
    let mut info = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    ensure!(
        unsafe { libc::statvfs(path.as_ptr(), info.as_mut_ptr()) } == 0,
        "Cannot determine available backup storage"
    );
    let info = unsafe { info.assume_init() };
    Ok(info.f_bavail.saturating_mul(info.f_frsize))
}
pub async fn execute(reg: &mut Registry, op: &Operation) -> Result<Value> {
    let app = owned(reg, op, "apps")?.clone();
    let operation = s(&op.data, "operation");
    if operation == "begin" {
        let size = op.data["size"].as_u64().context("Missing archive size")?;
        ensure!(
            size > 0 && size <= 2 * 1024 * 1024 * 1024,
            "Upload .cgp files up to 2 GiB"
        );
        ensure!(
            size + 512 * 1024 * 1024 < free_bytes()?,
            "Insufficient backup storage; retain at least 512 MiB of server headroom"
        );
        // Abandoned uploads expire so a disconnected browser cannot permanently
        // consume the account's two upload slots.
        let expired = reg
            .items
            .iter()
            .filter(|(_, i)| {
                i.tenant == op.tenant
                    && i.kind == "backup_imports"
                    && i.data["created"].as_i64().unwrap_or(0)
                        < chrono::Utc::now().timestamp() - 86400
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in expired {
            tokio::fs::remove_file(format!("{ROOT}/backups/{id}.part"))
                .await
                .or_else(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        Ok(())
                    } else {
                        Err(e)
                    }
                })?;
            reg.items.remove(&id);
        }
        tokio::fs::create_dir_all(format!("{ROOT}/backups")).await?;
        let active = reg
            .items
            .values()
            .filter(|i| i.tenant == op.tenant && i.kind == "backup_imports")
            .count();
        ensure!(
            active < 2,
            "Finish or cancel the existing archive uploads first"
        );
        let upload = uuid::Uuid::new_v4().simple().to_string();
        let file = format!("{ROOT}/backups/{upload}.part");
        tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
            .await?;
        reg.items.insert(
            upload.clone(),
            Item {
                tenant: op.tenant.clone(),
                kind: "backup_imports".into(),
                data: json!({"app_id":op.id,"size":size,"created":chrono::Utc::now().timestamp()}),
            },
        );
        return Ok(json!({"upload":upload}));
    }
    let upload = s(&op.data, "upload");
    ensure!(
        identifier(upload) && upload.len() == 32,
        "Invalid upload ID"
    );
    let entry = reference(reg, upload, &op.tenant, "backup_imports")?.clone();
    ensure!(entry.data["app_id"] == op.id, "Wrong application");
    let file = format!("{ROOT}/backups/{upload}.part");
    match operation {
        "cancel" => {
            tokio::fs::remove_file(file).await?;
            reg.items.remove(upload);
            Ok(json!({"cancelled":true}))
        }
        "chunk" => {
            let data = base64::engine::general_purpose::STANDARD.decode(s(&op.data, "data"))?;
            ensure!(
                !data.is_empty() && data.len() <= 196608,
                "Invalid chunk size"
            );
            ensure!(
                free_bytes()? > 512 * 1024 * 1024 + data.len() as u64,
                "Backup storage is full; upload stopped"
            );
            let offset = tokio::fs::metadata(&file).await?.len();
            ensure!(
                op.data["offset"].as_u64() == Some(offset),
                "Upload offset mismatch"
            );
            ensure!(
                offset + data.len() as u64 <= entry.data["size"].as_u64().unwrap(),
                "Upload size exceeded"
            );
            let mut writer = tokio::fs::OpenOptions::new()
                .append(true)
                .open(file)
                .await?;
            writer.write_all(&data).await?;
            Ok(json!({"next":offset+data.len() as u64}))
        }
        "finish" => {
            let size = tokio::fs::metadata(&file).await?.len();
            ensure!(
                Some(size) == entry.data["size"].as_u64(),
                "Upload is incomplete"
            );
            let source = file.clone();
            let manifest = tokio::task::spawn_blocking(move || -> Result<Value> {
                let mut archive = zip::ZipArchive::new(std::fs::File::open(source)?)?;
                ensure!(archive.len() <= 24, "Too many archive entries");
                let mut text = String::new();
                archive
                    .by_name("manifest.json")?
                    .take(1_048_577)
                    .read_to_string(&mut text)?;
                ensure!(text.len() <= 1_048_576, "Manifest too large");
                Ok(serde_json::from_str(&text)?)
            })
            .await??;
            ensure!(
                manifest["format"] == "CGPanel" && manifest["version"] == 1,
                "Unsupported .cgp format"
            );
            ensure!(manifest["owner"]==op.tenant&&manifest["app_id"]==op.id,"Archive belongs to another account or application; administrator-assisted migration is required");
            let records = manifest["resources"]
                .as_array()
                .context("Missing resource manifest")?;
            for r in records.iter().filter(|r| r["kind"] == "databases") {
                reference(reg, s(r, "id"), &op.tenant, "databases")?;
            }
            tokio::fs::rename(file, format!("{ROOT}/backups/{upload}.cgp")).await?;
            let data = json!({"id":upload,"app_id":op.id,"name":s(&app.data,"name"),"created":chrono::Utc::now().timestamp(),"bytes":size,"database_count":records.iter().filter(|r|r["kind"]=="databases").count(),"format":"cgp-v1","delivery":[],"imported":true});
            reg.items.insert(
                upload.into(),
                Item {
                    tenant: op.tenant.clone(),
                    kind: "full_backups".into(),
                    data: data.clone(),
                },
            );
            Ok(data)
        }
        _ => bail!("Choose begin, chunk, finish or cancel"),
    }
}
