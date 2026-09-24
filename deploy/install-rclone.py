#!/usr/bin/env python3
"""Install the pinned upstream rclone build needed for SFTP SOCKS support."""
import hashlib, io, os, pathlib, platform, urllib.request, zipfile
version='1.75.1'
arch={'x86_64':'amd64','aarch64':'arm64'}[platform.machine()]
name=f'rclone-v{version}-linux-{arch}'
base=f'https://downloads.rclone.org/v{version}/'
with urllib.request.urlopen(base+'SHA256SUMS',timeout=60) as response:
    sums=response.read().decode()
expected=next(line.split()[0] for line in sums.splitlines() if line.split()[-1:]==[name+'.zip'])
with urllib.request.urlopen(base+name+'.zip',timeout=120) as response:
    archive=response.read(100*1024*1024)
assert hashlib.sha256(archive).hexdigest()==expected, 'rclone checksum mismatch'
with zipfile.ZipFile(io.BytesIO(archive)) as archive:
    binary=archive.read(name+'/rclone')
target=pathlib.Path('/usr/local/lib/cgpanel/rclone')
target.parent.mkdir(mode=0o755,parents=True,exist_ok=True)
temporary=target.with_suffix('.new')
temporary.write_bytes(binary);temporary.chmod(0o755);os.replace(temporary,target)
print('Installed verified upstream rclone '+version)
