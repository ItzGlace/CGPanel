use super::*;
use tokio::net::unix::OwnedWriteHalf;

async fn event(writer: &mut OwnedWriteHalf, value: Value) -> Result<()> {
    tokio::time::timeout(
        Duration::from_secs(5),
        writer.write_all(format!("{value}\n").as_bytes()),
    )
    .await??;
    Ok(())
}

/// Streams bounded command output without holding the registry lock. Commands remain
/// inside the selected tenant container; disconnects kill the client process group.
pub async fn stream(reg: &Registry, op: Operation, writer: &mut OwnedWriteHalf) -> Result<()> {
    ensure!(
        identifier(&op.tenant) && op.tenant.len() >= 20 && identifier(&op.id),
        "Invalid identifier"
    );
    owned(reg, &op, "apps")?;
    let command = s(&op.data, "command");
    ensure!(
        !command.is_empty() && command.len() <= 4000 && !command.contains('\0'),
        "Invalid command"
    );
    let id = uid(&op.tenant).await?;
    let mut cmd = Command::new("runuser");
    cmd.as_std_mut().process_group(0);
    cmd.args(args(&[
        "-u",
        &user(&op.tenant),
        "--",
        "env",
        &format!("XDG_RUNTIME_DIR=/run/user/{id}"),
        &format!("HOME={}", home(&op.tenant)),
        "podman",
        "exec",
        "--user",
        "1000:1000",
        "--workdir",
        "/workspace",
        &container(&op.id),
        "timeout",
        "--signal=KILL",
        "25s",
        "sh",
        "-c",
        command,
    ]))
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    let mut child = cmd.spawn()?;
    let pid = child.id().context("Missing command PID")?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let result = tokio::time::timeout(Duration::from_secs(35), async {
        let mut out = [0u8; 4096]; let mut err = [0u8; 4096];
        let mut out_open = true; let mut err_open = true; let mut bytes = 0usize;
        while out_open || err_open {
            let (kind, n, data) = tokio::select! {
                n = stdout.read(&mut out), if out_open => {let n=n?; ("stdout",n,out[..n].to_vec())},
                n = stderr.read(&mut err), if err_open => {let n=n?; ("stderr",n,err[..n].to_vec())},
            };
            if n == 0 {if kind=="stdout" {out_open=false} else {err_open=false}; continue;}
            bytes += n; ensure!(bytes <= 2_097_152, "Output exceeded 2 MiB; command stopped");
            use base64::Engine;
            event(writer,json!({"type":kind,"data":base64::engine::general_purpose::STANDARD.encode(data)})).await?;
        }
        let status = child.wait().await?;
        event(writer,json!({"type":"exit","code":status.code().unwrap_or(-1)})).await
    }).await;
    if !matches!(&result, Ok(Ok(()))) {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
        let _ = child.kill().await;
    }
    result.context("Command timed out")?
}
