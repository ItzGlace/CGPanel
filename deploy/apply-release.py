#!/usr/bin/env python3
"""Apply a prebuilt bundle on Ubuntu 24.04; never build code on the target host."""
import fcntl, json, os, pathlib, shutil, signal, sqlite3, stat, subprocess, sys, time, urllib.request, tarfile, socket, traceback
from contextlib import closing
assert os.geteuid()==0
os.umask(0o077)
log_root=pathlib.Path('/var/lib/cgpanel-updates');log_root.mkdir(mode=0o700,exist_ok=True)
install_lock=(log_root/'installer.lock').open('a')
try:fcntl.flock(install_lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
except BlockingIOError:raise SystemExit('Another release installer is still running')
log_path=log_root/'last-install.log'
transcript=log_path.open('a');log_path.chmod(0o600)
def record(message):
    try:transcript.write(message);transcript.flush()
    except OSError:pass  # A full logging filesystem must not prevent recovery.
    try:print(message,end='' if message.endswith('\n') else '\n',flush=True)
    except BrokenPipeError:sys.stdout=open(os.devnull,'w')
def report_exception(kind,error,tb):
    try:traceback.print_exception(kind,error,tb,file=transcript);transcript.flush()
    except OSError:pass
    # The old wrapper may have exited with the SSH session. Do not leave its
    # matching update stuck at "installing" when this installer has failed.
    try:
        status_path=log_root/'status.json';status=json.loads(status_path.read_text())
        if status.get('state')=='installing' and status.get('available')=='v'+version:
            status.update(state='failed',error='Release installation failed; see '+str(log_path))
            temporary=log_root/'installer-status.tmp';temporary.write_text(json.dumps(status));temporary.replace(status_path)
    except (OSError,ValueError,NameError):pass
    sys.__excepthook__(kind,error,tb)
sys.excepthook=report_exception
record('\nCGPanel installer started '+time.strftime('%Y-%m-%d %H:%M:%S UTC',time.gmtime())+'\nLog: '+str(log_path)+'\n')
payload=pathlib.Path(sys.argv[1]).resolve()
manifest=json.loads((payload/'manifest.json').read_text())
version=manifest['version']
snapshot=pathlib.Path('/var/backups')/('cgpanel-'+time.strftime('%Y%m%d-%H%M%S')+'-'+str(time.time_ns()%1000000000))
# Create the recovery directory only after dependency preparation succeeds.
binary_names=['cgpanel','cgpanel-agent','cgpanel-workspace','cgpanel-egress','cgpanel-acme-hook']
# Fixed root-owned configuration roots; never derive restoration targets from a payload.
config_paths=['/etc/cgpanel','/etc/nginx','/etc/postfix','/etc/dovecot','/etc/rspamd','/etc/ssh',
 '/etc/pam.d/cgpanel-ftps','/etc/vsftpd.conf','/etc/fstab','/etc/systemd/system/vsftpd.service.d',
 '/etc/logrotate.d/cgpanel-waf','/usr/local/lib/cgpanel']
config_saved=False
database_saved=False
services_stopped=False
configuration_started=False
activated_binaries=set()
cancelled_signal=None
class InstallInterrupted(RuntimeError):pass
def cancel_install(signum,frame):
    global cancelled_signal
    cancelled_signal=signal.Signals(signum).name
    if services_stopped:raise InstallInterrupted('Installation interrupted by '+cancelled_signal)
    record('Cancellation requested ('+cancelled_signal+'); waiting for the current preparation step to finish before exiting\n')
for signum in (signal.SIGINT,signal.SIGTERM,signal.SIGHUP):signal.signal(signum,cancel_install)
active_services=[name for name in ['nginx','ssh','vsftpd','postfix','dovecot','rspamd','redis-server']
 if subprocess.run(['systemctl','is-active','--quiet',name]).returncode==0]

def run(*args, timeout=180, capture=False):
    record('Command: '+' '.join(map(str,args))+'\n')
    start=log_path.stat().st_size
    try:
        options=dict(stdout=subprocess.PIPE if capture else transcript,
                     stderr=subprocess.PIPE if capture else subprocess.STDOUT,
                     text=True,start_new_session=True)
        if not services_stopped:
            # Let a package transaction finish if the SSH session is interrupted.
            # The cancellation check below runs before any panel service stops.
            result=subprocess.run(args,check=False,timeout=timeout,**options)
        else:
            process=subprocess.Popen(args,**options)
            try:
                out,err=process.communicate(timeout=timeout)
            except BaseException:
                # Stop the configuration command and its children before restoring
                # files; otherwise a surviving child can overwrite the rollback.
                try:os.killpg(process.pid,signal.SIGTERM)
                except ProcessLookupError:pass
                try:process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    try:os.killpg(process.pid,signal.SIGKILL)
                    except ProcessLookupError:pass
                    process.wait(timeout=3)
                # A child may outlive its immediate parent or ignore SIGTERM.
                try:os.killpg(process.pid,signal.SIGKILL)
                except ProcessLookupError:pass
                for stream in (process.stdout,process.stderr):
                    if stream:stream.close()
                raise
            result=subprocess.CompletedProcess(args,process.returncode,out,err)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError('Timed out running '+str(args[0])+'; see '+str(log_path)) from error
    if capture:
        if result.stdout:record(result.stdout)
        if result.stderr:record(result.stderr)
    transcript.flush()
    if result.returncode:
        with log_path.open() as saved:
            saved.seek(start);detail=saved.read()[-6000:]
        raise RuntimeError(str(args[0])+' failed (exit '+str(result.returncode)+'): '+(detail or 'no output')+'\nInstaller log: '+str(log_path))
    return result

def copy_atomic(source,destination):
    temporary=destination.with_name(destination.name+'.update-copy')
    try:shutil.copy2(source,temporary);temporary.replace(destination)
    finally:temporary.unlink(missing_ok=True)

def archive_bytes(path):
    path=pathlib.Path(path)
    if not path.exists() and not path.is_symlink():return 0
    entries=[path]
    if path.is_dir() and not path.is_symlink():
        for folder,dirs,files in os.walk(path,followlinks=False):
            entries.extend(pathlib.Path(folder)/name for name in dirs+files)
    total=0
    for entry in entries:
        details=entry.lstat()
        total+=2048  # Tar/PAX headers, filenames and padding.
        if stat.S_ISREG(details.st_mode):total+=((details.st_size+511)//512)*512
    return total

def check_update_space(requirements,*,reserve=1024*1024*1024,capacity=None):
    if capacity is None:
        def capacity(path):
            path=pathlib.Path(path)
            while not path.exists():path=path.parent
            return path.stat().st_dev,shutil.disk_usage(path).free,str(path)
    filesystems={}
    for path,amount in requirements:
        identity,free,label=capacity(path)
        budget=filesystems.setdefault(identity,{'free':free,'required':reserve,'label':label})
        budget['free']=min(budget['free'],free);budget['required']+=amount
    for budget in filesystems.values():
        if budget['free']<budget['required']:
            mib=1024*1024
            raise RuntimeError('Not enough free disk space on '+budget['label']+': '+str(budget['free']//mib)+' MiB available, '+str((budget['required']+mib-1)//mib)+' MiB required including '+str(reserve//mib)+' MiB safety headroom. Free space before updating; CGPanel services have not been stopped.')

def storage_preflight(*,stage_binaries):
    database=sum(archive_bytes('/var/lib/cgpanel/panel.db'+suffix) for suffix in ('','-wal'))
    backup=database+archive_bytes('/var/lib/cgpanel-agent/registry.json')+archive_bytes('/etc/cgpanel')+sum(archive_bytes(path) for path in config_paths)
    requirements=[('/var/backups',backup),('/var/lib/cgpanel',database),('/etc/cgpanel',16*1024*1024),('/usr/local/lib/cgpanel',64*1024*1024)]
    if stage_binaries:
        requirements.extend([('/usr/local/bin',sum(archive_bytes(payload/'bin'/name) for name in binary_names)),('/var/backups',sum(archive_bytes(pathlib.Path('/usr/local/bin')/name) for name in binary_names))])
    check_update_space(requirements)

def probe_health(expected,*,url='http://127.0.0.1:2082/healthz',agent_socket='/run/cgpanel/agent.sock'):
    # Loopback probes must never inherit an external HTTP proxy.
    http=urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with http.open(url,timeout=2) as response:health=json.load(response)
    if health.get('status')!='ok':raise RuntimeError('Panel health status is not ok')
    if health.get('version')!=expected:raise RuntimeError('Panel reports version '+str(health.get('version'))+'; expected '+expected)
    try:
        with socket.socket(socket.AF_UNIX) as broker:
            broker.settimeout(5);broker.connect(agent_socket)
            broker.sendall(b'{}\n')
            if not broker.recv(4096):raise RuntimeError('Agent returned an empty reply')
    except InstallInterrupted:raise
    except (OSError,RuntimeError) as error:raise RuntimeError('Agent readiness: '+str(error)) from error

# Preinstall packages while existing application containers keep serving traffic.
record('Checking free disk space for staging, recovery and system headroom\n')
storage_preflight(stage_binaries=True)
run('python3',str(payload/'deploy/setup-v0.5.py'),str(payload),'--dependencies-only',timeout=1500)
if cancelled_signal:raise InstallInterrupted('Installation cancelled before stopping services ('+cancelled_signal+')')
snapshot.mkdir(mode=0o700)
staged_binaries={}
try:
    # Copy potentially large binaries while the old services are still online.
    # Complete all binary backups before activating any replacement, so an
    # interruption cannot make a partial backup the rollback source.
    (snapshot/'bin').mkdir()
    for name in binary_names:
        record('Staging binary while services remain online: '+name+'\n')
        old=pathlib.Path('/usr/local/bin')/name
        if old.exists():copy_atomic(old,snapshot/'bin'/name)
        new=old.with_name(old.name+'.release-new');staged_binaries[name]=new
        shutil.copy2(payload/'bin'/name,new);new.chmod(0o755)
    # Hold the job database write lock until both services are stopped. A worker
    # cannot start a new provisioning job between the check and the shutdown.
    with closing(sqlite3.connect('file:/var/lib/cgpanel/panel.db?mode=rw',uri=True,timeout=15)) as guard, guard:
        guard.execute('BEGIN IMMEDIATE')
        tables={row[0] for row in guard.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        if 'jobs' in tables and guard.execute("SELECT count(*) FROM jobs WHERE status='running'").fetchone()[0]:
            raise RuntimeError('Background jobs are running; update deferred. Retry after they finish.')
        if cancelled_signal:raise InstallInterrupted('Installation cancelled before stopping services ('+cancelled_signal+')')
        storage_preflight(stage_binaries=False)
        downtime_started=time.monotonic()
        services_stopped=True
        run('systemctl','stop','cgpanel','cgpanel-agent')
    record('Saving panel database and host configuration to '+str(snapshot)+'\n')
    with closing(sqlite3.connect('file:/var/lib/cgpanel/panel.db?mode=ro',uri=True)) as db,closing(sqlite3.connect(snapshot/'panel.db')) as copy: db.backup(copy)
    database_saved=True
    copy_atomic(pathlib.Path('/var/lib/cgpanel-agent/registry.json'),snapshot/'registry.json')
    shutil.copytree('/etc/cgpanel',snapshot/'etc-cgpanel',symlinks=True)
    with tarfile.open(snapshot/'host-config.tar','w') as archive:
        for name in config_paths:
            if pathlib.Path(name).exists():archive.add(name,arcname=name.lstrip('/'),recursive=True)
    firewall=run('nft','list','table','inet','cgpanel',capture=True).stdout
    (snapshot/'firewall.nft').write_text(firewall)
    (snapshot/'active-services.json').write_text(json.dumps(active_services))
    config_saved=True

    for name in binary_names:
        record('Activating binary: '+name+'\n')
        old=pathlib.Path('/usr/local/bin')/name
        activated_binaries.add(name)
        staged_binaries[name].replace(old)
    configuration_started=True
    run('python3',str(payload/'deploy/setup-v0.4.py'),str(payload),timeout=1500)
    pathlib.Path('/etc/cgpanel/version').write_text(version+'\n')
    record('Starting installed services and checking health\n')
    run('systemctl','start','cgpanel-agent','cgpanel')
    last_error=None
    for attempt in range(40):
        try:
            probe_health(version)
            break
        except InstallInterrupted:raise
        except Exception as error:
            detail=str(error)
            if detail!=last_error:record('Waiting for readiness: '+detail+'\n');last_error=detail
        time.sleep(1)
    else:raise RuntimeError('New panel did not pass its health check: '+str(last_error))
    run('systemctl','is-active','--quiet','cgpanel','cgpanel-agent')
    downtime=round(time.monotonic()-downtime_started,3)
    (snapshot/'outcome.json').write_text(json.dumps({'version':version,'panel_interruption_seconds':downtime,'website_containers_stopped':False}))
    record('Installed '+version+'; recovery snapshot: '+str(snapshot)+'; panel interruption seconds: '+str(downtime)+'\n')
except Exception as error:
    if not services_stopped:raise
    for signum in (signal.SIGINT,signal.SIGTERM,signal.SIGHUP):signal.signal(signum,signal.SIG_IGN)
    # Release unused staging space before trying to log or restore anything.
    for staged in staged_binaries.values():staged.unlink(missing_ok=True)
    if not config_saved:(snapshot/'host-config.tar').unlink(missing_ok=True)
    record('Installation error before rollback: '+str(error)+'\n')
    # Keep startup failures even after rollback replaces the running processes.
    try:run('journalctl','--no-pager','-u','cgpanel.service','-u','cgpanel-agent.service','-n','80',timeout=10)
    except Exception as journal_error:record('Unable to collect service journal: '+str(journal_error)+'\n')
    record('Installation failed; restoring the previous installation\n')
    subprocess.run(['systemctl','stop','cgpanel','cgpanel-agent'],timeout=60)
    for name in activated_binaries:
        if (snapshot/'bin'/name).exists():copy_atomic(snapshot/'bin'/name,pathlib.Path('/usr/local/bin')/name)
    if database_saved and configuration_started:
        for suffix in ('-wal','-shm'):
            pathlib.Path('/var/lib/cgpanel/panel.db'+suffix).unlink(missing_ok=True)
        copy_atomic(snapshot/'panel.db',pathlib.Path('/var/lib/cgpanel/panel.db'))
        shutil.chown('/var/lib/cgpanel/panel.db',user='cgpanel',group='cgpanel')
    if configuration_started and (snapshot/'registry.json').exists():copy_atomic(snapshot/'registry.json',pathlib.Path('/var/lib/cgpanel-agent/registry.json'))
    if config_saved and configuration_started:
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
    subprocess.run(['systemctl','start','cgpanel-agent','cgpanel'],timeout=90)
    raise
finally:
    for staged in staged_binaries.values():staged.unlink(missing_ok=True)
