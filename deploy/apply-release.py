#!/usr/bin/env python3
"""Apply a prebuilt bundle on Ubuntu 24.04; never build code on the target host."""
import json, os, pathlib, shutil, sqlite3, subprocess, sys, time, urllib.request
assert os.geteuid()==0
os.umask(0o077)
payload=pathlib.Path(sys.argv[1]).resolve()
manifest=json.loads((payload/'manifest.json').read_text())
version=manifest['version']
snapshot=pathlib.Path('/var/backups')/('cgpanel-'+time.strftime('%Y%m%d-%H%M%S'))
snapshot.mkdir(mode=0o700)
binary_names=['cgpanel','cgpanel-agent','cgpanel-workspace','cgpanel-egress','cgpanel-acme-hook']
def run(*args): subprocess.run(args,check=True,timeout=180)
# Hold the job database write lock until both services are stopped. A worker
# cannot start a new provisioning job between the check and the shutdown.
with sqlite3.connect('/var/lib/cgpanel/panel.db',timeout=15) as guard:
    guard.execute('BEGIN IMMEDIATE')
    tables={row[0] for row in guard.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    if 'jobs' in tables and guard.execute("SELECT count(*) FROM jobs WHERE status='running'").fetchone()[0]:
        raise RuntimeError('Background jobs are running; update deferred. Retry after they finish.')
    run('systemctl','stop','cgpanel','cgpanel-agent')
try:
    with sqlite3.connect('/var/lib/cgpanel/panel.db') as db,sqlite3.connect(snapshot/'panel.db') as copy: db.backup(copy)
    shutil.copy2('/var/lib/cgpanel-agent/registry.json',snapshot/'registry.json')
    shutil.copytree('/etc/cgpanel',snapshot/'etc-cgpanel')
    (snapshot/'bin').mkdir()
    for name in binary_names:
        old=pathlib.Path('/usr/local/bin')/name
        if old.exists():shutil.copy2(old,snapshot/'bin'/name)
        new=old.with_suffix('.new');shutil.copy2(payload/'bin'/name,new);new.chmod(0o755);new.replace(old)
    run('python3',str(payload/'deploy/setup-v0.4.py'),str(payload))
    pathlib.Path('/etc/cgpanel/version').write_text(version+'\n')
    run('systemctl','start','cgpanel-agent','cgpanel')
    for attempt in range(40):
        try:
            with urllib.request.urlopen('http://127.0.0.1:2082/healthz',timeout=2) as response: health=json.load(response)
            if health.get('version')==version:break
        except Exception:pass
        time.sleep(1)
    else:raise RuntimeError('New panel did not pass its health check')
    run('systemctl','is-active','--quiet','cgpanel','cgpanel-agent')
    print('Installed',version,'Recovery snapshot:',snapshot)
except Exception:
    subprocess.run(['systemctl','stop','cgpanel','cgpanel-agent'],timeout=60)
    for name in binary_names:
        if (snapshot/'bin'/name).exists():shutil.copy2(snapshot/'bin'/name,pathlib.Path('/usr/local/bin')/name)
    if (snapshot/'panel.db').exists():
        for suffix in ('-wal','-shm'):
            pathlib.Path('/var/lib/cgpanel/panel.db'+suffix).unlink(missing_ok=True)
        shutil.copy2(snapshot/'panel.db','/var/lib/cgpanel/panel.db')
        shutil.chown('/var/lib/cgpanel/panel.db',user='cgpanel',group='cgpanel')
    if (snapshot/'registry.json').exists():shutil.copy2(snapshot/'registry.json','/var/lib/cgpanel-agent/registry.json')
    if (snapshot/'etc-cgpanel/version').exists():shutil.copy2(snapshot/'etc-cgpanel/version','/etc/cgpanel/version')
    subprocess.run(['systemctl','start','cgpanel-agent','cgpanel'],timeout=90)
    raise
