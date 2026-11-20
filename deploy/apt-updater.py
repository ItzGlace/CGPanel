#!/usr/bin/env python3
"""APT channel adapter. Package signatures and version selection remain APT's job."""
import json,pathlib,re,sqlite3,subprocess,sys,time
from contextlib import closing
from package_manager import locked_paths
root=pathlib.Path('/var/lib/cgpanel-updates');root.mkdir(mode=0o700,exist_ok=True)
def write(value):
 path=root/'apt-status.tmp';path.write_text(json.dumps(value));path.chmod(0o600);path.replace(root/'status.json')
status={'state':'checking','checked_at':int(time.time()),'channel':'apt'}
try:
 deadline=time.monotonic()+120
 while locked_paths():
  if time.monotonic()>deadline:raise RuntimeError('Package manager is busy; retry after it finishes. Lock files were retained.')
  time.sleep(2)
 subprocess.run(['apt-get','-o','APT::Update::Error-Mode=any','-o','DPkg::Lock::Timeout=120','-o','Acquire::Retries=2','-o','Acquire::http::Timeout=30','-o','Acquire::https::Timeout=30','update'],check=True,timeout=600)
 policy=subprocess.check_output(['apt-cache','policy','cgpanel'],text=True)
 candidate=re.search(r'^\s*Candidate:\s*(\S+)',policy,re.M)
 if not candidate or not re.fullmatch(r'\d+\.\d+\.\d+',candidate[1]):raise RuntimeError('No stable CGPanel candidate in the signed APT repository')
 candidate=candidate[1];installed=pathlib.Path('/etc/cgpanel/version').read_text().strip()
 newer=tuple(map(int,candidate.split('.')))>tuple(map(int,installed.split('.')))
 status.update(available='v'+candidate,installed=installed,state='available' if newer else 'current',release_url='https://github.com/ItzGlace/CGPanel/releases/tag/v'+candidate);write(status)
 if '--check' in sys.argv or not newer:raise SystemExit(0)
 with closing(sqlite3.connect('file:/var/lib/cgpanel/panel.db?mode=ro',uri=True)) as db:
  if db.execute("SELECT count(*) FROM jobs WHERE status='running'").fetchone()[0]:
   status.update(state='deferred',error='Background jobs are running; retry after they finish.');write(status);raise SystemExit(0)
 status['state']='installing';write(status)
 subprocess.run(['apt-get','-o','DPkg::Lock::Timeout=120','install','--only-upgrade','-y','cgpanel'],check=True,timeout=1800)
 status.update(state='current',installed=candidate,updated_at=int(time.time()));write(status)
except SystemExit:raise
except Exception as error:status.update(state='failed',error=str(error)[:1000]);write(status);raise
