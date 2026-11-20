#!/usr/bin/env python3
"""Verify subprocess diagnostics without executing the system updater's main flow."""
import ast,pathlib,re,subprocess,sys,tempfile,time,unittest
from unittest.mock import patch
root=pathlib.Path(__file__).resolve().parents[1]
source=ast.parse((root/'deploy/updater.py').read_text());functions=ast.Module(body=[n for n in source.body if isinstance(n,ast.FunctionDef) and n.name in ('run','safe_error')],type_ignores=[])

class Tests(unittest.TestCase):
 def setUp(self):
  self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup);self.folder=pathlib.Path(self.temp.name)
  self.scope={'ROOT':self.folder,'re':re,'subprocess':subprocess,'time':time};exec(compile(functions,'updater-functions','exec'),self.scope)
 def test_actual_stderr_reaches_status_error_and_root_log(self):
  with self.assertRaisesRegex(RuntimeError,'package index is locked'):
   self.scope['run'](sys.executable,'-c',"import sys;print('package index is locked',file=sys.stderr);sys.exit(7)")
  log=self.folder/'last-update.log';self.assertIn('package index is locked',log.read_text());self.assertEqual(log.stat().st_mode&0o777,0o600)
 def test_success_preserves_output_and_status(self):
  result=self.scope['run'](sys.executable,'-c',"print('fixture installed')");self.assertEqual(result.returncode,0);self.assertIn('fixture installed',result.stdout)
 def test_private_url_credentials_are_redacted_from_ui_error(self):
  with self.assertRaises(RuntimeError) as raised:self.scope['run'](sys.executable,'-c',"import sys;print('https://user:privatepass@repo.example/path?token=secret',file=sys.stderr);sys.exit(1)")
  self.assertNotIn('privatepass',str(raised.exception));self.assertNotIn('token=secret',str(raised.exception))
 def test_timeout_output_is_retained(self):
  with patch.object(subprocess,'run',side_effect=subprocess.TimeoutExpired(['fixture'],1,output=b'last progress\n',stderr=b'fixture timeout\n')):
   with self.assertRaisesRegex(RuntimeError,'timed out'):self.scope['run']('fixture')
  self.assertIn('last progress', (self.folder/'last-update.log').read_text())

if __name__=='__main__':unittest.main()
