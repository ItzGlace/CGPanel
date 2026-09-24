//! File operations executed as the tenant OS identity, never as root.
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};
fn string<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}
fn parts(path: &str) -> Result<Vec<&str>> {
    ensure!(
        path.len() <= 4096 && !path.starts_with('/') && !path.contains(['\\', '\0']),
        "Invalid workspace path"
    );
    if path.is_empty() {
        return Ok(vec![]);
    }
    let p: Vec<_> = path.split('/').collect();
    ensure!(
        p.iter()
            .all(|x| !x.is_empty() && *x != "." && *x != ".." && x.len() <= 255),
        "Invalid workspace path"
    );
    Ok(p)
}
fn directory(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?)
}
fn fdpath(fd: &File, name: &str) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", fd.as_raw_fd())).join(name)
}
fn walk(root: &File, path: &str) -> Result<File> {
    let mut fd = root.try_clone()?;
    for p in parts(path)? {
        fd = directory(&fdpath(&fd, p))?;
    }
    Ok(fd)
}
fn parent(root: &File, path: &str) -> Result<(File, String)> {
    let mut p = parts(path)?;
    let name = p
        .pop()
        .context("Choose a file or folder, not the workspace root")?;
    Ok((walk(root, &p.join("/"))?, name.into()))
}
fn regular(path: &Path, write: bool) -> Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .write(write)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let m = f.metadata()?;
    ensure!(
        m.is_file() && m.nlink() == 1,
        "Only regular, unlinked files are supported; symlinks and hardlinks are blocked"
    );
    Ok(f)
}
fn rename(from: &File, name: &str, to: &File, target: &str, overwrite: bool) -> Result<()> {
    use std::ffi::CString;
    let a = CString::new(name)?;
    let b = CString::new(target)?;
    let r = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            from.as_raw_fd(),
            a.as_ptr(),
            to.as_raw_fd(),
            b.as_ptr(),
            if overwrite { 0 } else { libc::RENAME_NOREPLACE },
        )
    };
    if r != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
fn create(parent: &File, name: &str) -> Result<File> {
    Ok(OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .mode(0o660)
        .custom_flags(libc::O_NOFOLLOW)
        .open(fdpath(parent, name))?)
}
fn copy_tree(src: &Path, dst: &Path, budget: &mut (u64, u64)) -> Result<()> {
    budget.0 += 1;
    ensure!(budget.0 <= 10000, "Copy exceeds 10,000 entries");
    let meta = fs::symlink_metadata(src)?;
    ensure!(!meta.is_symlink(), "Copy does not follow symbolic links");
    if meta.is_dir() {
        let source = directory(src)?;
        fs::create_dir(dst)?;
        let dest = directory(dst)?;
        for entry in fs::read_dir(fdpath(&source, ""))? {
            let name = entry?.file_name();
            copy_tree(
                &fdpath(&source, "").join(&name),
                &fdpath(&dest, "").join(&name),
                budget,
            )?;
        }
    } else {
        let input = regular(src, false)?;
        budget.1 += meta.len();
        ensure!(budget.1 <= 1024 * 1024 * 1024, "Copy exceeds 1 GiB");
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o660)
            .custom_flags(libc::O_NOFOLLOW)
            .open(dst)?;
        let copied = std::io::copy(&mut input.take(meta.len() + 1), &mut output)?;
        ensure!(copied <= meta.len(), "Source grew while copying; retry");
    }
    Ok(())
}
fn zip_tree(
    writer: &mut zip::ZipWriter<File>,
    source: &Path,
    name: &str,
    budget: &mut (u64, u64),
) -> Result<()> {
    use zip::write::SimpleFileOptions;
    budget.0 += 1;
    ensure!(budget.0 <= 10000, "Archive exceeds 10,000 entries");
    let meta = fs::symlink_metadata(source)?;
    ensure!(!meta.is_symlink(), "Archives do not follow symbolic links");
    if meta.is_dir() {
        let dir = directory(source)?;
        writer.add_directory(format!("{name}/"), SimpleFileOptions::default())?;
        for entry in fs::read_dir(fdpath(&dir, ""))? {
            let entry = entry?;
            let child = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Archive names must be UTF-8"))?;
            zip_tree(
                writer,
                &fdpath(&dir, &child),
                &format!("{name}/{child}"),
                budget,
            )?;
        }
    } else {
        let file = regular(source, false)?;
        let remaining = 1024 * 1024 * 1024 - budget.1;
        ensure!(meta.len() <= remaining, "Archive exceeds 1 GiB");
        writer.start_file(
            name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )?;
        let copied = std::io::copy(&mut file.take(remaining + 1), writer)?;
        budget.1 += copied;
        ensure!(budget.1 <= 1024 * 1024 * 1024, "Archive exceeds 1 GiB");
    }
    Ok(())
}
fn extract_zip(source: &Path, parent: &File, name: &str) -> Result<()> {
    let mut archive = zip::ZipArchive::new(regular(source, false)?)?;
    ensure!(archive.len() <= 10000, "ZIP exceeds 10,000 entries");
    let temporary = format!(".cgpanel-extract-{}", uuid::Uuid::new_v4().simple());
    let staging = fdpath(parent, &temporary);
    fs::create_dir(&staging)?;
    let root = directory(&staging)?;
    let result = (|| -> Result<()> {
        let mut total = 0u64;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().trim_end_matches('/').to_owned();
            let components = parts(&name)?;
            ensure!(!components.is_empty(), "Invalid ZIP entry");
            let mode = entry.unix_mode().unwrap_or(0) & 0o170000;
            ensure!(
                [0, 0o100000, 0o040000].contains(&mode),
                "ZIP contains a symlink or special file"
            );
            let mut dir = root.try_clone()?;
            for component in &components[..components.len() - 1] {
                let p = fdpath(&dir, component);
                if !p.exists() {
                    fs::create_dir(&p)?;
                }
                dir = directory(&p)?;
            }
            let leaf = components.last().unwrap();
            let p = fdpath(&dir, leaf);
            if entry.is_dir() {
                if !p.exists() {
                    fs::create_dir(&p)?;
                }
                directory(&p)?;
            } else {
                let mut out = create(&dir, leaf)?;
                let remaining = 1024 * 1024 * 1024 - total;
                ensure!(entry.size() <= remaining, "Expanded ZIP exceeds 1 GiB");
                let copied = std::io::copy(
                    &mut std::io::Read::by_ref(&mut entry).take(remaining + 1),
                    &mut out,
                )?;
                total += copied;
                ensure!(total <= 1024 * 1024 * 1024, "Expanded ZIP exceeds 1 GiB");
            }
        }
        rename(parent, &temporary, parent, name, false)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}
fn execute(root: &File, v: Value) -> Result<Value> {
    let action = string(&v, "operation");
    let path = string(&v, "path");
    if action == "list" {
        let dir = walk(root, path)?;
        let mut entries = vec![];
        for entry in fs::read_dir(fdpath(&dir, ""))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let m = fs::symlink_metadata(entry.path())?;
            entries.push(json!({"name":name,"kind":if m.is_symlink(){"link"}else if m.is_dir(){"directory"}else if m.is_file(){"file"}else{"special"},"size":m.len(),"modified":m.mtime(),"mode":format!("{:03o}",m.mode()&0o777)}));
            ensure!(entries.len() <= 100000, "Directory exceeds 100,000 entries");
        }
        entries.sort_by(|a, b| {
            (a["kind"] != "directory", string(a, "name").to_lowercase())
                .cmp(&(b["kind"] != "directory", string(b, "name").to_lowercase()))
        });
        let total = entries.len();
        let offset = v["offset"].as_u64().unwrap_or(0) as usize;
        return Ok(
            json!({"path":path,"total":total,"entries":entries.into_iter().skip(offset).take(400).collect::<Vec<_>>()}),
        );
    }
    let (dir, name) = parent(root, path)?;
    let target = fdpath(&dir, &name);
    match action {
        "read" | "download" => {
            let mut f = regular(&target, false)?;
            let size = f.metadata()?.len();
            if action == "read" {
                ensure!(
                    size <= 262144,
                    "Editor supports text files up to 256 KiB; use code-server for larger files"
                );
                let mut data = String::new();
                f.read_to_string(&mut data)?;
                ensure!(!data.contains('\0'), "Binary file; use Download instead");
                Ok(
                    json!({"content":data,"revision":format!("{:x}",Sha256::digest(data.as_bytes())),"size":size}),
                )
            } else {
                let offset = v["offset"].as_u64().unwrap_or(0);
                ensure!(offset <= size, "Invalid offset");
                f.seek(SeekFrom::Start(offset))?;
                let mut data = vec![0; 196608];
                let n = f.read(&mut data)?;
                data.truncate(n);
                Ok(
                    json!({"data":STANDARD.encode(&data),"size":size,"next":offset+n as u64,"done":offset+n as u64>=size}),
                )
            }
        }
        "save" => {
            let content = string(&v, "content");
            ensure!(content.len() <= 262144, "Editor limit is 256 KiB");
            let mut current = regular(&target, true)?;
            let mut data = Vec::new();
            std::io::Read::by_ref(&mut current)
                .take(262145)
                .read_to_end(&mut data)?;
            ensure!(
                format!("{:x}", Sha256::digest(&data)) == string(&v, "revision"),
                "File changed since you opened it; reopen before saving"
            );
            current.seek(SeekFrom::Start(0))?;
            current.write_all(content.as_bytes())?;
            current.set_len(content.len() as u64)?;
            current.sync_all()?;
            Ok(json!({"revision":format!("{:x}",Sha256::digest(content.as_bytes()))}))
        }
        "archive" => {
            let dest = string(&v, "destination");
            ensure!(
                dest != path && !dest.starts_with(&format!("{path}/")),
                "Archive cannot be inside its source"
            );
            let (to, new) = parent(root, dest)?;
            let file = create(&to, &new)?;
            let mut zip = zip::ZipWriter::new(file);
            let result = zip_tree(&mut zip, &target, &name, &mut (0, 0));
            if result.is_err() {
                let _ = fs::remove_file(fdpath(&to, &new));
            }
            result?;
            zip.finish()?.sync_all()?;
            Ok(json!({"created":true}))
        }
        "extract" => {
            let (to, new) = parent(root, string(&v, "destination"))?;
            extract_zip(&target, &to, &new)?;
            Ok(json!({"extracted":true}))
        }
        "touch" => {
            create(&dir, &name)?;
            Ok(json!({"created":true}))
        }
        "mkdir" => {
            fs::create_dir(&target)?;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o2770))?;
            Ok(json!({"created":true}))
        }
        "move" | "copy" => {
            let destination = string(&v, "destination");
            ensure!(
                destination != path && !destination.starts_with(&format!("{path}/")),
                "Destination cannot be inside the source"
            );
            let (to, new) = parent(root, destination)?;
            if action == "move" {
                rename(&dir, &name, &to, &new, false)?;
            } else {
                copy_tree(&target, &fdpath(&to, &new), &mut (0, 0))?;
            }
            Ok(json!({"done":true}))
        }
        "trash" => {
            let trash = fdpath(root, ".cgpanel-trash");
            if !trash.exists() {
                fs::create_dir(&trash)?;
            }
            let td = directory(&trash)?;
            let new = format!("{}--{}", uuid::Uuid::new_v4().simple(), name);
            rename(&dir, &name, &td, &new, false)?;
            Ok(json!({"trash_path":format!(".cgpanel-trash/{new}")}))
        }
        "chmod" => {
            let mode = u32::from_str_radix(string(&v, "mode"), 8)?;
            ensure!(
                [0o600, 0o640, 0o644, 0o660, 0o664, 0o700, 0o750, 0o755, 0o770, 0o775]
                    .contains(&mode),
                "Choose a standard permission mode without special bits"
            );
            let f = if fs::symlink_metadata(&target)?.is_dir() {
                directory(&target)?
            } else {
                regular(&target, false)?
            };
            fs::set_permissions(fdpath(&f, ""), fs::Permissions::from_mode(mode))?;
            Ok(json!({"saved":true}))
        }
        "upload_begin" => {
            let name = format!(".cgpanel-upload-{}", uuid::Uuid::new_v4().simple());
            create(root, &name)?;
            Ok(json!({"upload":name}))
        }
        "upload_chunk" | "upload_finish" | "upload_cancel" => {
            let upload = string(&v, "upload");
            ensure!(
                upload.starts_with(".cgpanel-upload-")
                    && upload.len() == 48
                    && !upload.contains('/'),
                "Invalid upload identifier"
            );
            let temp = fdpath(root, upload);
            if action == "upload_cancel" {
                fs::remove_file(temp)?;
                return Ok(json!({"cancelled":true}));
            }
            let mut f = regular(&temp, true)?;
            if action == "upload_chunk" {
                let data = STANDARD.decode(string(&v, "data"))?;
                ensure!(data.len() <= 196608, "Chunk exceeds 192 KiB");
                let offset = v["offset"].as_u64().unwrap_or(u64::MAX);
                ensure!(
                    offset == f.metadata()?.len()
                        && offset + data.len() as u64 <= 2 * 1024 * 1024 * 1024,
                    "Upload offset mismatch or 2 GiB limit exceeded"
                );
                f.seek(SeekFrom::End(0))?;
                f.write_all(&data)?;
                Ok(json!({"next":offset+data.len() as u64}))
            } else {
                f.sync_all()?;
                if target.exists() {
                    ensure!(
                        v["overwrite"] == true,
                        "Destination exists; choose overwrite explicitly"
                    );
                    regular(&target, false)?;
                }
                rename(root, upload, &dir, &name, v["overwrite"] == true)?;
                Ok(json!({"uploaded":true}))
            }
        }
        _ => bail!("Unknown file operation"),
    }
}
fn main() {
    unsafe {
        libc::umask(0o007);
    }
    let result = (|| -> Result<Value> {
        let base = std::env::args().nth(1).context("Missing workspace")?;
        let mut fd = directory(Path::new("/"))?;
        for part in base.trim_start_matches('/').split('/') {
            ensure!(
                !part.is_empty() && part != ".." && part != ".",
                "Invalid root"
            );
            fd = directory(&fdpath(&fd, part))?;
        }
        let input: Value = serde_json::from_reader(std::io::stdin().take(600000))?;
        execute(&fd, input)
    })();
    match result {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    }
}
