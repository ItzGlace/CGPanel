"""Regression coverage for nft's canonical semicolon-free output."""
import ast,pathlib,re,unittest
source=pathlib.Path(__file__).resolve().parents[1]/'deploy/setup-v0.5.py'
node=next(n for n in ast.parse(source.read_text()).body if isinstance(n,ast.FunctionDef) and n.name=='firewall_upgrade')
exec(compile(ast.Module(body=[node],type_ignores=[]),str(source),'exec'))
class FirewallMigration(unittest.TestCase):
 def test_live_and_saved_forms(self):
  for separator in [';','\n']:
   original='table inet cgpanel {\n'+''.join('set '+name+' { type '+kind+separator+' elements = { '+address+' }\n}\n' for name,kind,address in [('database4','ipv4_addr','194.77.220.139'),('database6','ipv6_addr','::1'),('blocked4','ipv4_addr','192.0.2.1'),('blocked6','ipv6_addr','2001:db8::1')])+'chain input { tcp dport 22 accept\n}\n}'
   updated=firewall_upgrade(original)
   for name in ['database4','database6','blocked4','blocked6']:
    self.assertIn('flags interval',re.search(r'set '+name+r'\s*\{([^}]*)',updated,re.S)[1])
   self.assertIn('194.77.220.139',updated)
   self.assertEqual(updated,firewall_upgrade(updated))
if __name__=='__main__':unittest.main()
