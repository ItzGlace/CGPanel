use super::*;
fn budget(reg: &Registry, tenant: &str) -> Value {
    reg.items
        .get(tenant)
        .filter(|i| i.kind == "tenant_limits")
        .map(|i| i.data.clone())
        .unwrap_or(json!({"memory_mb":2048,"cpu_millis":2000,"disk_mb":4096}))
}
fn allocation(data: &Value) -> Value {
    json!({"memory_mb":data["memory_mb"].as_u64().unwrap_or(512),"cpu_millis":data["cpu_millis"].as_u64().unwrap_or(1000),"disk_mb":data["disk_mb"].as_u64().unwrap_or(1024)})
}
fn volumes(tenant: &str) -> BTreeMap<String, u64> {
    let mut result = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir("/var/lib/cgpanel-volumes") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Ok(data) = std::fs::read_to_string(path) {
                    if let Ok(v) = serde_json::from_str::<Value>(&data) {
                        if s(&v, "tenant") == tenant {
                            result.insert(s(&v, "app").into(), v["size_mb"].as_u64().unwrap_or(0));
                        }
                    }
                }
            }
        }
    }
    result
}
fn used(reg: &Registry, tenant: &str, exclude: &str) -> Value {
    let mut memory = 0u64;
    let mut cpu = 0u64;
    let mut disk = 0u64;
    let mut volumes = volumes(tenant);
    volumes.remove(exclude);
    for (id, item) in reg
        .items
        .iter()
        .filter(|(id, i)| i.tenant == tenant && i.kind == "apps" && id.as_str() != exclude)
    {
        let a = allocation(&item.data);
        memory += a["memory_mb"].as_u64().unwrap();
        cpu += a["cpu_millis"].as_u64().unwrap();
        disk += volumes.remove(id).unwrap_or(a["disk_mb"].as_u64().unwrap());
        if item.data["ide_enabled"] == true {
            memory += 384;
            cpu += 750;
        }
    }
    disk += volumes.values().sum::<u64>();
    json!({"memory_mb":memory,"cpu_millis":cpu,"disk_mb":disk})
}
fn validate(v: &Value) -> Result<()> {
    for (key, min, max) in [
        ("memory_mb", 64, 1073741824),
        ("cpu_millis", 50, 1073741824),
        ("disk_mb", 128, 1073741824),
    ] {
        ensure!(
            v[key].as_u64().is_some_and(|n| (min..=max).contains(&n)),
            "Invalid {key} allocation"
        );
    }
    Ok(())
}
pub fn check(reg: &Registry, op: &Operation, extra_ide: bool) -> Result<Value> {
    for key in ["memory_mb", "cpu_millis", "disk_mb"] {
        if op.data.get(key).is_some() {
            ensure!(op.data[key].as_u64().is_some(), "Invalid {key} allocation");
        }
    }
    let value = allocation(&op.data);
    validate(&value)?;
    let limits = budget(reg, &op.tenant);
    let old_disk = reg
        .items
        .get(&op.id)
        .map(|i| allocation(&i.data)["disk_mb"].as_u64().unwrap_or(0));
    if old_disk.is_none_or(|old| value["disk_mb"].as_u64().unwrap_or(0) > old) {
        reserve(reg, &op.tenant, limits["disk_mb"].as_u64().unwrap_or(4096))?;
    }
    let usage = used(reg, &op.tenant, &op.id);
    let ide = extra_ide
        || reg
            .items
            .get(&op.id)
            .is_some_and(|i| i.data["ide_enabled"] == true);
    for key in ["memory_mb", "cpu_millis", "disk_mb"] {
        let extra = if ide {
            match key {
                "memory_mb" => 384,
                "cpu_millis" => 750,
                _ => 0,
            }
        } else {
            0
        };
        ensure!(
            usage[key].as_u64().unwrap() + value[key].as_u64().unwrap() + extra
                <= limits[key].as_u64().unwrap_or(0),
            "Account {key} budget exceeded; lower the allocation or ask your administrator"
        );
    }
    Ok(value)
}
pub fn status(reg: &Registry, op: &Operation) -> Result<Value> {
    let total = budget(reg, &op.tenant);
    let used = used(reg, &op.tenant, "");
    let mut free = json!({});
    for key in ["memory_mb", "cpu_millis", "disk_mb"] {
        free[key] = json!(total[key]
            .as_u64()
            .unwrap()
            .saturating_sub(used[key].as_u64().unwrap()));
    }
    let volumes = volumes(&op.tenant);
    let storage=disk_capacity().ok().map(|(total,free)|json!({"total_mb":total,"free_mb":free,"reserve_mb":(total/10).max(2048),"pressure":free<(total/10).max(2048)}));
    Ok(
        json!({"server_storage":storage,"total":total,"allocated":used,"free":free,"applications":reg.items.iter().filter(|(_,i)|i.tenant==op.tenant&&i.kind=="apps").map(|(id,i)|json!({"id":id,"name":i.data["name"],"limits":allocation(&i.data),"disk_enforced":volumes.contains_key(id)})).collect::<Vec<_>>(),"disk_scope":"Workspace volumes, including retained deleted workspaces. Database storage and container images are server-managed separately. Filesystem metadata reduces usable space."}),
    )
}
fn disk_capacity() -> Result<(u64, u64)> {
    let path = std::ffi::CString::new("/var/lib/cgpanel-volumes")?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    ensure!(
        unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } == 0,
        "Cannot inspect workspace storage capacity"
    );
    let stats = unsafe { stats.assume_init() };
    Ok((
        stats.f_blocks * stats.f_frsize / 1048576,
        stats.f_bavail * stats.f_frsize / 1048576,
    ))
}
fn headroom(total: u64, free: u64, promises: u64) -> Result<()> {
    let reserve = (total / 10).max(2048);
    ensure!(promises.saturating_add(reserve)<=free,"Server disk capacity exhausted: preserve at least {reserve} MiB for system, database, logs and recovery; reduce account budgets");
    Ok(())
}
pub fn reserve(reg: &Registry, tenant: &str, requested: u64) -> Result<()> {
    let (total, free) = disk_capacity()?;
    // Volume images already reserve physical blocks. Budget promises reserve the remainder.
    let mut tenants: std::collections::BTreeSet<String> = reg
        .items
        .values()
        .filter(|i| identifier(&i.tenant) && i.tenant.len() == 32)
        .map(|i| i.tenant.clone())
        .collect();
    tenants.insert(tenant.into());
    let mut promises = 0u64;
    for id in tenants {
        let limit = if id == tenant {
            requested
        } else {
            budget(reg, &id)["disk_mb"].as_u64().unwrap_or(4096)
        };
        promises =
            promises.saturating_add(limit.saturating_sub(volumes(&id).values().sum::<u64>()));
    }
    headroom(total, free, promises)
}
pub async fn set_budget(reg: &mut Registry, op: &Operation) -> Result<Value> {
    validate(&op.data)?;
    if op.data["disk_mb"].as_u64().unwrap()
        > budget(reg, &op.tenant)["disk_mb"].as_u64().unwrap_or(0)
    {
        reserve(reg, &op.tenant, op.data["disk_mb"].as_u64().unwrap())?;
    }
    let used = used(reg, &op.tenant, "");
    for key in ["memory_mb", "cpu_millis", "disk_mb"] {
        ensure!(
            op.data[key].as_u64().unwrap() >= used[key].as_u64().unwrap(),
            "Budget cannot be below allocated {key}"
        );
    }
    reg.items.insert(
        op.tenant.clone(),
        Item {
            tenant: op.tenant.clone(),
            kind: "tenant_limits".into(),
            data: allocation(&op.data),
        },
    );
    status(reg, op)
}
pub async fn volume(op: &Operation, disk: u64) -> Result<()> {
    exec(
        "python3",
        args(&[
            "/usr/local/lib/cgpanel/workspace-volume.py",
            &op.tenant,
            &op.id,
            &disk.to_string(),
        ]),
        None,
        360,
    )
    .await?;
    Ok(())
}
pub async fn configure(reg: &mut Registry, op: &Operation) -> Result<Value> {
    let mut item = owned(reg, op, "apps")?.clone();
    let mut request = Operation {
        action: op.action.clone(),
        tenant: op.tenant.clone(),
        id: op.id.clone(),
        data: item.data.clone(),
    };
    for key in ["memory_mb", "cpu_millis", "disk_mb"] {
        if let Some(value) = op.data.get(key) {
            request.data[key] = value.clone();
        }
    }
    let limits = check(reg, &request, false)?;
    let running = pod(
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
    let old_disk = allocation(&item.data)["disk_mb"].as_u64().unwrap();
    ensure!(
        limits["disk_mb"].as_u64().unwrap() >= old_disk,
        "Volumes cannot shrink in place; migrate to a smaller application"
    );
    let disk_change = limits["disk_mb"].as_u64().unwrap() != old_disk
        || !volumes(&op.tenant).contains_key(&op.id);
    if !disk_change {
        if !running {
            pod(&op.tenant, args(&["start", &container(&op.id)]), None, 40).await?;
        }
        let update = pod(
            &op.tenant,
            args(&[
                "update",
                "--memory",
                &format!("{}m", limits["memory_mb"]),
                "--cpus",
                &format!(
                    "{:.3}",
                    limits["cpu_millis"].as_u64().unwrap() as f64 / 1000.0
                ),
                &container(&op.id),
            ]),
            None,
            30,
        )
        .await;
        if !running {
            pod(
                &op.tenant,
                args(&["stop", "--time", "15", &container(&op.id)]),
                None,
                40,
            )
            .await?;
        }
        update?;
        for key in ["memory_mb", "cpu_millis"] {
            item.data[key] = limits[key].clone();
        }
        reg.items.insert(op.id.clone(), item);
        return status(reg, op);
    }
    let ide = item.data["ide_enabled"] == true;
    let ide_name = format!("cgp_ide_{}", op.id);
    let transfer = !s(&item.data, "transfer_user").is_empty();
    let original_transfer = json!({"sftp":item.data["sftp_enabled"]==true,"ftps":item.data["ftps_enabled"]==true,"rotate":false,"allowed_ips":item.data["transfer_ips"].as_array().cloned().unwrap_or_default()});
    let result = async {
        if transfer {
            services_agent::disable(reg, op).await?;
        }
        if running {
            pod(
                &op.tenant,
                args(&["stop", "--time", "15", &container(&op.id)]),
                None,
                40,
            )
            .await?;
        }
        if ide {
            pod(
                &op.tenant,
                args(&["stop", "--time", "10", &ide_name]),
                None,
                30,
            )
            .await?;
        }
        let transfer = s(&item.data, "transfer_user");
        if !transfer.is_empty() {
            let _ = run("pkill", &["-KILL", "-u", transfer]).await;
        }
        volume(op, limits["disk_mb"].as_u64().unwrap()).await?;
        item.data["disk_mb"] = limits["disk_mb"].clone();
        reg.items.insert(op.id.clone(), item.clone());
        // crun cannot update the cgroup of a stopped rootless container.
        pod(&op.tenant, args(&["start", &container(&op.id)]), None, 40).await?;
        pod(
            &op.tenant,
            args(&[
                "update",
                "--memory",
                &format!("{}m", limits["memory_mb"]),
                "--cpus",
                &format!(
                    "{:.3}",
                    limits["cpu_millis"].as_u64().unwrap() as f64 / 1000.0
                ),
                &container(&op.id),
            ]),
            None,
            30,
        )
        .await?;
        for key in ["memory_mb", "cpu_millis"] {
            item.data[key] = limits[key].clone();
        }
        reg.items.insert(op.id.clone(), item);
        Ok::<_, anyhow::Error>(())
    }
    .await;
    let app_resume = if running {
        pod(&op.tenant, args(&["start", &container(&op.id)]), None, 40)
            .await
            .map(|_| ())
    } else {
        pod(
            &op.tenant,
            args(&["stop", "--time", "15", &container(&op.id)]),
            None,
            40,
        )
        .await
        .map(|_| ())
    };
    let ide_resume = if ide {
        pod(&op.tenant, args(&["start", &ide_name]), None, 40)
            .await
            .map(|_| ())
    } else {
        Ok(())
    };
    let transfer_resume = if transfer {
        services_agent::configure(
            reg,
            &Operation {
                action: "services_configure".into(),
                tenant: op.tenant.clone(),
                id: op.id.clone(),
                data: original_transfer,
            },
        )
        .await
        .map(|_| ())
    } else {
        Ok(())
    };
    result?;
    transfer_resume?;
    app_resume?;
    ide_resume?;
    status(reg, op)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fifty_gib_cannot_be_promised_on_thirty_gib_host() {
        assert!(headroom(30 * 1024, 28 * 1024, 50 * 1024).is_err());
    }
    #[test]
    fn all_accounts_unallocated_promises_count_together() {
        assert!(headroom(30 * 1024, 28 * 1024, 3 * 10 * 1024).is_err());
        assert!(headroom(30 * 1024, 18 * 1024, 10 * 1024).is_ok());
    }
    #[test]
    fn reserve_cannot_be_consumed_by_growth() {
        assert!(headroom(30 * 1024, 3 * 1024, 1).is_err());
        assert!(headroom(30 * 1024, 4 * 1024, 1024).is_ok());
    }
}
