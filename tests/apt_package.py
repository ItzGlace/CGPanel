#!/usr/bin/env python3
"""Native APT/dpkg install/removal and signed metadata/hash rejection; disposable container only."""
import json,os,pathlib,shutil,subprocess,tempfile
assert os.environ.get('CGPANEL_DISPOSABLE_TEST')=='1'
repository=pathlib.Path(os.environ['CGPANEL_APT_TEST_REPOSITORY']);package=next(repository.glob('cgpanel_*_amd64.deb'));version=subprocess.check_output(['dpkg-deb','-f',str(package),'Version'],text=True).strip()
def run(*args,ok=True,**kwargs):
 result=subprocess.run(args,text=True,capture_output=True,timeout=100,**kwargs)
 assert (result.returncode==0)==ok,(args,result.stdout[-2500:],result.stderr[-2500:]);return result
with tempfile.TemporaryDirectory(prefix='cgp-apt-') as temp:
 folder=pathlib.Path(temp);folder.chmod(0o755);repo=folder/'repository';shutil.copytree(repository,repo);repo.chmod(0o755)
 sources=folder/'sources.list';sources.write_text('deb [signed-by='+str(repo/'cgpanel-archive-keyring.gpg')+'] file:'+str(repo)+' ./\n')
 options=['-o','APT::Update::Error-Mode=any','-o','Dir::Etc::sourcelist='+str(sources),'-o','Dir::Etc::sourceparts=-','-o','APT::Sandbox::User=root']
 run('apt-get',*options,'update')
 candidate=run('apt-cache',*options,'policy','cgpanel').stdout;assert 'Candidate: '+version in candidate
 run('apt-get',*options,'install','-y',str(repo/package.name))
 assert run('dpkg-query','-W','-f=${Status} ${Version}','cgpanel').stdout=='install ok installed '+version
 payload=pathlib.Path('/usr/share/cgpanel/payload');assert (payload/'target/release/cgpanel').resolve()==payload/'bin/cgpanel'
 for binary in (payload/'bin').iterdir():assert 'not found' not in run('ldd',str(binary)).stdout
 assert pathlib.Path('/etc/cgpanel/package-channel').read_text().strip()=='apt'
 run('cgpanel-setup',ok=False);run('cgpanel-setup','--dedicated-host','--public-ip','invalid',ok=False)
 assert not pathlib.Path('/etc/cgpanel/agent.env').exists(),'Invalid setup arguments changed host configuration'
 downloads=folder/'downloads';downloads.mkdir();run('apt-get',*options,'download','cgpanel',cwd=downloads)
 downloaded=downloads/package.name;assert downloaded.read_bytes()==package.read_bytes();downloaded.unlink()
 with (repo/package.name).open('ab') as f:f.write(b'corrupt')
 bad=run('apt-get',*options,'download','cgpanel',cwd=downloads,ok=False);assert 'Hash Sum mismatch' in bad.stderr+bad.stdout
 (repo/'InRelease').write_text((repo/'InRelease').read_text().replace('Origin: CGPanel','Origin: Tampered'))
 bad=run('apt-get',*options,'update',ok=False);assert 'signature' in (bad.stderr+bad.stdout).lower()
 sentinel=pathlib.Path('/srv/cgpanel/tenants/kept/site.txt');sentinel.parent.mkdir(parents=True);sentinel.write_text('customer data')
 run('apt-get','remove','-y','cgpanel');assert sentinel.read_text()=='customer data' and not payload.exists()
 print('PASS: signed APT discovery, native package installation, Ubuntu library compatibility, setup argument validation, signature/hash tamper rejection and removal preserving customer data')
