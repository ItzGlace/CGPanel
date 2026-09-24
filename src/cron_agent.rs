use super::*;

pub fn start(registry: Arc<Mutex<Registry>>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let now = chrono::Utc::now().timestamp();
            let due = {
                let mut reg = registry.lock().await;
                let due: Vec<_> = reg
                    .items
                    .iter()
                    .filter(|(_, i)| {
                        i.kind == "schedules" && i.data["next"].as_i64().unwrap_or(i64::MAX) <= now
                    })
                    .map(|(id, item)| (id.clone(), item.clone()))
                    .collect();
                for (id, item) in &due {
                    if let Some(i) = reg.items.get_mut(id) {
                        match cgpanel::schedule::next(
                            s(&item.data, "schedule"),
                            s(&item.data, "timezone"),
                            now,
                        ) {
                            Ok(next) => i.data["next"] = json!(next),
                            Err(_) => i.data["next"] = json!(i64::MAX),
                        }
                        i.data["last_state"] = json!("running");
                    }
                }
                if !due.is_empty() {
                    let _ = save(&reg).await;
                }
                due
            };
            // Run outside the registry lock so slow tenant commands do not freeze the panel.
            for (id, item) in due {
                let started = chrono::Utc::now().timestamp();
                let result = pod(
                    &item.tenant,
                    args(&[
                        "exec",
                        "--user",
                        "1000:1000",
                        "--workdir",
                        "/workspace",
                        &container(s(&item.data, "app_id")),
                        "timeout",
                        "--signal=KILL",
                        "25s",
                        "sh",
                        "-c",
                        s(&item.data, "command"),
                    ]),
                    None,
                    35,
                )
                .await;
                let success = result.is_ok();
                let output = match result {
                    Ok(value) => value,
                    Err(error) => error.to_string(),
                };
                let mut reg = registry.lock().await;
                if let Some(i) = reg.items.get_mut(&id) {
                    i.data["last_success"] = json!(success);
                    i.data["last_state"] = json!(if success { "succeeded" } else { "failed" });
                    let mut history = i.data["history"].as_array().cloned().unwrap_or_default();
                    history.push(json!({"started":started,"finished":chrono::Utc::now().timestamp(),"success":success,"output":output.chars().take(4096).collect::<String>()}));
                    if history.len() > 20 {
                        history.drain(..history.len() - 20);
                    }
                    i.data["history"] = json!(history);
                    let _ = save(&reg).await;
                }
            }
        }
    });
}
