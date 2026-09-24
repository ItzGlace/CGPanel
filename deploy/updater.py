#!/usr/bin/env python3
"""Root-owned updater for stable ItzGlace/CGPanel GitHub releases, with rollback."""
import urllib.error
import fcntl, hashlib, json, os, pathlib, platform, re, shutil, sqlite3, subprocess, sys, tarfile, tempfile, time, urllib.request
ROOT=pathlib.Path('/var/lib/cgpanel-updates')
ROOT.mkdir(mode=0o700,exist_ok=True)
os.umask(0o077)
lock=(ROOT/'lock').open('w')
try: fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
except BlockingIOError: raise SystemExit(0)
def run(*command): return subprocess.run(command,check=True,capture_output=True,text=True,timeout=1800)
def write(value):
    temp=ROOT/'status.tmp';temp.write_text(json.dumps(value));temp.replace(ROOT/'status.json')
def version(value):
    if not re.fullmatch(r'v?\d+\.\d+\.\d+',value): raise ValueError('Only stable semantic release versions are supported')
    return tuple(map(int,value.removeprefix('v').split('.')))
def fetch(url,limit):
    request=urllib.request.Request(url,headers={'User-Agent':'CGPanel-updater','Accept':'application/vnd.github+json' if 'api.github.com' in url else 'application/octet-stream'})
    with urllib.request.urlopen(request,timeout=90) as response:
        data=response.read(limit+1)
    if len(data)>limit: raise ValueError('Download exceeds maximum size')
    return data
status={'state':'checking','checked_at':int(time.time())}
try:
    config=json.loads(pathlib.Path('/etc/cgpanel/updates.json').read_text())
    if '--auto' in sys.argv and not config.get('enabled',False): raise SystemExit(0)
    try: release=json.loads(fetch('https://api.github.com/repos/ItzGlace/CGPanel/releases/latest',2*1024*1024))
    except urllib.error.HTTPError as error:
        if error.code==404:
            status.update(state='no_stable_release',error='No published stable release is available yet. Prereleases are not installed automatically.');write(status);raise SystemExit(0)
        raise
    tag=release['tag_name'];version(tag)
    installed=pathlib.Path('/etc/cgpanel/version').read_text().strip()
    status.update(available=tag,installed=installed,release_url=release['html_url'],state='available' if version(tag)>version(installed) else 'current')
    write(status)
    if '--check' in sys.argv or version(tag)<=version(installed): raise SystemExit(0)
    with sqlite3.connect('/var/lib/cgpanel/panel.db') as db:
        if db.execute("SELECT count(*) FROM jobs WHERE status='running'").fetchone()[0]:
            status.update(state='deferred',error='Background jobs are running. The next scheduled check will retry.');write(status);raise SystemExit(0)
    assert platform.machine()=='x86_64','This release updater currently supports x86_64'
    asset=f'CGPanel-{tag}-linux-x86_64.tar.gz'
    names={item['name']:item for item in release['assets']}
    assert asset in names and 'SHA256SUMS' in names,'Release does not provide a binary update bundle'
    base=f'https://github.com/ItzGlace/CGPanel/releases/download/{tag}/'
    sums=fetch(base+'SHA256SUMS',65536).decode()
    expected=next(line.split()[0] for line in sums.splitlines() if line.split()[-1:]==[asset])
    status['state']='downloading';write(status)
    archive=fetch(base+asset,128*1024*1024)
    assert hashlib.sha256(archive).hexdigest()==expected,'Update checksum verification failed'
    staging=pathlib.Path(tempfile.mkdtemp(prefix='release-',dir=ROOT))
    bundle=staging/'release.tar.gz';bundle.write_bytes(archive)
    with tarfile.open(bundle) as source:
        for member in source.getmembers():
            path=pathlib.PurePosixPath(member.name)
            assert not path.is_absolute() and '..' not in path.parts and not member.issym() and not member.islnk(),'Unsafe archive member'
            assert member.isdir() or member.isfile(),'Unsupported archive member'
        source.extractall(staging/'payload',filter='data')
    payload=staging/'payload'
    manifest=json.loads((payload/'manifest.json').read_text())
    assert manifest['version']==tag.removeprefix('v'),'Manifest version mismatch'
    for name in ('cgpanel','cgpanel-agent','cgpanel-workspace','cgpanel-egress','cgpanel-acme-hook'):
        assert (payload/'bin'/name).is_file(),'Incomplete binary release'
    status['state']='installing';write(status)
    run('python3',str(payload/'deploy/apply-release.py'),str(payload))
    status.update(state='current',installed=tag.removeprefix('v'),updated_at=int(time.time()))
    write(status)
    shutil.rmtree(staging)
except SystemExit: raise
except Exception as error:
    status.update(state='failed',error=str(error)[:1000]);write(status);raise
