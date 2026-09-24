#!/usr/bin/env python3
"""Apply a prebuilt bundle on Ubuntu 24.04; never build code on the target host."""
import json, os, pathlib, shutil, sqlite3, subprocess, sys, time, urllib.request, tarfile, socket
assert os.geteuid()==0
os.umask(0o077)
payload=pathlib.Path(sys.argv[1]).resolve()
manifest=json.loads((payload/'manifest.json').read_text())
version=manifest['version']
snapshot=pathlib.Path('/var/backups')/('cgpanel-'+time.strftime('%Y%m%d-%H%M%S'))
snapshot.mkdir(mode=0o700)
binary_names=['cgpanel','cgpanel-agent','cgpanel-workspace','cgpanel-egress','cgpanel-acme-hook']
# Fixed root-owned configuration roots; never derive restoration targets from a payload.
config_paths=['/etc/cgpanel','/etc/nginx','/etc/postfix','/etc/dovecot','/etc/rspamd','/etc/ssh',
 '/etc/pam.d/cgpanel-ftps','/etc/vsftpd.conf','/etc/fstab','/etc/systemd/system/vsftpd.service.d',
 '/etc/logrotate.d/cgpanel-waf','/usr/local/lib/cgpanel']
config_saved=False
active_services=[name for name in ['nginx','ssh','vsftpd','postfix','dovecot','rspamd','redis-server']
 if subprocess.run(['systemctl','is-active','--quiet',name]).returncode==0]

def run(*args, timeout=180): subprocess.run(args,check=True,timeout=timeout)
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
    with tarfile.open(snapshot/'host-config.tar','w') as archive:
        for name in config_paths:
            if pathlib.Path(name).exists():archive.add(name,arcname=name.lstrip('/'),recursive=True)
    firewall=subprocess.run(['nft','list','table','inet','cgpanel'],capture_output=True,text=True,check=True).stdout
    (snapshot/'firewall.nft').write_text(firewall)
    (snapshot/'active-services.json').write_text(json.dumps(active_services))
    config_saved=True

    (snapshot/'bin').mkdir()
    for name in binary_names:
        old=pathlib.Path('/usr/local/bin')/name
        if old.exists():shutil.copy2(old,snapshot/'bin'/name)
        new=old.with_suffix('.new');shutil.copy2(payload/'bin'/name,new);new.chmod(0o755);new.replace(old)
    run('python3',str(payload/'deploy/setup-v0.4.py'),str(payload),timeout=1500)
    pathlib.Path('/etc/cgpanel/version').write_text(version+'\n')
    run('systemctl','start','cgpanel-agent','cgpanel')
    for attempt in range(40):
        try:
            with urllib.request.urlopen('http://127.0.0.1:2082/healthz',timeout=2) as response: health=json.load(response)
            if health.get('version')==version:
                with socket.socket(socket.AF_UNIX) as broker:
                    broker.settimeout(5);broker.connect('/run/cgpanel/agent.sock')
                    broker.sendall(b'{}\n')
                    if broker.recv(4096):break
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
    if config_saved:
        for name in config_paths:
            path=pathlib.Path(name)
            if path.is_symlink() or path.is_file():path.unlink()
            elif path.is_dir():shutil.rmtree(path)
        with tarfile.open(snapshot/'host-config.tar') as archive:
            archive.extractall('/',filter='fully_trusted')  # Locally created root-only recovery archive.
        subprocess.run(['systemctl','daemon-reload'],timeout=60)
        subprocess.run(['nft','-f','-'],input='delete table inet cgpanel\n'+(snapshot/'firewall.nft').read_text(),text=True,check=True,timeout=30)
        for service in active_services:
            subprocess.run(['systemctl','reload-or-restart',service],timeout=90)
        for service in set(['vsftpd','postfix','dovecot','rspamd','redis-server'])-set(active_services):
            subprocess.run(['systemctl','stop',service],timeout=90)
    elif (snapshot/'etc-cgpanel/version').exists():shutil.copy2(snapshot/'etc-cgpanel/version','/etc/cgpanel/version')
    subprocess.run(['systemctl','start','cgpanel-agent','cgpanel'],timeout=90)
    raise
