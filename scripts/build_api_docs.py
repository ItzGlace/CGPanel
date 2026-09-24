#!/usr/bin/env python3
"""Generate the OpenAPI contract and readable endpoint reference; no dependencies."""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = re.search(r'^version = "([^"]+)"', (ROOT / 'Cargo.toml').read_text(), re.M)[1]
S = {'type': 'string'}
I = {'type': 'integer'}
B = {'type': 'boolean'}
O = {'type': 'object', 'additionalProperties': True}
IDS = {'type': 'array', 'items': S}
def enum(*items): return {'type': 'string', 'enum': list(items)}
def obj(properties, required=()):
    return {'type': 'object', 'properties': properties, 'required': list(required)} if required else {'type': 'object', 'properties': properties}
def ref(name): return {'$ref': '#/components/schemas/' + name}
def array(schema): return {'type': 'array', 'items': schema}
def query(name, description, schema=S): return {'name': name, 'in': 'query', 'description': description, 'schema': schema}

schemas = {
    'Error': obj({'error': S}, ['error']),
    'Login': obj({'username': S, 'password': {'type': 'string', 'format': 'password'}}, ['username', 'password']),
    'TokenCreate': obj({'name': {'type': 'string', 'minLength': 1, 'maxLength': 64}, 'scope': enum('read', 'admin'), 'expires_days': {'type': 'integer', 'minimum': 1, 'maximum': 90, 'default': 30}, 'allowed_ips': IDS}, ['name']),
    'TokenMetadata': obj({'id': S, 'name': S, 'prefix': S, 'scope': enum('read', 'admin'), 'allowed_ips': IDS, 'created': I, 'expires': I, 'last_used': {'type': 'integer', 'nullable': True}, 'revoked': {'type': 'integer', 'nullable': True}}),
    'TokenSecret': {'allOf': [ref('TokenMetadata'), obj({'token': {'type': 'string', 'description': 'Returned only on creation. Store privately.'}}, ['token'])]},
    'UserCreate': obj({'username': {'type': 'string', 'pattern': '^[a-z0-9_]{3,32}$'}, 'password': {'type': 'string', 'format': 'password', 'minLength': 14, 'maxLength': 256}, 'quota': I, 'allowed_ips': IDS}, ['username', 'password']),
    'UserUpdate': obj({'enabled': B, 'quota': I, 'allowed_ips': IDS, 'password': {'type': 'string', 'format': 'password'}}),
    'User': obj({'id': S, 'username': S, 'role': enum('admin', 'user'), 'enabled': {'type': 'integer', 'enum': [0, 1]}, 'quota': I, 'allowed_ips': IDS, 'created': S}),
    'Resource': obj({'id': S, 'owner': S, 'kind': S, 'name': S, 'data': O, 'created': S}),
    'ResourceCreated': obj({'id': S, 'result': O}),
    'Identity': obj({'id': S, 'username': S, 'role': enum('admin', 'user'), 'csrf': {'type': 'string', 'description': 'Empty for bearer authentication.'}}),
    'System': obj({'version': S, 'counts': O, 'events': array(O), 'host': O}),
    'Cron': obj({'schedule': S, 'timezone': S}, ['schedule']),
    'Monitor': obj({'enabled': B, 'scheme': enum('http', 'https'), 'path': S, 'interval': I, 'telegram_id': S, 'analytics': B, 'clicks': B, 'retention_days': I}, ['scheme', 'path']),
    'BackupPlan': obj({'app_id': S, 'database_ids': IDS, 'destinations': IDS, 'schedule': S, 'timezone': S, 'quiesce': B, 'retention': I, 'enabled': B}, ['app_id', 'schedule']),
    'Job': obj({'id': S, 'owner': S, 'kind': S, 'target': S, 'status': enum('queued', 'running', 'succeeded', 'failed', 'interrupted'), 'result': O, 'error': S, 'created': I, 'started': {'type': 'integer', 'nullable': True}, 'finished': {'type': 'integer', 'nullable': True}}),
    'Queued': obj({'id': S, 'status': {'type': 'string', 'enum': ['queued']}}),
}
resources = {
    'apps': ('Application', {'runtime': enum('python','php','node','java','rust','static'), 'mode': enum('web','worker'), 'command': S, 'env': {'type': 'object', 'additionalProperties': S}}, ['runtime','mode'], {'name':'website','runtime':'php','mode':'web','command':'','env':{}}),
    'domains': ('Domain', {'app_id': S}, [], {'name':'example.com','app_id':'APP_ID'}),
    'databases': ('Database', {'engine': enum('mysql','postgresql'), 'allowed_ips': IDS}, ['engine'], {'name':'website','engine':'mysql','allowed_ips':[]}),
    'dns': ('DnsRecord', {'domain_id':S,'type':enum('A','AAAA','CNAME','MX','TXT','NS'),'value':S,'ttl':I}, ['domain_id','type','value'], {'name':'www','domain_id':'DOMAIN_ID','type':'A','value':'203.0.113.20','ttl':300}),
    'schedules': ('Schedule', {'app_id':S,'schedule':S,'timezone':S,'command':S}, ['app_id','schedule','command'], {'name':'cleanup','app_id':'APP_ID','schedule':'0 3 * * *','timezone':'UTC','command':'python cleanup.py'}),
    'backups': ('WorkspaceBackup', {'app_id':S}, ['app_id'], {'name':'snapshot','app_id':'APP_ID'}),
    'blocks': ('IpBlock', {}, [], {'name':'203.0.113.55'}),
}
for kind,(name,fields,required,example) in resources.items():
    schemas[name+'Create']=obj({'owner':S,'name':S,**fields},['name',*required])
schemas['ResourceCreate']={'anyOf':[ref(v[0]+'Create') for v in resources.values()], 'description':'Choose the schema matching the kind path parameter. owner is optional for administrators; defaults to caller.'}

integration_fields = {
    'proxy': ({'url':S}, ['url'], {'url':'socks5h://proxy.example.com:1080'}),
    'telegram': ({'token':S,'chat_id':S}, ['token','chat_id'], {'token':'BOT_TOKEN','chat_id':'CHAT_ID'}),
    's3': ({'endpoint':S,'bucket':S,'region':S,'access_key':S,'secret_key':S,'prefix':S,'ca_pem':S}, ['endpoint','bucket','region','access_key','secret_key'], {'endpoint':'https://s3.example.com','bucket':'backups','region':'us-east-1','access_key':'ACCESS_KEY','secret_key':'SECRET_KEY','prefix':'cgpanel'}),
    'ssh': ({'host':S,'port':I,'username':S,'remote_dir':S,'private_key':S,'host_key':S}, ['host','username','remote_dir','private_key','host_key'], {'host':'backup.example.com','port':22,'username':'backup','remote_dir':'/backups','private_key':'OPENSSH_PRIVATE_KEY','host_key':'VERIFIED_KEY_TYPE BASE64_KEY'}),
    'cloudflare': ({'token':S}, ['token'], {'token':'SCOPED_DNS_TOKEN'}),
}
for kind,(fields,required,example) in integration_fields.items():
    # Secret fields are conditionally required only on creation. Edits preserve blanks.
    schemas[kind.title()+'Integration']=obj({'id':S,'owner':S,'name':S,'type':enum(kind),'proxy_id':S,**fields},['name','type'])
schemas['IntegrationSave']={'oneOf':[ref(k.title()+'Integration') for k in integration_fields], 'description':'New integrations require the fields described by each type; blank secret fields preserve existing values on an edit. Send complete nonsecret configuration. Cloudflare DNS tooling uses its own direct connection.'}
job_fields = {
    'runtime': ({'version':S,'command':S}, {'target':'APP_ID','version':'3.14','command':''}),
    'ide': ({'enabled':B,'rotate':B}, {'target':'APP_ID','enabled':True}),
    'full_backup': ({'database_ids':IDS,'destinations':IDS,'quiesce':B}, {'target':'APP_ID','database_ids':['DATABASE_ID'],'destinations':[],'quiesce':True}),
    'backup_deliver': ({'destinations':IDS}, {'target':'BACKUP_ID','owner':'TENANT_ID','destinations':['STORAGE_ID']}),
    'restore_full': ({'backup_id':S,'confirm':enum('RESTORE')}, {'target':'APP_ID','backup_id':'BACKUP_ID','confirm':'RESTORE'}),
    'integration_test': ({}, {'target':'INTEGRATION_ID','owner':'TENANT_ID'}),
    'tls': ({'email':S,'validation':enum('http','dns_local','dns_cloudflare'),'integration_id':S,'agree_tos':B,'staging':B}, {'target':'DOMAIN_ID','email':'admin@example.com','validation':'http','agree_tos':True,'staging':True}),
    'panel_tls': ({'email':S,'agree_tos':B,'staging':B}, {'target':'ADMIN_ID','email':'admin@example.com','agree_tos':True,'staging':True}),
    'seo': ({'scheme':enum('http','https')}, {'target':'DOMAIN_ID','scheme':'https'}),
    'egress': ({'proxy_id':S,'locked':B}, {'target':'APP_ID','proxy_id':'PROXY_ID','locked':True}),
}
for kind,(fields,example) in job_fields.items():
    schemas[kind+'Job']=obj({'kind':enum(kind),'target':S,'owner':S,**fields},['kind','target'])
schemas['JobSubmission']={'oneOf':[ref(k+'Job') for k in job_fields], 'description':'Flat payload. Required job-specific fields are validated during execution; an accepted job can still fail. Never place integration secrets here.'}

paths={}
def endpoint(path, method, title, tag, description, request=None, example=None, response=O, parameters=(), session=False, public=False, media='application/json'):
    security=[] if public else ([{'SessionCookie':[],**({'CsrfHeader':[]} if method!='get' else {})}] if session else [{'AdminBearer':[]},{'SessionCookie':[],**({'CsrfHeader':[]} if method!='get' else {})}])
    op={'operationId':re.sub(r'[^a-zA-Z0-9]+','_',method+'_'+path).strip('_'),'summary':title,'tags':[tag],'description':description,'security':security,'responses':{'200':{'description':'Success. Job submissions return queued, not completion.','content':{media:{'schema':response}}},'default':{'description':'Application errors usually return an error object. Framework/proxy errors may differ. See API guide.','content':{'application/json':{'schema':ref('Error')}}}}}
    params=[{'name':p,'in':'path','required':True,'schema':enum(*resources) if p=='kind' else S} for p in re.findall(r'{([^}]+)}',path)]
    params.extend(parameters)
    if params:op['parameters']=params
    if request:
        content={'schema':ref(request) if isinstance(request,str) else request}
        if example is not None:content['example']=example
        op['requestBody']={'required':True,'content':{'application/json':content}}
    paths.setdefault(path,{})[method]=op

endpoint('/api/v4/files/{id}','post','Workspace file operations','Workspace','Owned application only. See the v0.4 guide for bounded uploads/downloads, revision-checked edits and file operations. Works with stopped applications.',O,{'operation':'list','path':'','offset':0})
endpoint('/api/v4/runtimes','get','Approved runtime versions','Workspace','Version tracks available for per-application runtime jobs.')
endpoint('/api/v4/runtimes/{id}','get','Current application runtime','Workspace','Owned application runtime, image and command.')
endpoint('/api/v4/ide/{id}','get','Workspace IDE status','Workspace','Owned application code-server installation, state and separate-origin URL.')
endpoint('/api/v4/ide/{id}/password','post','Read workspace IDE password','Workspace','Session and CSRF only. Owned application must have its IDE enabled. Passwords never appear in job results.',O,{},session=True)
endpoint('/api/v4/updates','get','Panel update status','Administration','Administrator only. Installed version, settings and latest updater state.')
endpoint('/api/v4/updates','post','Configure or run panel updates','Administration','Administrator only. settings changes enabled; check/apply queue a root-owned system service.',obj({'action':enum('settings','check','apply'),'enabled':B},['action']),{'action':'settings','enabled':False})
owner=query('owner','Administrator only: tenant ID. Omit to use the authenticated caller; not an all-tenant query.')
endpoint('/healthz','get','HTTP process health','Public','Does not check broker, DNS, containers or databases.',public=True)
endpoint('/api/login','post','Sign in','Authentication','Sets cg_session cookie for eight hours and returns csrf. Login throttling applies.', 'Login', {'username':'admin','password':'YOUR_PASSWORD'}, public=True)
endpoint('/api/me','get','Current identity','Authentication','Returns id, username, role and csrf. Bearer csrf is empty.',response=ref('Identity'))
endpoint('/api/logout','post','End browser session','Authentication','Session and CSRF only. Does not revoke API tokens.',O,{},session=True)
endpoint('/api/password','post','Change own password','Authentication','Session and CSRF only. Requires current password; new password 14–256 characters. Revokes all own sessions and API tokens.',obj({'current':S,'password':S},['current','password']),{'current':'CURRENT_PASSWORD','password':'NEW_PASSWORD'},session=True)
endpoint('/api/admin/system','get','Administrator system overview','Administration','Administrator only. Version, resource counts, recent events and broker host health.',response=ref('System'))
endpoint('/api/admin/audit','get','Paginated audit events','Administration','Administrator only. Events in descending ID order. Pass next_before as before until null. Includes token IDs for bearer mutations.',response=obj({'events':array(O),'next_before':{'type':'integer','nullable':True}}),parameters=[query('limit','1–200, defaults to 50',I),query('before','Exclusive positive event ID cursor',I)])
endpoint('/api/admin/tokens','get','List own API token metadata','Administration','Administrator session only. Latest 200 records; no secret or hash. Expired and revoked records remain visible.',response=array(ref('TokenMetadata')),session=True)
endpoint('/api/admin/tokens','post','Create administrator API token','Administration','Administrator session and CSRF only. Secret shown once. At most 20 active tokens, 1–90 days; account and token CIDRs both apply. Defaults: read scope, 30 days, no extra IP filter.','TokenCreate',{'name':'reporting','scope':'read','expires_days':30,'allowed_ips':['203.0.113.10/32']},response=ref('TokenSecret'),session=True)
endpoint('/api/admin/tokens/{id}','delete','Revoke own API token','Administration','Administrator session and CSRF only. Idempotent for an existing own token. Does not cancel in-flight requests or queued jobs.',session=True)
endpoint('/api/admin/openapi.json','get','Download OpenAPI contract','Administration','Administrator only. Same contract checked into docs/openapi.json.')
endpoint('/api/docs','get','List accessible documentation','Documentation','Tenant guides for all signed-in users; additional API/operations guides for administrators.',response=array(obj({'slug':S,'title':S})))
endpoint('/api/docs/{slug}','get','Read bundled guide','Documentation','Known slugs: getting-started, features; admin only: operations, api, reference. Returns sanitized HTML and original Markdown.',response=obj({'slug':S,'title':S,'html':S,'markdown':S}))
endpoint('/api/overview','get','Workspace overview','Resources','Own counts/events for tenants; all counts/events and host health for administrators.',response=ref('System'))
endpoint('/api/users','get','List accounts','Accounts','Administrator only. Includes disabled accounts; no password hashes.',response=array(ref('User')))
endpoint('/api/users','post','Create tenant account','Accounts','Administrator only. Always creates role user. quota defaults to 10 and is clamped 1–100 per resource kind. allowed_ips is up to 32 CIDRs.','UserCreate',{'username':'site_owner','password':'REPLACE_WITH_PRIVATE_PASSWORD','quota':10,'allowed_ips':[]},response=obj({'id':S,'username':S}))
endpoint('/api/users/{id}','post','Replace tenant access settings','Accounts','Administrator only, cannot change administrator accounts. Send enabled/quota/allowed_ips together; omitted values reset to true/10/[]. Optional nonempty password resets tenant password. Revokes sessions. Does not stop workloads.','UserUpdate',{'enabled':True,'quota':20,'allowed_ips':['203.0.113.0/24']})
endpoint('/api/resources/{kind}','get','List resources','Resources','Kinds: apps, domains, databases, dns, schedules, backups (legacy), blocks. Administrator sees all owners; tenant sees own. Passwords and app environment are not returned.',response=array(ref('Resource')))
endpoint('/api/resources/{kind}','post','Create managed resource','Resources','Use the request schema matching kind. Optional owner selects an active tenant for administrators; otherwise caller. Links must have same owner. New root domains and IP blocks require admin. See resource examples below.','ResourceCreate',{'owner':'TENANT_ID',**resources['apps'][3]},response=ref('ResourceCreated'))
endpoint('/api/resource/{id}','delete','Delete a resource','Resources','Owner or admin; blocks require admin. Remove linked resources first. Deleting an app retains workspace files; database deletion removes its data. Inspect dependencies and backups first.')
actions='Apps: start, stop, restart, logs, inspect, terminal, files, read, write. Databases: access. Schedules: status. Legacy workspace backups: restore. Domains: tls (legacy synchronous path; use a TLS job). All use POST, including reads, so read-only bearer tokens cannot call them. See action payload table below.'
endpoint('/api/resource/{id}/{action}','post','Run resource action','Resources',actions,O,{'command':'id'})
endpoint('/api/v2/integrations','get','List redacted integrations','Integrations','Own account unless administrator supplies owner. Secrets omitted; secret-set metadata is returned.',response=array(O),parameters=[owner])
endpoint('/api/v2/integrations','post','Create or edit integration','Integrations','Supply id to edit. Creation requires type-specific credentials. Blank secret fields preserve existing values. At most 40 per owner. Referenced/locked proxies cannot be freely changed by tenants.','IntegrationSave',{'owner':'TENANT_ID','name':'alerts','type':'telegram','token':'BOT_TOKEN','chat_id':'CHAT_ID','proxy_id':''})
endpoint('/api/v2/integrations/{id}','delete','Delete integration','Integrations','Own account unless owner supplied by admin. Remove dependent monitor/backup/proxy references first.',parameters=[owner])
endpoint('/api/v2/jobs','get','List background jobs','Jobs','Latest 100 accessible jobs, newest first. Admin sees all; tenant sees own. There is no individual-job GET endpoint.',response=array(ref('Job')))
endpoint('/api/v2/jobs','post','Submit background operation','Jobs','Flat kind/target payload. Supported types and examples below. No idempotency keys. At most 10 pending ordinary jobs per account. owner is needed for admin integration_test/backup_deliver on tenant objects; resource jobs infer it. panel_tls requires admin.','JobSubmission',{'kind':'full_backup',**job_fields['full_backup'][1]},response=ref('Queued'))
endpoint('/api/v2/jobs/{id}/retry','post','Retry failed or interrupted job','Jobs','Returns a new job ID. restore_full, tls, panel_tls and egress cannot use automatic retry; review and submit a fresh request. Original permissions/ownership apply.',response=ref('Queued'))
endpoint('/api/v2/cron-preview','post','Preview next five cron occurrences','Scheduling','Five numeric fields or hourly/daily/weekly aliases. Optional IANA timezone, UTC by default. Returns Unix seconds. POST requires admin-scope token or session CSRF.','Cron',{'schedule':'0 3 * * *','timezone':'Asia/Tehran'},response=obj({'next':array(I)}))
endpoint('/api/v2/backup-plans','get','List automatic backup plans','Backups','Tenant sees own; admin sees all. Includes config, enabled, next_run and last_job.',response=array(O))
endpoint('/api/v2/backup-plans','post','Create automatic backup plan','Backups','Owner inferred from app. At most 20 per owner. retention defaults 7, clamped 1–30; applies locally after successful delivery. No update endpoint: remove and recreate a plan.','BackupPlan',{'app_id':'APP_ID','database_ids':['DATABASE_ID'],'destinations':['STORAGE_ID'],'schedule':'0 3 * * *','timezone':'UTC','quiesce':True,'retention':7,'enabled':True})
endpoint('/api/v2/backup-plans/{id}','delete','Remove automatic backup plan','Backups','Removes an accessible schedule, not its existing archives.')
endpoint('/api/v2/backups','get','List full .cgp archives','Backups','Account scoped. Unlike legacy resources/backups, these contain selected SQL and configuration metadata.',response=array(O),parameters=[owner])
endpoint('/api/v2/backups/{id}','delete','Delete local full archive','Backups','Account scoped. Removes local archive; remote copies are not removed.',parameters=[owner])
endpoint('/api/v2/backups/{id}/download','get','Download .cgp archive','Backups','Account scoped authenticated binary ZIP download. Contains secrets. Use a private destination file.',response={'type':'string','format':'binary'},parameters=[owner],media='application/octet-stream')
endpoint('/api/v2/monitor/{id}','get','Read website monitoring','Monitoring','Domain ID; owner/admin only. Settings, latest 120 checks and 24-hour totals. Analytics tracking key is included.')
endpoint('/api/v2/monitor/{id}','post','Replace monitoring configuration','Monitoring','Domain ID. Send complete configuration. Two failed probes open an outage. Interval 60–3600 seconds; retention 1–30 days. Telegram integration must belong to domain owner.','Monitor',{'enabled':True,'scheme':'https','path':'/','interval':60,'telegram_id':'','analytics':True,'clicks':True,'retention_days':30})
endpoint('/api/v2/analytics/{id}','get','Read website analytics and heatmap','Monitoring','Domain ID; owner/admin. Data within configured retention, at most 30 days. daily, pages, heatmap, targets and referrers. IPs are site/day salted hashes, not raw addresses.',parameters=[query('path','Page path for heatmap, defaults to most viewed page'),query('viewport','desktop (default) or mobile',enum('desktop','mobile'))])
endpoint('/api/v2/domains/{id}/tls','get','Certificate and renewal status','HTTPS and routing','Domain ID; owner/admin. installed, certificate, renewal_timer_active, last_renewal_run, validation and cdn.')
endpoint('/api/v2/domains/{id}/zone','get','Download BIND DNS zone','HTTPS and routing','Domain ID; owner/admin. Import at your provider. Does not change nameservers or enable CDN.',response=S,media='text/plain')
endpoint('/api/v2/domains/{id}/cdn','post','Set trusted CDN mode','HTTPS and routing','Domain ID; owner/admin. Cloudflare mode accepts visitor headers only from known provider networks.',obj({'provider':enum('none','cloudflare')},['provider']),{'provider':'cloudflare'})
endpoint('/api/v2/egress/{id}','get','Read application proxy policy','HTTPS and routing','App ID; owner/admin. Change it by submitting an egress job.',response=obj({'proxy_id':S,'locked':B}))

contract={'openapi':'3.0.3','info':{'title':'CGPanel Administrator and Management API','version':VERSION,'description':'Community alpha. Session + CSRF or administrator bearer tokens. See docs/API.md for operational boundaries.','license':{'name':'AGPL-3.0-or-later','url':'https://www.gnu.org/licenses/agpl-3.0.html'}},'servers':[{'url':'/','description':'Your panel HTTPS origin, normally port 2083'}],'paths':paths,'components':{'securitySchemes':{'AdminBearer':{'type':'http','scheme':'bearer','description':'Revocable cgp_ administrator token. read scope permits GET/HEAD only; admin permits mutations except session-only credential operations.'},'SessionCookie':{'type':'apiKey','in':'cookie','name':'cg_session'},'CsrfHeader':{'type':'apiKey','in':'header','name':'x-csrf-token'}},'schemas':schemas},'externalDocs':{'url':'https://github.com/ItzGlace/CGPanel/blob/main/docs/API.md'}}

lines=['# Endpoint reference', '', f'CGPanel {VERSION}. Generated from `scripts/build_api_docs.py`; do not edit this file directly.', '', 'Use the [API guide](https://github.com/ItzGlace/CGPanel/blob/main/docs/API.md) for authentication and workflows. All paths below include their API prefix. An administrator session or token is required where stated; other management endpoints also accept authorized tenant sessions. Cookie-based non-GET calls require CSRF. Read-only bearer tokens cannot call POST, even for read-like actions.', '', '## Endpoint index', '', '| Method | Path | Purpose |', '| --- | --- | --- |']
for path,methods in paths.items():
    for method,op in methods.items():lines.append(f'| {method.upper()} | `{path}` | {op["summary"]} |')
for path,methods in paths.items():
    for method,op in methods.items():
        lines.extend(['',f'## {method.upper()} {path}', '',op['summary']+'. '+op['description'],''])
        for p in op.get('parameters',[]):lines.append(f'- `{p["name"]}` ({p["in"]}): {p.get("description","Opaque path value.")}')
        if op.get('parameters'):lines.append('')
        body=op.get('requestBody',{}).get('content',{}).get('application/json')
        if body:
            lines.append('Request schema: `'+body['schema'].get('$ref','object').split('/')[-1]+'` in the OpenAPI contract.')
            if 'example' in body:lines.extend(['','```json',json.dumps(body['example'],indent=2),'```'])
        lines.extend(['','Success response schema:','', '```json',json.dumps(op['responses']['200']['content'],indent=2),'```'])
lines.extend(['','## Resource creation examples','','Add `owner: "TENANT_ID"` for an administrator creating on behalf of a tenant. Replace placeholder IDs and addresses.'])
for kind,(name,fields,required,example) in resources.items():lines.extend(['',f'### POST /api/resources/{kind}','','```json',json.dumps({'owner':'TENANT_ID',**example},indent=2),'```'])
lines.extend(['','## Resource action payloads','','All use `POST /api/resource/{id}/{action}`. Empty requests still send `{}`.','', '| Resource / action | JSON body | Result / behavior |','| --- | --- | --- |','| apps / start, stop, restart | `{}` | Start/stop/restart the owned container |','| apps / inspect, logs | `{}` | Captured inspect/log output |','| apps / terminal | `{"command":"id"}` | UID 1000 command, 25-second timeout |','| apps / files | `{}` | Workspace file listing, depth at most 3 |','| apps / read | `{"path":"index.html"}` | Text in `output`; workspace-relative path only |','| apps / write | `{"path":"index.html","content":"Hello"}` | Write bounded text inside workspace |','| databases / access | `{"allowed_ips":["203.0.113.10"]}` | Replace exact-IP database remote allowlist |','| schedules / status | `{}` | Next run and bounded execution history |','| backups / restore | `{"confirm":"RESTORE"}` | Legacy workspace snapshot restore |','| domains / tls | `{"email":"admin@example.com","validation":"http","agree_tos":true}` | Legacy synchronous HTTP certificate request; prefer v2 job |'])
lines.extend(['','## Integration creation examples','','Secrets here are placeholders. Store the real JSON privately. On edit, send id plus all intended nonsecret fields; blank secret fields keep prior values. New integrations require the type-specific fields shown.'])
for kind,(fields,required,example) in integration_fields.items():lines.extend(['',f'### {kind}','','```json',json.dumps({'type':kind,'name':kind+'-connection','owner':'TENANT_ID',**example},indent=2),'```'])
lines.extend(['','## Background job payloads','','POST these flat objects to `/api/v2/jobs`, then poll the job list. Integration tests can send an actual Telegram message or upload a small test archive. Restore overwrites selected existing data. Certificate requests require actual agreement with the provider.'])
for kind,(fields,example) in job_fields.items():lines.extend(['',f'### {kind}','','```json',json.dumps({'kind':kind,**example},indent=2),'```'])
lines.extend(['','## Public telemetry','','The public website tracker is served from `/telemetry/tracker.js`; ingestion is `POST /telemetry/collect/{domain_id}`. These are website instrumentation routes, not administrator management endpoints. The per-domain Nginx `/__cgpanel/` mapping exposes the same-origin tracker/collector. Use the generated setup snippet in Website analytics rather than an administrator token. Ingestion requires the site tracking key, matching Origin and enabled configuration; it enforces separate size/rate limits and honors Do Not Track.',''])

outputs={'docs/openapi.json':json.dumps(contract,indent=2,ensure_ascii=False)+'\n','docs/API-REFERENCE.md':'\n'.join(lines)}
for name,content in outputs.items():
    path=ROOT/name
    if '--check' in sys.argv:
        if not path.exists() or path.read_text(encoding='utf-8')!=content:raise SystemExit(f'Stale documentation: run python3 scripts/build_api_docs.py ({name})')
    else:path.write_text(content,encoding='utf-8',newline='\n')
print(f'API documentation: {sum(len(p) for p in paths.values())} operations, {len(schemas)} schemas; '+('current' if '--check' in sys.argv else 'generated'))
