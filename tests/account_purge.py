#!/usr/bin/env python3
"""Native file/database account removal in a disposable container; service/mount actions adapted."""
import importlib.util,json,os,pathlib,pwd,sqlite3,subprocess
assert os.environ.get('CGPANEL_DISPOSABLE_TEST')=='1'
root=pathlib.Path('/project');spec=importlib.util.spec_from_file_location('purge',root/'deploy/account-purge.py');purge=importlib.util.module_from_spec(spec);spec.loader.exec_module(purge)
uids=['a'*32,'b'*32];apps=['c'*32,'d'*32];database='e'*32
for uid,app in zip(uids,apps):
 home=purge.HOMES/uid;home.mkdir(parents=True);subprocess.run(['useradd','-M','-U','-d',str(home),'cg_'+uid[:20]],check=True)
 for folder in ['apps/'+app,'apps/'+app+'.pre-volume-recovery','.local/share/containers']:(home/folder).mkdir(parents=True)
 (home/'apps'/app/'site.txt').write_text('active site');(home/'apps'/(app+'.pre-volume-recovery')/'old.txt').write_text('recovery data')
 (home/'external-link').symlink_to(purge.HOMES/uids[1]/'apps'/apps[1],target_is_directory=True)
 mail=pathlib.Path('/var/lib/cgpanel-mail')/uid;mail.mkdir(parents=True);(mail/'message').write_text('mail')
purge.ROOT.mkdir(parents=True);purge.VOLUMES.mkdir(parents=True)
for uid,app in zip(uids,apps):
 (purge.VOLUMES/(app+'.img')).write_bytes(b'workspace volume')
 (purge.VOLUMES/(app+'.json')).write_text(json.dumps({'app':app,'tenant':uid,'path':str(purge.HOMES/uid/'apps'/app),'size_mb':1024}))
pathlib.Path('/var/lib/cgpanel').mkdir(exist_ok=True)
with sqlite3.connect('/var/lib/cgpanel/panel.db') as db:
 db.executescript((root/'src/schema.sql').read_text())
 for uid in uids:db.execute("INSERT INTO users(id,username,password,role) VALUES(?,?,?,'user')",(uid,'user'+uid[:1],'dummy'))
 db.execute("INSERT INTO users(id,username,password,role) VALUES(?,'admin','dummy','admin')",('f'*32,))
items={app:{'tenant':uid,'kind':'apps','data':{}} for uid,app in zip(uids,apps)}
registry=purge.ROOT/'registry.json';registry.write_text(json.dumps({'items':items}))
try:purge.plan('f'*32);raise AssertionError('administrator accepted')
except ValueError:pass
# Root-owned records are authoritative; a tenant-created filename cannot claim another volume.
(purge.HOMES/uids[0]/'apps'/apps[1]).mkdir()
p=purge.plan(uids[0]);assert p['apps']==[apps[0]] and not p['public']['blockers'],p
primary=pwd.getpwnam('cg_'+uids[0][:20]);subprocess.run(['useradd','-o','-u',str(primary.pw_uid),'-M','foreign_shared_uid'],check=True)
assert any('shares the tenant UID' in b for b in purge.plan(uids[0])['public']['blockers'])
subprocess.run(['userdel','foreign_shared_uid'],check=True)
# A server mail hostname remains a dependency even without a domain-ID certificate reference.
items['9'*32]={'tenant':uids[0],'kind':'domains','data':{'name':'mail-host.example'}}
items['mail_settings']={'tenant':uids[1],'kind':'mail_settings','data':{'enabled':True,'hostname':'mail.mail-host.example'}}
registry.write_text(json.dumps({'items':items}));assert any('mail hostname' in b for b in purge.plan(uids[0])['public']['blockers'])
del items['9'*32];del items['mail_settings'];registry.write_text(json.dumps({'items':items}))

# Shared group members and foreign resource dependencies block deletion.
foreign=items[apps[1]];foreign['data']['app_id']=apps[0];registry.write_text(json.dumps({'items':items}));assert purge.plan(uids[0])['public']['blockers'];del foreign['data']['app_id']
items={apps[1]:foreign,'retire_'+uids[0]:{'tenant':uids[0],'kind':'retiring_tenant','data':{'state':'erasing','plan':p}}};registry.write_text(json.dumps({'items':items}))
assert purge.plan(uids[0])['public']['fingerprint']==p['public']['fingerprint']
# Adapt unavailable systemd/Podman and loop-device inspection. Real filesystem erasure,
# UNIX account removal and foreign-account preservation run without mocks.
original=purge.run
def adapted(*args,**kw):
 if args[0] in ('systemctl','loginctl','nginx','sshd','pkill'):return subprocess.CompletedProcess(args,0,'','')
 if args[0]=='losetup':return subprocess.CompletedProcess(args,0,'','')
 if args[0]=='runuser' and 'podman' in args:return subprocess.CompletedProcess(args,0,str(purge.HOMES/uids[0]/'.local/share/containers')+'\n','')
 return original(*args,**kw)
purge.run=adapted
purge.detach(purge.plan(uids[0]));purge.erase(purge.plan(uids[0]));purge.erase(purge.plan(uids[0]))
assert not (purge.HOMES/uids[0]).exists();assert not (purge.VOLUMES/(apps[0]+'.img')).exists()
assert (purge.HOMES/uids[1]/'apps'/apps[1]/'site.txt').read_text()=='active site'
assert (purge.VOLUMES/(apps[1]+'.img')).exists() and pathlib.Path('/var/lib/cgpanel-mail',uids[1],'message').exists()
try:pwd.getpwnam('cg_'+uids[0][:20]);raise AssertionError('OS user retained')
except KeyError:pass
pwd.getpwnam('cg_'+uids[1][:20]);print('PASS: permanent account files/mail/volumes/OS identity removal, recovery copies, safe symlinks, foreign ownership blockers and repeatable cleanup; service and mount operations adapted')
