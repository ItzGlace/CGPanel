#!/usr/bin/env python3
"""Installer transport fallback and integrity checks without installed host changes."""
import hashlib, importlib.util, io, pathlib, tempfile, unittest, urllib.error, zipfile
root=pathlib.Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('rclone_installer',root/'deploy/install-rclone.py')
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)

class Tests(unittest.TestCase):
 def setUp(self):
  self.directory=tempfile.TemporaryDirectory();self.addCleanup(self.directory.cleanup)
  self.folder=pathlib.Path(self.directory.name);self.target=self.folder/'rclone';self.target.write_bytes(b'previous installation')
  archive=io.BytesIO()
  with zipfile.ZipFile(archive,'w') as z:z.writestr('rclone-v1.75.1-linux-amd64/rclone',b'verified test executable')
  self.data=archive.getvalue();self.digest=hashlib.sha256(self.data).hexdigest()
 def test_primary_403_uses_github_and_validates(self):
  seen=[]
  def fetch(url):
   seen.append(url)
   if len(seen)==1:raise urllib.error.HTTPError(url,403,'Forbidden',{},None)
   return self.data
  module.install('amd64',target=self.target,fetcher=fetch,expected=self.digest)
  self.assertEqual(self.target.read_bytes(),b'verified test executable')
  self.assertEqual(self.target.stat().st_mode&0o777,0o755)
  self.assertEqual(len(seen),2);self.assertIn('github.com/rclone/rclone/releases/download/v1.75.1/',seen[1])
 def test_checksum_failure_never_overwrites_or_falls_back(self):
  seen=[]
  def fetch(url):seen.append(url);return b'corrupted download'
  with self.assertRaisesRegex(ValueError,'checksum mismatch'):module.install('amd64',target=self.target,fetcher=fetch,expected=self.digest)
  self.assertEqual(len(seen),1);self.assertEqual(self.target.read_bytes(),b'previous installation')
 def test_bundled_archive_requires_no_network(self):
  archive=self.folder/'bundle.zip';archive.write_bytes(self.data)
  def fetch(url):raise AssertionError('Bundled installation made a network request')
  module.install('amd64',target=self.target,bundled=archive,fetcher=fetch,expected=self.digest)
  self.assertEqual(self.target.read_bytes(),b'verified test executable')
 def test_bad_bundle_keeps_installed_binary(self):
  archive=self.folder/'bundle.zip';archive.write_bytes(b'corrupted bundle')
  with self.assertRaisesRegex(ValueError,'checksum mismatch'):module.install('amd64',target=self.target,bundled=archive,expected=self.digest)
  self.assertEqual(self.target.read_bytes(),b'previous installation')
 def test_all_sources_unavailable_preserves_binary(self):
  def fetch(url):raise urllib.error.URLError('unavailable')
  with self.assertRaisesRegex(RuntimeError,'any configured source'):module.install('amd64',target=self.target,fetcher=fetch)
  self.assertEqual(self.target.read_bytes(),b'previous installation')
 def test_wrong_archive_member_preserves_binary(self):
  data=io.BytesIO()
  with zipfile.ZipFile(data,'w') as z:z.writestr('../rclone',b'not extracted')
  blob=data.getvalue()
  with self.assertRaises(KeyError):module.install('amd64',target=self.target,fetcher=lambda _:blob,expected=hashlib.sha256(blob).hexdigest())
  self.assertEqual(self.target.read_bytes(),b'previous installation')

if __name__=='__main__':unittest.main()
