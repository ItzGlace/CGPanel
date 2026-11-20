#!/usr/bin/env python3
"""A 0.5.2 -> current update against real binaries/SQLite/Nginx, no VM.
Only run in the dedicated test image. systemd and nftables use fixture adapters;
APT command responses use a fixture, but lock contention is a real POSIX lock.
"""
from contextlib import closing
import fcntl,hashlib,http.server,json,os,pathlib,secrets,shutil,signal,socket,sqlite3,subprocess,sys,tarfile,threading,time,urllib.request
assert os.environ.get('CGPANEL_DISPOSABLE_TEST')=='1','Disposable upgrade container required'
SOURCE=pathlib.Path(__file__).resolve().parents[1];ROOT=pathlib.Path('/tmp/cgpanel-upgrade-fixture');ROOT.mkdir()
scenario=sys.argv[1] if len(sys.argv)>1 else 'success';assert scenario in ('success','prepare-failure','setup-failure','snapshot-failure','space-failure','health-failure','interrupted','cancel-preflight')
legacy=pathlib.Path(os.environ['CGPANEL_UPGRADE_LEGACY_BINARY']);new=pathlib.Path(os.environ['CGPANEL_UPGRADE_NEW_BIN_DIR']);version=os.environ['CGPANEL_UPGRADE_NEW_VERSION']
def run(*args,**kw):
 result=subprocess.run(args,text=True,capture_output=True,timeout=100,**kw)
 if result.returncode:raise RuntimeError(str(args[0])+': '+result.stderr[-2500:]+result.stdout[-2500:])
 return result.stdout
for name in ['/etc/cgpanel','/var/lib/cgpanel','/var/lib/cgpanel-agent','/var/lib/cgpanel-updates','/etc/nginx/cgpanel-ide','/var/backups','/run/sshd','/run/cgpanel','/etc/bind/cgpanel']:
 pathlib.Path(name).mkdir(parents=True,exist_ok=True)
run('useradd','--system','--home','/var/lib/cgpanel','--shell','/usr/sbin/nologin','cgpanel')
run('ssh-keygen','-A')
run('openssl','req','-x509','-newkey','rsa:2048','-days','1','-nodes','-keyout','/etc/cgpanel/panel.key','-out','/etc/cgpanel/panel.crt','-subj','/CN=localhost')
pathlib.Path('/etc/cgpanel/version').write_text('0.5.2\n');pathlib.Path('/etc/cgpanel/agent.env').write_text('CGPANEL_PUBLIC_IP=192.0.2.10\n')
firewall='table inet cgpanel {\n set ide_ports { type inet_service; }\n set ftps_ports { type inet_service; flags interval; }\n set mail_ports { type inet_service; flags interval; }\n'+''.join(' set '+name+' { type '+kind+'; flags interval; }\n' for name,kind in [('blocked4','ipv4_addr'),('blocked6','ipv6_addr'),('database4','ipv4_addr'),('database6','ipv6_addr')])+' chain input { type filter hook input priority 0; policy accept; }\n}\n'
pathlib.Path('/etc/cgpanel/firewall.nft').write_text(firewall);(ROOT/'firewall.txt').write_text(firewall)
registry={'items':{},'ports':20000};pathlib.Path('/var/lib/cgpanel-agent/registry.json').write_text(json.dumps(registry))
shutil.copy2(legacy,'/usr/local/bin/cgpanel');os.chmod('/usr/local/bin/cgpanel',0o755)
for name in ['cgpanel-agent','cgpanel-workspace','cgpanel-egress','cgpanel-acme-hook']:
 p=pathlib.Path('/usr/local/bin')/name;p.write_text('#!/bin/sh\necho legacy helper\n');p.chmod(0o755)
old_hashes={name:hashlib.sha256((pathlib.Path('/usr/local/bin')/name).read_bytes()).hexdigest() for name in ['cgpanel','cgpanel-agent','cgpanel-workspace','cgpanel-egress','cgpanel-acme-hook']}
password=secrets.token_urlsafe(24);run('/usr/local/bin/cgpanel','bootstrap',env={**os.environ,'CGPANEL_DB':'/var/lib/cgpanel/panel.db','CGPANEL_ADMIN_PASSWORD':password})
with closing(sqlite3.connect('/var/lib/cgpanel/panel.db')) as db, db:db.execute('CREATE TABLE fixture_sentinel(value TEXT)');db.execute("INSERT INTO fixture_sentinel VALUES('preserve-tenant-data')")
with closing(sqlite3.connect('/var/lib/cgpanel/panel.db')) as db:original_tables={row[0] for row in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
for entry in pathlib.Path('/var/lib/cgpanel').iterdir():shutil.chown(entry,user='cgpanel',group='cgpanel')
shutil.chown('/var/lib/cgpanel',user='cgpanel',group='cgpanel');pathlib.Path('/var/lib/cgpanel').chmod(0o750)
# An independent web workload remains online through the panel update.
class Website(http.server.BaseHTTPRequestHandler):
 def do_GET(self):self.send_response(200);self.end_headers();self.wfile.write(b'KEEPALIVE')
 def log_message(self,*args):pass
website=http.server.ThreadingHTTPServer(('127.0.0.1',9080),Website);threading.Thread(target=website.serve_forever,daemon=True).start()
pathlib.Path('/etc/nginx/sites-enabled/default').unlink(missing_ok=True)
pathlib.Path('/etc/nginx/conf.d/00-fixture.conf').write_text('map $http_upgrade $cg_connection { default upgrade; "" close; }\nserver { listen 8088; location / { proxy_pass http://127.0.0.1:9080; } }\n')
run('nginx','-t');run('nginx')
shutil.copy2(SOURCE/'tests/upgrade_service_adapter.py','/usr/local/bin/systemctl');os.chmod('/usr/local/bin/systemctl',0o755)
(ROOT/'services.json').write_text(json.dumps({'nginx':{'active':True},'ssh':{'active':True}}))
# The broker responds on a real Unix socket; kernel/PODMAN operations are out of scope.
agent=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);agent.bind('/run/cgpanel/agent.sock');agent.listen(8);shutil.chown('/run/cgpanel',group='cgpanel');pathlib.Path('/run/cgpanel').chmod(0o750);shutil.chown('/run/cgpanel/agent.sock',group='cgpanel');pathlib.Path('/run/cgpanel/agent.sock').chmod(0o660)
def broker():
 while True:
  try:connection,_=agent.accept()
  except OSError:return
  with connection:
   connection.recv(8192)
   if scenario!='health-failure':connection.sendall(b'{"ok":false,"error":"fixture broker"}\n')
threading.Thread(target=broker,daemon=True).start()
run('systemctl','start','cgpanel-agent','cgpanel')
def health():
 with urllib.request.urlopen('http://127.0.0.1:2082/healthz',timeout=2) as response:return json.load(response)['version']
for _ in range(100):
 try:
  if health()=='0.5.2':break
 except OSError:pass
 time.sleep(.03)
else:raise AssertionError('Legacy 0.5.2 panel did not start')
# Construct the payload with the actual release packager. This catches missing
# deployment modules/assets instead of masking packaging bugs by copying them.
bundle_path=os.environ.get('CGPANEL_UPGRADE_BUNDLE')
if not bundle_path:
 staging=ROOT/'package-source';(staging/'scripts').mkdir(parents=True)
 shutil.copy2(SOURCE/'scripts/package_release.py',staging/'scripts/package_release.py')
 shutil.copytree(SOURCE/'deploy',staging/'deploy')
 (staging/'Cargo.toml').write_text('[package]\nversion = "'+version+'"\n')
 compiled=ROOT/'compiled';(compiled/'release').mkdir(parents=True)
 for name in old_hashes:shutil.copy2(new/name,compiled/'release'/name)
 run('python3',str(staging/'scripts/package_release.py'),str(ROOT/'dist'),env={**os.environ,'CARGO_TARGET_DIR':str(compiled)})
 bundle_path=str(next((ROOT/'dist').glob('*.tar.gz')))
payload=ROOT/'payload';payload.mkdir()
with tarfile.open(bundle_path) as bundle:bundle.extractall(payload,filter='data')
assert json.loads((payload/'manifest.json').read_text())['version']==version
assert (payload/'deploy/package_manager.py').exists()
if scenario in ('space-failure','snapshot-failure'):
 installer=payload/'deploy/apply-release.py'
 if scenario=='space-failure':
  fixture="import shutil,types\nshutil.disk_usage=lambda path: types.SimpleNamespace(free=0)\n"
 else:
  fixture="import tarfile,pathlib\n_fixture_tar_open=tarfile.open\ndef _fixture_open(name,mode='r',*args,**kw):\n if mode=='w':\n  pathlib.Path(name).write_bytes(b'partial snapshot');raise OSError(28,'FIXTURE_SNAPSHOT_NO_SPACE')\n return _fixture_tar_open(name,mode,*args,**kw)\ntarfile.open=_fixture_open\n"
 installer.write_text(fixture+installer.read_text())
if scenario=='setup-failure':
 with (payload/'deploy/setup-v0.4.py').open('a') as file:file.write("\nraise RuntimeError('FIXTURE_CONFIGURATION_FAILURE')\n")
if scenario=='interrupted':
 with (payload/'deploy/setup-v0.4.py').open('a') as file:file.write("\nimport os,time\nfrom pathlib import Path\nPath('/tmp/cgpanel-upgrade-fixture/configuring.pid').write_text(str(os.getpid()))\ntime.sleep(60)\n")
# Native APT uses POSIX record locks. Hold the real index lock while the
# preparation step starts, and verify it cannot stop the old panel meanwhile.
lock=pathlib.Path('/var/lib/apt/lists/lock');lock.parent.mkdir(parents=True,exist_ok=True)
held=subprocess.Popen([sys.executable,'-c',"import fcntl,sys,time; f=open(sys.argv[1],'a+');fcntl.lockf(f,fcntl.LOCK_EX);print('locked',flush=True);time.sleep(30)",str(lock)],stdout=subprocess.PIPE,text=True);assert held.stdout.readline().strip()=='locked'
pathlib.Path('/usr/local/bin/dpkg-query').write_text('#!/bin/sh\ncase "$*" in *python3-pymysql*) [ -f /tmp/cgpanel-upgrade-fixture/packages-ready ] || exit 1;; esac\nexec /usr/bin/dpkg-query "$@"\n');pathlib.Path('/usr/local/bin/dpkg-query').chmod(0o755)
pathlib.Path('/usr/local/bin/apt-get').write_text('#!/bin/sh\necho "$*" >> /tmp/cgpanel-upgrade-fixture/apt.log\n'+("echo FIXTURE_REPOSITORY_NOT_SIGNED >&2; exit 100\n" if scenario=='prepare-failure' else 'touch /tmp/cgpanel-upgrade-fixture/packages-ready\nexit 0\n'));pathlib.Path('/usr/local/bin/apt-get').chmod(0o755)
pathlib.Path('/usr/local/bin/nft').write_text('#!/bin/sh\ncase "$1" in list) cat /tmp/cgpanel-upgrade-fixture/firewall.txt;; esac\nexit 0\n');pathlib.Path('/usr/local/bin/nft').chmod(0o755)
site_errors=[];done=threading.Event()
def keepalive():
 while not done.is_set():
  try:
   with urllib.request.urlopen('http://127.0.0.1:8088',timeout=1) as response:
    if response.read()!=b'KEEPALIVE':site_errors.append('wrong response')
  except Exception as error:site_errors.append(type(error).__name__)
  time.sleep(.03)
threading.Thread(target=keepalive,daemon=True).start();log=(ROOT/'apply.log').open('w')
# Match the legacy updater, whose context manager left its read connection open
# until the installer subprocess returned. Atomic rollback must tolerate this.
(pathlib.Path('/var/lib/cgpanel-updates')/'status.json').write_text(json.dumps({'state':'installing','available':'v'+version,'installed':'0.5.2'}))
legacy_check=sqlite3.connect('/var/lib/cgpanel/panel.db')
with legacy_check:legacy_check.execute("SELECT count(*) FROM jobs WHERE status='running'").fetchone()
process=subprocess.Popen(['python3',str(payload/'deploy/apply-release.py'),str(payload)],stdout=log,stderr=log)
try:
 time.sleep(.7)
 if scenario=='space-failure':assert process.poll() is not None,'Low-space preflight did not refuse the update'
 else:assert process.poll() is None,'Installer did not wait for APT'
 assert health()=='0.5.2';assert not (ROOT/'apt.log').exists();assert not list(pathlib.Path('/var/backups').glob('cgpanel-*'))
 if scenario=='cancel-preflight':process.send_signal(signal.SIGTERM)
 held.terminate();held.wait();held.stdout.close()
 if scenario=='interrupted':
  for _ in range(1000):
   if (ROOT/'configuring.pid').exists():break
   if process.poll() is not None:raise AssertionError('Installer exited before interruption fixture')
   time.sleep(.05)
  else:raise AssertionError('Installer did not reach configuration fixture')
  competing=subprocess.run(['python3',str(payload/'deploy/apply-release.py'),str(payload)],capture_output=True,text=True,timeout=5)
  assert competing.returncode!=0 and 'Another release installer is still running' in competing.stderr
  assert process.poll() is None,'Competing installer interrupted the active installation'
  process.send_signal(signal.SIGINT)
 code=process.wait(timeout=100);legacy_check.close()
 for _ in range(100):
  try:
   current=health()
   if current==(version if scenario=='success' else '0.5.2'):break
  except OSError:pass
  time.sleep(.03)
 else:raise AssertionError('Panel did not recover after update')
 transcript=pathlib.Path('/var/lib/cgpanel-updates/last-install.log')
 assert transcript.exists() and transcript.stat().st_mode&0o777==0o600
 assert not site_errors,site_errors
 with sqlite3.connect('/var/lib/cgpanel/panel.db') as db:assert db.execute('SELECT value FROM fixture_sentinel').fetchone()[0]=='preserve-tenant-data';assert db.execute('SELECT count(*) FROM users').fetchone()[0]==1
 if scenario=='success':
  assert code==0,transcript.read_text()[-5000:];assert pathlib.Path('/etc/cgpanel/version').read_text().strip()==version
  assert (pathlib.Path('/usr/local/lib/cgpanel')/'updater.py').exists()
 else:
  assert code!=0;assert pathlib.Path('/etc/cgpanel/version').read_text().strip()=='0.5.2'
  assert json.loads(pathlib.Path('/var/lib/cgpanel-updates/status.json').read_text())['state']=='failed','Failed installer left a stale installing state'
  with closing(sqlite3.connect('/var/lib/cgpanel/panel.db')) as db:assert {row[0] for row in db.execute("SELECT name FROM sqlite_master WHERE type='table'") }==original_tables,'Rollback did not restore the original database schema'
  assert {name:hashlib.sha256((pathlib.Path('/usr/local/bin')/name).read_bytes()).hexdigest() for name in old_hashes}==old_hashes
  assert ('Not enough free disk space' if scenario=='space-failure' else 'FIXTURE_SNAPSHOT_NO_SPACE' if scenario=='snapshot-failure' else 'FIXTURE_REPOSITORY_NOT_SIGNED' if scenario=='prepare-failure' else 'empty reply' if scenario=='health-failure' else 'SIGINT' if scenario=='interrupted' else 'SIGTERM' if scenario=='cancel-preflight' else 'FIXTURE_CONFIGURATION_FAILURE') in transcript.read_text()
  if scenario in ('prepare-failure','cancel-preflight','space-failure'):assert not list(pathlib.Path('/var/backups').glob('cgpanel-*'))
  if scenario=='snapshot-failure':assert not list(pathlib.Path('/var/backups').glob('cgpanel-*/host-config.tar')),'Partial snapshot was retained'
  if scenario=='interrupted':
   pid=(ROOT/'configuring.pid').read_text();status=pathlib.Path('/proc')/pid/'stat'
   assert not status.exists() or status.read_text().split()[2]=='Z','Configuration child survived rollback'
 print('PASS:',scenario,'expected installation/recovery verified; panel data preserved; website remains reachable')
 assert not list(pathlib.Path('/usr/local/bin').glob('*.release-new')),'Staged binaries leaked after completion'
finally:
 done.set();legacy_check.close()
 if held.poll() is None:held.terminate();held.wait();held.stdout.close()
 if process.poll() is None:process.kill();process.wait()
 log.close();subprocess.run(['systemctl','stop','cgpanel'],capture_output=True);agent.close();website.shutdown()
