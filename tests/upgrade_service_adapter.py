#!/usr/bin/env python3
"""Service-manager adapter ONLY for the disposable upgrade regression container.
Panel binaries, SQLite, Nginx, files and configuration validators remain real.
"""
import json,os,pathlib,signal,subprocess,sys,time
assert os.environ.get('CGPANEL_DISPOSABLE_TEST')=='1'
ROOT=pathlib.Path('/tmp/cgpanel-upgrade-fixture');statefile=ROOT/'services.json';state=json.loads(statefile.read_text()) if statefile.exists() else {}
with (ROOT/'commands.log').open('a') as log:log.write(json.dumps(sys.argv[1:])+'\n')
args=sys.argv[1:];action=args[0];names=[v for v in args[1:] if not v.startswith('-')]
def alive(pid):
 try:return pathlib.Path('/proc/'+str(pid)+'/stat').read_text().split()[2]!='Z'
 except FileNotFoundError:return False
def stop(name):
 value=state.get(name,{})
 if value.get('pid') and alive(value['pid']):
  os.kill(value['pid'],signal.SIGTERM)
  for _ in range(100):
   if not alive(value['pid']):break
   time.sleep(.02)
 state[name]={'active':False}
def start(name):
 if name=='cgpanel':
  if state.get(name,{}).get('pid') and alive(state[name]['pid']):return
  environment={**os.environ,'CGPANEL_DB':'/var/lib/cgpanel/panel.db','CGPANEL_BIND':'127.0.0.1:2082','CGPANEL_AGENT_SOCKET':'/run/cgpanel/agent.sock','CGPANEL_INSECURE_LOCAL':'1'}
  log=(ROOT/'panel.log').open('ab');p=subprocess.Popen(['/usr/local/bin/cgpanel'],env=environment,stdout=log,stderr=log,start_new_session=True,user='cgpanel',group='cgpanel');log.close();state[name]={'active':True,'pid':p.pid}
 else:state[name]={'active':True}
if action=='is-active':raise SystemExit(0 if all(state.get(name,{}).get('active',False) and (not state[name].get('pid') or alive(state[name]['pid'])) for name in names) else 3)
if action in ('stop','restart'):
 for name in names:stop(name)
if action in ('start','restart') or (action=='enable' and '--now' in args):
 for name in names:start(name)
if action in ('reload','reload-or-restart'):
 for name in names:
  if name=='nginx':subprocess.run(['nginx','-s','reload'],check=True,capture_output=True)
  state[name]={'active':True}
statefile.write_text(json.dumps(state))
