#!/usr/bin/env python3
"""Generate a flat, signed APT repository. The signing key stays outside this repository."""
import argparse,datetime,email.utils,gzip,hashlib,pathlib,shutil,subprocess
p=argparse.ArgumentParser();p.add_argument('deb',type=pathlib.Path);p.add_argument('output',type=pathlib.Path);p.add_argument('--key',required=True);a=p.parse_args()
a.output.mkdir(parents=True,exist_ok=True);deb=a.output/a.deb.name
if a.deb.resolve()!=deb.resolve():shutil.copy2(a.deb,deb)
control=subprocess.check_output(['dpkg-deb','-f',str(deb)],text=True).rstrip()
packages=control+'\nFilename: ./'+deb.name+'\nSize: '+str(deb.stat().st_size)+'\nSHA256: '+hashlib.sha256(deb.read_bytes()).hexdigest()+'\n\n'
(a.output/'Packages').write_text(packages);(a.output/'Packages.gz').write_bytes(gzip.compress(packages.encode(),mtime=0))
now=datetime.datetime.now(datetime.timezone.utc)
release='Origin: CGPanel\nLabel: CGPanel stable\nSuite: stable\nArchitectures: amd64\nDescription: CGPanel for Ubuntu 24.04 amd64\nDate: '+email.utils.format_datetime(now,usegmt=True)+'\nSHA256:\n'
for name in ['Packages','Packages.gz']:
 data=(a.output/name).read_bytes();release+=' '+hashlib.sha256(data).hexdigest()+' '+str(len(data))+' '+name+'\n'
(a.output/'Release').write_text(release)
for args in [['--clearsign','--output',str(a.output/'InRelease')],['--detach-sign','--armor','--output',str(a.output/'Release.gpg')]]:
 subprocess.run(['gpg','--batch','--yes','--local-user',a.key,*args,str(a.output/'Release')],check=True)
with (a.output/'cgpanel-archive-keyring.gpg').open('wb') as f:subprocess.run(['gpg','--export',a.key],stdout=f,check=True)
print('Signed APT metadata and package:',a.output)
