#!/usr/bin/env python3
"""Root-only, account-scoped retirement helper. Called by the broker on stdin.
The broker persists a retirement plan before any destructive phase. No arbitrary
filesystem paths, SQL identifiers, commands or credentials arrive from a client.
"""
import grp,hashlib,json,os,pathlib,pwd,re,shutil,sqlite3,subprocess,sys
from contextlib import closing

ROOT=pathlib.Path('/var/lib/cgpanel-agent');HOMES=pathlib.Path('/srv/cgpanel/tenants');VOLUMES=pathlib.Path('/var/lib/cgpanel-volumes')
def valid_id(value):return isinstance(value,str) and re.fullmatch('[a-f0-9]{32}',value) is not None
def run(*args,check=True,timeout=90,input=None):
    p=subprocess.run(args,capture_output=True,text=True,timeout=timeout,input=input)
    if check and p.returncode:raise RuntimeError(args[0]+' failed: '+p.stderr[-1200:])
    return p
def inside(path,parent):
    try:pathlib.Path(path).relative_to(parent);return True
    except ValueError:return False
def mounts():
    value=json.loads(run('findmnt','--json','--list','-o','TARGET,SOURCE').stdout)
    return value.get('filesystems',[])
def plan(tenant):
    if not valid_id(tenant):raise ValueError('Invalid tenant ID')
    registry=json.loads((ROOT/'registry.json').read_text());marker=registry['items'].get('retire_'+tenant,{})
    if marker.get('data',{}).get('state')=='completed':return {'completed':True,'purged':True,'tenant':tenant,'public':{**marker['data'].get('public',{}),'completed':True,'blockers':[]}}
    with closing(sqlite3.connect('file:/var/lib/cgpanel/panel.db?mode=ro',uri=True)) as db:
        row=db.execute('SELECT username,role FROM users WHERE id=?',(tenant,)).fetchone()
        if row is None or row[1]!='user':raise ValueError('Only an existing non-administrator account can be deleted')
        resources=db.execute('SELECT id,kind,name,data FROM resources WHERE owner=?',(tenant,)).fetchall()
        jobs=[r[0] for r in db.execute('SELECT id FROM jobs WHERE owner=?',(tenant,)) if valid_id(r[0])]
    inventory=marker.get('data',{}).get('plan',{}).get('inventory')
    if inventory is None:
        inventory={key:item for key,item in registry['items'].items() if item.get('tenant')==tenant and not key.startswith('retire_')}
        for key,kind,name,data in resources:
            existing=registry['items'].get(key)
            if existing and existing.get('tenant')!=tenant:raise ValueError('Resource ownership disagrees between the panel and broker')
            inventory.setdefault(key,{'tenant':tenant,'kind':kind,'data':{**json.loads(data),'name':name}})
    if any(item.get('kind')=='mail_settings' for item in inventory.values()):raise ValueError('This account owns server-wide mail settings')
    blockers=[];keys=set(inventory)
    for key,item in registry['items'].items():
        if item.get('tenant')==tenant:continue
        for field in ('app_id','domain_id','integration_id','proxy_id','certificate_domain','database_ids'):
            values=item.get('data',{}).get(field,[]);values=values if isinstance(values,list) else [values]
            if any(isinstance(v,str) and v in keys for v in values):blockers.append('Another account or server setting references '+key+' ('+field+')')
    domains=[(key,item['data']['name']) for key,item in inventory.items() if item['kind']=='domains']
    if any(not isinstance(name,str) or len(name)>253 or len(name.split('.'))<2 or any(not re.fullmatch(r'[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?',label) for label in name.split('.')) for _,name in domains):raise ValueError('Invalid stored domain name')
    for _,name in domains:
        for other in registry['items'].values():
            hostname=other.get('data',{}).get('hostname','')
            if other.get('tenant')!=tenant and other.get('kind')=='mail_settings' and other.get('data',{}).get('enabled') and (hostname==name or hostname.endswith('.'+name)):
                blockers.append('The server-wide mail hostname depends on '+name)
        for other in registry['items'].values():
            foreign=other.get('data',{}).get('name','')
            if other.get('tenant')!=tenant and other.get('kind')=='domains' and foreign.endswith('.'+name):blockers.append('Another account owns a subdomain of '+name)
    home=HOMES/tenant;os_name='cg_'+tenant[:20]
    try:account=pwd.getpwnam(os_name)
    except KeyError:account=None
    if account and (account.pw_uid<1000 or account.pw_gid<1000 or account.pw_dir!=str(home) or grp.getgrgid(account.pw_gid).gr_name!=os_name):blockers.append('Operating-system account is not an isolated CGPanel tenant')
    if account and any(entry.pw_uid==account.pw_uid and entry.pw_name!=os_name for entry in pwd.getpwall()):blockers.append('Another operating-system account shares the tenant UID')
    if home.is_symlink():blockers.append('Tenant home is a symbolic link; inspect it before deletion')
    apps={key for key,item in inventory.items() if item['kind']=='apps'};volumes=[]
    for file in VOLUMES.glob('*.json'):
        value=json.loads(file.read_text())
        if value.get('tenant')!=tenant:continue
        app=value.get('app')
        if not valid_id(app) or file.stem!=app or value.get('path')!=str(home/'apps'/app):raise ValueError('Invalid owned volume record')
        if registry['items'].get(app,{}).get('tenant',tenant)!=tenant:raise ValueError('Volume ownership conflicts with another account')
        apps.add(app);volumes.append(value)
    if any(not valid_id(key) for key in apps|{key for key,item in inventory.items() if item['kind'] in ('domains','databases','mailboxes','backups','full_backups')}):raise ValueError('Invalid resource identity')
    fstab=pathlib.Path('/etc/fstab').read_text();binds=[]
    for line in fstab.splitlines():
        parts=line.split()
        if len(parts)<4 or line.lstrip().startswith('#'):continue
        source,target=map(pathlib.Path,parts[:2])
        if inside(source,home/'apps') and 'bind' in parts[3].split(','):
            app=source.name
            if not valid_id(app) or source!=home/'apps'/app or not str(target).startswith('/srv/sftp/') or '..' in target.parts:raise ValueError('Unexpected transfer bind mount')
            if registry['items'].get(app,{}).get('tenant',tenant)!=tenant:raise ValueError('Transfer mount conflicts with another account')
            apps.add(app);binds.append({'source':str(source),'target':str(target)})
    allowed_mounts={str(home/'apps'/app) for app in apps}|{b['target'] for b in binds}
    for mount in mounts():
        target=mount['target']
        if inside(target,home) and target not in allowed_mounts:blockers.append('Unexpected mount inside tenant storage: '+target)
    for bind in binds:
        if os.path.ismount(bind['target']):
            a=os.stat(bind['source']);b=os.stat(bind['target'])
            if (a.st_dev,a.st_ino)!=(b.st_dev,b.st_ino):blockers.append('Transfer mount no longer matches its workspace: '+bind['target'])
    transfers=[]
    if account:
        named={item['data'].get('transfer_user') for item in inventory.values() if item['kind']=='apps'}
        for entry in pwd.getpwall():
            if entry.pw_gid!=account.pw_gid or entry.pw_name==os_name:continue
            expected=entry.pw_name in named or any(entry.pw_name=='cgsftp_'+app[:16] for app in apps)
            if not expected or entry.pw_uid<1000:blockers.append('Unrecognized OS account shares tenant group: '+entry.pw_name)
            elif any(other.pw_uid==entry.pw_uid and other.pw_name!=entry.pw_name for other in pwd.getpwall()):blockers.append('Another operating-system account shares a transfer UID')
            else:transfers.append(entry.pw_name)
    for app in apps:
        file=VOLUMES/(app+'.json')
        if file.exists() and json.loads(file.read_text()).get('tenant')!=tenant:blockers.append('A volume belongs to another account: '+app)
    for key,name in domains:
        for cert in ('cgp_'+key,'cgp_'+key+'_staging'):
            needle='/etc/letsencrypt/live/'+cert+'/'
            for file in pathlib.Path('/etc/nginx').rglob('*.conf'):
                if file.name=='cgp_'+key+'.conf':continue
                try:
                    if needle in file.read_text():blockers.append('A shared Nginx configuration uses this certificate: '+str(file))
                except (OSError,UnicodeError):pass
    private={'tenant':tenant,'username':row[0],'inventory':inventory,'apps':sorted(apps),'volumes':volumes,'binds':binds,'transfers':transfers,'jobs':jobs,'os_identity':{'uid':account.pw_uid,'gid':account.pw_gid} if account else None}
    previous=marker.get('data',{}).get('plan')
    if previous:
        private={key:value for key,value in previous.items() if key!='public'}
    fingerprint=hashlib.sha256(json.dumps(private,sort_keys=True,separators=(',',':')).encode()).hexdigest()
    counts={}
    for item in inventory.values():counts[item['kind']]=counts.get(item['kind'],0)+1
    private['public']={'fingerprint':fingerprint,'blockers':sorted(set(blockers)),'counts':counts,'home':str(home),'volume_mb':sum(v['size_mb'] for v in volumes),'domains':[name for _,name in domains],'scope':'Account, local applications/container images, workspaces and recovery copies, databases, mail, schedules, integrations and local backups. External backups and server-wide recovery snapshots are retained.'}
    return private

def remove(path):
    path=pathlib.Path(path)
    if path.is_symlink() or path.is_file():path.unlink()
    elif path.exists():
        if not shutil.rmtree.avoids_symlink_attacks:raise RuntimeError('Safe directory removal is unavailable')
        if any(inside(m['target'],path) for m in mounts()):raise RuntimeError('Refusing to erase mounted storage: '+str(path))
        shutil.rmtree(path)

def detach(p):
    tenant=p['tenant'];home=HOMES/tenant;os_name='cg_'+tenant[:20]
    try:account=pwd.getpwnam(os_name)
    except KeyError:account=None
    if account:
        uid=account.pw_uid
        env=['runuser','-u',os_name,'--','env','HOME='+str(home),'XDG_RUNTIME_DIR=/run/user/'+str(uid),'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/'+str(uid)+'/bus','podman']
        # The OS user is dedicated to this retiring account. Preserve every other
        # UID's containers, image store, volumes and services.
        probe=run(*env,'info','--format','{{.Store.GraphRoot}}',check=False,timeout=30)
        if probe.returncode==0:
            graph=pathlib.Path(probe.stdout.strip())
            if not inside(graph,home) or not inside(graph.resolve(),home.resolve()):raise RuntimeError('Container storage is shared outside the tenant home')
            run(*env,'rm','--all','--force',timeout=120)
        elif (home/'.local/share/containers').exists():raise RuntimeError('Cannot inspect the tenant container store; no data was erased')
        run('loginctl','disable-linger',os_name,check=False);run('loginctl','terminate-user',os_name,check=False)
        run('pkill','-KILL','-u',str(uid),check=False)
    for name in p['transfers']:
        try:entry=pwd.getpwnam(name)
        except KeyError:continue
        run('pkill','-KILL','-u',str(entry.pw_uid),check=False)
    for bind in p['binds']:
        if os.path.ismount(bind['target']):run('umount',bind['target'])
    for app in p['apps']:
        folder=home/'apps'/app;image=VOLUMES/(app+'.img')
        if os.path.ismount(folder):
            device=run('findmnt','-n','-o','SOURCE','--mountpoint',str(folder)).stdout.strip()
            backing=run('losetup','-n','-O','BACK-FILE',device).stdout.strip()
            if pathlib.Path(backing)!=image:raise RuntimeError('Unexpected workspace mount source')
            run('umount',str(folder));run('losetup','-d',device,check=False)
        attached=run('losetup','-j',str(image)).stdout.strip()
        if attached:raise RuntimeError('Workspace image is still attached; inspect it before deletion')
    fstab=pathlib.Path('/etc/fstab');kept=[]
    sources={str(home/'apps'/app) for app in p['apps']}|{str(VOLUMES/(app+'.img')) for app in p['apps']}
    for line in fstab.read_text().splitlines():
        parts=line.split()
        if not line.lstrip().startswith('#') and len(parts)>=2 and parts[0] in sources:continue
        kept.append(line)
    temp=fstab.with_name('fstab.cgpanel-new');temp.write_text('\n'.join(kept)+'\n');temp.chmod(fstab.stat().st_mode&0o777);temp.replace(fstab)
    # Remove only SSH profiles for verified transfer users. Include directives
    # become harmless glob includes, so a later sshd reload remains valid.
    for base in ['/etc/cgpanel/sftp','/etc/ssh/sshd_config.d']:
        for file in pathlib.Path(base).glob('*.conf'):
            if file.is_symlink():continue
            matches=re.findall(r'^Match User ([a-z_][a-z0-9_-]{0,31})\s*$',file.read_text(),re.M)
            if len(matches)==1 and matches[0] in p['transfers']:
                main=pathlib.Path('/etc/ssh/sshd_config');text=main.read_text();text=re.sub(r'^Include '+re.escape(str(file))+r'\s*$',lambda m:'Include '+str(file)+'*',text,flags=re.M);main.write_text(text);file.unlink()
    for name in p['transfers']:
        remove(pathlib.Path('/etc/cgpanel/ftps-users')/name)
        try:pwd.getpwnam(name)
        except KeyError:continue
        run('userdel',name)
    for key,item in p['inventory'].items():
        if item['kind']=='domains':
            for path in [f'/etc/nginx/conf.d/cgp_{key}.conf',f'/etc/bind/cgpanel/{key}.zone',f'/etc/cgpanel/acme/{key}.ini',str(ROOT/'challenges'/(key+'.json'))]:remove(path)
        elif item['kind']=='databases':
            name='cgp_'+key[:20]
            if item['data'].get('engine')=='mysql':
                # Account host values are validated IP literals/networks by the broker.
                hosts=run('mariadb','--batch','--skip-column-names',input="SELECT Host FROM mysql.user WHERE User='"+name+"';").stdout.splitlines()
                for host in hosts:
                    if not re.fullmatch(r'[a-zA-Z0-9.:/%_-]+',host):raise ValueError('Invalid stored database host')
                sql='DROP DATABASE IF EXISTS `'+name+'`;'+''.join("DROP USER IF EXISTS '"+name+"'@'"+host+"';" for host in hosts)
                run('mariadb','--batch',input=sql)
            else:run('runuser','-u','postgres','--','psql','-X','-v','ON_ERROR_STOP=1',input=f'DROP DATABASE IF EXISTS {name} WITH (FORCE); DROP ROLE IF EXISTS {name};')
    for app in p['apps']:remove('/etc/nginx/cgpanel-ide/'+app+'.conf')
    run('systemctl','daemon-reload');run('sshd','-t');run('systemctl','reload','ssh');run('nginx','-t');run('systemctl','reload','nginx')
    for key,item in p['inventory'].items():
        if item['kind']!='domains':continue
        for cert in ('cgp_'+key,'cgp_'+key+'_staging'):
            if pathlib.Path('/etc/letsencrypt/renewal/'+cert+'.conf').exists():
                tool='/opt/cgpanel-tools/certbot/bin/certbot'
                if not pathlib.Path(tool).exists():raise RuntimeError('Certbot is required to retire a managed certificate')
                run(tool,'delete','--cert-name',cert,'--non-interactive')
        for suffix in ('.key','.txt'):remove('/var/lib/rspamd/dkim/'+item['data']['name']+suffix)
    return {'detached':True}

def erase(p):
    tenant=p['tenant'];home=HOMES/tenant
    remove(home);remove('/var/lib/cgpanel-mail/'+tenant)
    for app in p['apps']:
        remove(VOLUMES/(app+'.img'));remove(VOLUMES/(app+'.json'))
    for key,item in p['inventory'].items():
        if item['kind']=='domains':
            remove('/srv/cgpanel/public/'+key);remove('/srv/cgpanel/policies/'+key)
        if item['kind'] in ('backups','full_backups'):
            for suffix in ('.cgp','.tar.gz'):remove(ROOT/'backups'/(key+suffix))
            remove('/var/lib/cgpanel-exports/'+key+'.cgp')
    for job in p['jobs']:
        remove(ROOT/'staging'/job);remove(ROOT/'restore'/job)
    for bind in p['binds']:
        target=pathlib.Path(bind['target'])
        if target.exists() and not any(target.iterdir()):target.rmdir()
        root=pathlib.Path(bind['target']).parent
        if root.exists() and not any(root.iterdir()):root.rmdir()
    os_name='cg_'+tenant[:20]
    try:pwd.getpwnam(os_name)
    except KeyError:pass
    else:run('userdel',os_name)
    for base in ('/var/mail','/var/spool/mail'):remove(pathlib.Path(base)/os_name)
    try:grp.getgrnam(os_name)
    except KeyError:pass
    else:run('groupdel',os_name)
    return {'purged':True}

if __name__=='__main__':
    try:
        if os.geteuid()!=0:raise ValueError('Host root is required')
        value=json.load(sys.stdin);tenant=value['tenant'];mode=value['mode'];p=plan(tenant)
        if p.get('completed'):result=p
        elif mode=='plan':result=p
        elif p['public']['blockers']:raise ValueError('; '.join(p['public']['blockers']))
        elif mode=='detach':
            with closing(sqlite3.connect('file:/var/lib/cgpanel/panel.db?mode=ro',uri=True)) as db:
                if db.execute('SELECT enabled FROM users WHERE id=?',(tenant,)).fetchone()!=(0,):raise ValueError('Suspend the account before erasing its data')
            result=detach(p)
        elif mode=='erase':result=erase(p)
        else:raise ValueError('Choose a retirement phase')
        print(json.dumps(result))
    except Exception as error:print(str(error)[:1800],file=sys.stderr);sys.exit(1)
