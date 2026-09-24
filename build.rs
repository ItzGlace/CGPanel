use std::{env, fs, path::Path};
fn collect(dir: &Path, base: &Path, out: &mut String) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, base, out);
            continue;
        }
        let rel = path
            .strip_prefix(base)
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/");
        let mime = match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
            "png" => "image/png",
            "svg" => "image/svg+xml",
            "woff2" => "font/woff2",
            _ => "application/octet-stream",
        };
        let absolute = fs::canonicalize(&path).unwrap();
        out.push_str(&format!(
            "{rel:?} => Some(({mime:?}, include_bytes!({:?}))),\n",
            absolute.to_str().unwrap()
        ));
    }
}
fn main() {
    println!("cargo:rerun-if-changed=web/assets");
    let mut code = String::from(
        "fn embedded_asset(path:&str)->Option<(&'static str,&'static [u8])>{ match path {\n",
    );
    collect(Path::new("web/assets"), Path::new("web/assets"), &mut code);
    code.push_str("_ => None }}\n");
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("assets.rs"),
        code,
    )
    .unwrap();
}
