#!/usr/bin/env python3
"""APT lock regressions: real POSIX locks, race retries and failure diagnostics."""
import importlib.util,pathlib,subprocess,sys,tempfile,time,unittest
from unittest.mock import patch
root=pathlib.Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('cgp_package_manager',root/'deploy/package_manager.py');manager=importlib.util.module_from_spec(spec);spec.loader.exec_module(manager)

class Tests(unittest.TestCase):
 def holder(self,path):
  code="import fcntl,sys,time; f=open(sys.argv[1],'a+');fcntl.lockf(f,fcntl.LOCK_EX);print('locked',flush=True);time.sleep(30)"
  p=subprocess.Popen([sys.executable,'-c',code,str(path)],stdout=subprocess.PIPE,text=True);self.assertEqual(p.stdout.readline().strip(),'locked');self.addCleanup(lambda:(p.terminate(),p.wait(),p.stdout.close()));return p
 def test_index_lock_blocks_all_package_commands(self):
  with tempfile.TemporaryDirectory() as folder:
   path=pathlib.Path(folder)/'lock';path.write_text('sentinel');inode=path.stat().st_ino;process=self.holder(path);calls=[]
   def runner(*args,**kw):calls.append(args);return subprocess.CompletedProcess(args,0,'')
   with self.assertRaisesRegex(TimeoutError,'still busy'):manager.install_packages(['python3-pymysql'],timeout=.1,locks=[path],poll=.02,runner=runner,log=lambda *a,**k:None)
   self.assertEqual(calls,[]);self.assertIsNone(process.poll());self.assertEqual(path.read_text(),'sentinel');self.assertEqual(path.stat().st_ino,inode)
 def test_lock_release_allows_update_then_install(self):
  with tempfile.TemporaryDirectory() as folder:
   path=pathlib.Path(folder)/'lock';process=self.holder(path);calls=[]
   import threading
   release=threading.Timer(.1,process.terminate);release.start()
   try:manager.install_packages(['python3-pymysql'],timeout=2,locks=[path],poll=.02,runner=lambda args,**kw:(calls.append(args) or subprocess.CompletedProcess(args,0,'')),log=lambda *a,**k:None)
   finally:release.cancel()
   self.assertEqual([call[3] for call in calls],['update','install'])
   self.assertEqual(calls[-1][-1],'python3-pymysql')
 def test_lock_race_is_retried(self):
  answers=[(100,'E: Could not get lock /var/lib/apt/lists/lock. It is held by process 123\n'),(0,'updated\n'),(0,'installed\n')];calls=[]
  def runner(args,**kw):calls.append(args);code,out=answers.pop(0);return subprocess.CompletedProcess(args,code,out)
  manager.install_packages(['driver'],timeout=1,locks=[],poll=.01,runner=runner,log=lambda *a,**k:None)
  self.assertEqual([call[3] for call in calls],['update','update','install'])
 def test_signature_or_repository_errors_are_not_retried(self):
  calls=[]
  def runner(args,**kw):calls.append(args);return subprocess.CompletedProcess(args,100,'E: The repository is not signed\n')
  with self.assertRaisesRegex(RuntimeError,'not signed'):manager.install_packages(['driver'],locks=[],runner=runner,log=lambda *a,**k:None)
  self.assertEqual(len(calls),1)
 def test_no_packages_does_not_run_apt(self):
  manager.install_packages([],runner=lambda *a,**k:self.fail('APT should not run'))

if __name__=='__main__':unittest.main()
