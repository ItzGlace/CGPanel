#!/usr/bin/env python3
"""Real local HTTP/socket probes and interrupted installer command groups."""
import ast,http.server,io,json,os,pathlib,shutil,signal,socket,stat,subprocess,sys,tempfile,threading,time,unittest,urllib.request
from unittest.mock import patch

source=pathlib.Path(__file__).resolve().parents[1]/'deploy/apply-release.py'
names={'InstallInterrupted','record','cancel_install','run','probe_health','copy_atomic','archive_bytes','check_update_space'}
definitions=ast.Module(body=[n for n in ast.parse(source.read_text()).body if isinstance(n,(ast.ClassDef,ast.FunctionDef)) and n.name in names],type_ignores=[])

class Tests(unittest.TestCase):
 def setUp(self):
  folder=tempfile.TemporaryDirectory();self.addCleanup(folder.cleanup);self.folder=pathlib.Path(folder.name)
  self.log=self.folder/'install.log';self.transcript=self.log.open('a');self.addCleanup(self.transcript.close)
  self.scope={'json':json,'os':os,'pathlib':pathlib,'shutil':shutil,'signal':signal,'socket':socket,'stat':stat,'subprocess':subprocess,'sys':sys,'urllib':urllib,'transcript':self.transcript,'log_path':self.log,'services_stopped':True,'cancelled_signal':None}
  exec(compile(definitions,str(source),'exec'),self.scope)
 def http(self,value):
  class Handler(http.server.BaseHTTPRequestHandler):
   def do_GET(self):self.send_response(200);self.end_headers();self.wfile.write(json.dumps(value).encode())
   def log_message(self,*args):pass
  server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
  self.addCleanup(lambda:(server.shutdown(),server.server_close(),thread.join()))
  return 'http://127.0.0.1:'+str(server.server_port)+'/healthz'
 def agent(self,reply):
  path=self.folder/'agent.sock';listener=socket.socket(socket.AF_UNIX);listener.bind(str(path));listener.listen(1);listener.settimeout(3)
  def respond():
   try:
    with listener.accept()[0] as connection:
     self.assertEqual(connection.recv(128),b'{}\n')
     if reply:connection.sendall(reply)
   finally:listener.close()
  thread=threading.Thread(target=respond,daemon=True);thread.start();self.addCleanup(lambda:thread.join(timeout=4))
  return str(path)
 def test_real_loopback_http_ignores_proxy_and_requires_agent_response(self):
  url=self.http({'status':'ok','version':'fixture'});agent=self.agent(b'{"ok":false,"error":"missing action"}\n')
  with patch.dict(os.environ,{'http_proxy':'http://127.0.0.1:1','HTTP_PROXY':'http://127.0.0.1:1','no_proxy':'','NO_PROXY':''}):
   self.scope['probe_health']('fixture',url=url,agent_socket=agent)
 def test_wrong_panel_version_reports_actual_and_expected(self):
  with self.assertRaisesRegex(RuntimeError,'reports version old; expected new'):
   self.scope['probe_health']('new',url=self.http({'status':'ok','version':'old'}),agent_socket=str(self.folder/'absent'))
 def test_agent_not_ready_is_distinguished_from_panel_startup(self):
  with self.assertRaisesRegex(RuntimeError,'Agent readiness:'):
   self.scope['probe_health']('fixture',url=self.http({'status':'ok','version':'fixture'}),agent_socket=str(self.folder/'absent'))
 def test_empty_agent_reply_does_not_pass_readiness(self):
  with self.assertRaisesRegex(RuntimeError,'empty reply'):
   self.scope['probe_health']('fixture',url=self.http({'status':'ok','version':'fixture'}),agent_socket=self.agent(b''))
 def test_configuration_children_are_terminated_on_each_cancel_signal(self):
  for signum in (signal.SIGINT,signal.SIGTERM,signal.SIGHUP):
   with self.subTest(signal=signum):
    marker=self.folder/('pids-'+str(signum));ready=self.folder/('ready-'+str(signum))
    child="import os,pathlib,signal,time;signal.signal(signal.SIGTERM,signal.SIG_IGN);pathlib.Path("+repr(str(ready))+").write_text(str(os.getpid()));time.sleep(60)"
    command="import os,pathlib,subprocess,sys,time\np=subprocess.Popen([sys.executable,'-c',"+repr(child)+"])\nwhile not pathlib.Path("+repr(str(ready))+").exists():time.sleep(.01)\npathlib.Path("+repr(str(marker))+").write_text(str(os.getpid())+' '+str(p.pid))\ntime.sleep(60)"
    def cancel():
     for _ in range(500):
      if marker.exists():os.kill(os.getpid(),signum);return
      time.sleep(.01)
    previous=signal.signal(signum,self.scope['cancel_install']);thread=threading.Thread(target=cancel,daemon=True);thread.start()
    try:
     with self.assertRaisesRegex(self.scope['InstallInterrupted'],signal.Signals(signum).name):self.scope['run'](sys.executable,'-c',command,timeout=10)
    finally:signal.signal(signum,previous);thread.join(timeout=6)
    for pid in marker.read_text().split():
     status=pathlib.Path('/proc')/pid/'stat'
     for _ in range(100):
      if not status.exists() or status.read_text().split()[2]=='Z':break
      time.sleep(.01)
     else:self.fail('Configuration child survived cancellation')
 def test_preparation_finishes_before_pending_cancel_is_reported(self):
  self.scope['services_stopped']=False;marker=self.folder/'preparing'
  command="import pathlib,time;pathlib.Path("+repr(str(marker))+").touch();time.sleep(.2);print('prepared')"
  def cancel():
   for _ in range(500):
    if marker.exists():os.kill(os.getpid(),signal.SIGTERM);return
    time.sleep(.01)
  previous=signal.signal(signal.SIGTERM,self.scope['cancel_install']);thread=threading.Thread(target=cancel,daemon=True);thread.start()
  try:self.assertEqual(self.scope['run'](sys.executable,'-c',command,timeout=5).returncode,0)
  finally:signal.signal(signal.SIGTERM,previous);thread.join(timeout=6)
  self.assertEqual(self.scope['cancelled_signal'],'SIGTERM');self.assertIn('prepared',self.log.read_text())
 def test_disconnected_stdout_does_not_prevent_persistent_logging(self):
  class Disconnected:
   def write(self,*args):raise BrokenPipeError()
   def flush(self):pass
  with patch.object(sys,'stdout',Disconnected()):
   self.scope['record']('recovery continues\n');replacement=sys.stdout
  replacement.close()
  self.assertIn('recovery continues',self.log.read_text())
 def test_interrupted_copy_never_exposes_an_incomplete_backup_or_restore(self):
  source=self.folder/'source';source.write_bytes(b'complete data')
  destination=self.folder/'destination';destination.write_bytes(b'previous data')
  def partial_copy(source,target):
   target.write_bytes(b'partial');raise self.scope['InstallInterrupted']('SIGINT')
  with patch.object(shutil,'copy2',partial_copy):
   with self.assertRaises(self.scope['InstallInterrupted']):self.scope['copy_atomic'](source,destination)
  self.assertEqual(destination.read_bytes(),b'previous data');self.assertFalse(list(self.folder.glob('*.update-copy')))
 def test_requirements_on_one_filesystem_are_added_before_reserving_headroom(self):
  with self.assertRaisesRegex(RuntimeError,'Not enough free disk space'):
   self.scope['check_update_space']([('/bin',400),('/backups',500)],reserve=200,capacity=lambda path:(1,1000,path))
  self.scope['check_update_space']([('/bin',400),('/backups',500)],reserve=200,capacity=lambda path:(1,1100,path))
 def test_free_space_on_another_filesystem_cannot_pay_for_backups(self):
  def capacity(path):return (1,10000,path) if path=='/bin' else (2,300,path)
  with self.assertRaisesRegex(RuntimeError,'on /backups'):
   self.scope['check_update_space']([('/bin',400),('/backups',200)],reserve=200,capacity=capacity)
 def test_archive_estimate_counts_sparse_data_without_following_directory_links(self):
  data=self.folder/'data';data.mkdir();sparse=data/'sparse'
  with sparse.open('wb') as file:file.truncate(16*1024*1024)
  elsewhere=self.folder/'elsewhere';elsewhere.mkdir()
  with (elsewhere/'large').open('wb') as file:file.truncate(128*1024*1024)
  (data/'linked').symlink_to(elsewhere,target_is_directory=True)
  amount=self.scope['archive_bytes'](data)
  self.assertGreaterEqual(amount,16*1024*1024);self.assertLess(amount,17*1024*1024)
 def test_full_log_filesystem_does_not_abort_recovery_logging(self):
  class Full:
   def write(self,*args):raise OSError(28,'No space left on device')
   def flush(self):raise OSError(28,'No space left on device')
  self.scope['transcript']=Full();output=io.StringIO()
  with patch.object(sys,'stdout',output):self.scope['record']('continue recovery\n')
  self.assertIn('continue recovery',output.getvalue())

if __name__=='__main__':unittest.main()
