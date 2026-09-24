#!/usr/bin/env python3
"""Isolated analytics transport, aggregation, privacy and access tests."""
import json, os, secrets, socket, sqlite3, subprocess, tempfile, time
from pathlib import Path
from admin_api import Client, BINARY

with tempfile.TemporaryDirectory(prefix='cgp-analytics-') as folder:
    folder=Path(folder); dbfile=folder/'db.sqlite'; password=secrets.token_urlsafe(24)
    with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
    env={**os.environ,'CGPANEL_DB':str(dbfile),'CGPANEL_ADMIN':'admin','CGPANEL_ADMIN_PASSWORD':password,'CGPANEL_BIND':f'127.0.0.1:{port}','CGPANEL_INSECURE_LOCAL':'1','CGPANEL_AGENT_SOCKET':str(folder/'missing.sock')}
    subprocess.run([str(BINARY),'bootstrap'],env=env,check=True,capture_output=True)
    env.pop('CGPANEL_ADMIN_PASSWORD')
    with (folder/'log').open('w') as log:
        process=subprocess.Popen([str(BINARY)],env=env,stdout=log,stderr=log)
        try:
            client=Client(f'http://127.0.0.1:{port}')
            for _ in range(100):
                try:client.call('/healthz');break
                except OSError:time.sleep(.05)
            client.login('admin',password);uid=client.call('/api/me')['id'];rid='a'*32
            with sqlite3.connect(dbfile) as db:
                db.execute("INSERT INTO resources(id,owner,kind,name,data) VALUES(?,?,'domains','site.example','{}')",(rid,uid))
            client.call('/api/v2/monitor/'+rid,'POST',{'scheme':'https','path':'/','analytics':True,'clicks':True})
            key=client.call('/api/v2/monitor/'+rid)['settings']['analytics_key']
            anonymous=Client(client.base)
            event={'key':key,'kind':'view','path':'/page?secret=discarded','referrer':'https://private.example/path?secret=1'}
            headers={'Content-Type':'text/plain;charset=UTF-8','Origin':'https://site.example','x-cgpanel-client-ip':'203.0.113.10'}
            endpoint='/telemetry/collect/'+rid
            anonymous.call(endpoint,'POST',event,status=204,headers=headers)
            anonymous.call(endpoint,'POST',event,status=204,headers=headers)
            anonymous.call(endpoint,'POST',event,status=204,headers={**headers,'x-cgpanel-client-ip':'203.0.113.11'})
            anonymous.call(endpoint,'POST',{**event,'kind':'click','x':250,'y':125,'target':'button'},status=204,headers=headers)
            anonymous.call(endpoint,'POST',event,status=204,headers={**headers,'dnt':'1'})
            anonymous.call(endpoint,'POST',event,status=403,headers={**headers,'Origin':'https://other.example'})
            anonymous.call(endpoint,'POST',{**event,'key':'wrong'},status=403,headers=headers)
            anonymous.call('/api/v2/analytics/'+rid,status=401)
            result=client.call('/api/v2/analytics/'+rid)
            assert result['daily'][0]['views']==3 and result['daily'][0]['unique_ips']==2,result
            assert result['pages'][0]['path']=='/page'
            assert result['heatmap']['cells']==[{'col':2,'row':2,'count':1}]
            assert result['referrers'][0]['host']==''
            with sqlite3.connect(dbfile) as db:
                rows=db.execute('SELECT visitor,path,referrer FROM analytics_events').fetchall()
                assert all(len(row[0])==64 and '203.0.113.' not in str(row) and 'secret' not in str(row) for row in rows)
            ranges=client.call('/api/v5/network-range','POST',{'value':'10.10.10.17/24,2001:db8::1/126,bad'})
            assert ranges[0]['first']=='10.10.10.0' and ranges[0]['last']=='10.10.10.255'
            assert ranges[1]['last']=='2001:db8::3' and ranges[2]['error']
            print('PASS: text/plain beacons, unique IPs, clicks, origin/key rejection, DNT, privacy and CIDR previews')
        finally:
            process.terminate();process.wait(timeout=10)
