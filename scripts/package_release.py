#!/usr/bin/env python3
"""Package Ubuntu 24.04 x86_64 release binaries and root-owned deployment tools."""
import hashlib, json, os, pathlib, re, shutil, sys, tarfile, tempfile
root=pathlib.Path(__file__).resolve().parents[1]
version=re.search(r'^version = "([^"]+)"',(root/'Cargo.toml').read_text(),re.M)[1]
target=pathlib.Path(os.environ.get('CARGO_TARGET_DIR',root/'target'))/'release'
output=pathlib.Path(sys.argv[1] if len(sys.argv)>1 else root/'dist');output.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory() as temp:
    stage=pathlib.Path(temp);(stage/'bin').mkdir();(stage/'deploy').mkdir()
    for name in ('cgpanel','cgpanel-agent','cgpanel-workspace','cgpanel-egress','cgpanel-acme-hook'):shutil.copy2(target/name,stage/'bin'/name)
    for name in ('updater.py','apply-release.py','setup-v0.4.py','setup-v0.5.py','workspace-volume.py','harden-sftp.py','setup-mail.py','mail-config.py','adopt-sftp.py','reload-nginx'):shutil.copy2(root/'deploy'/name,stage/'deploy'/name)
    shutil.copytree(root/'deploy/php-runtime',stage/'deploy/php-runtime')
    shutil.copytree(root/'deploy/waf',stage/'deploy/waf')
    (stage/'manifest.json').write_text(json.dumps({'version':version,'platform':'ubuntu-24.04-x86_64'}))
    archive=output/f'CGPanel-v{version}-linux-x86_64.tar.gz'
    with tarfile.open(archive,'w:gz') as tar:
        for path in sorted(stage.rglob('*')):
            if path.is_file():tar.add(path,arcname=path.relative_to(stage).as_posix())
    (output/'SHA256SUMS').write_text(hashlib.sha256(archive.read_bytes()).hexdigest()+'  '+archive.name+'\n')
print(archive)
