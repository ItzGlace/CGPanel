#!/usr/bin/env python3
"""Verify each runtime's actual starter HTTP service on an installed development host."""
import os, time, urllib.request
from smoke import Client

admin=Client()
admin.login('admin',os.environ['CGPANEL_TEST_PASSWORD'])
for runtime in ['python','php','node','java','rust','static']:
    app=None
    try:
        result=admin.call('/resources/apps','POST',{'name':'qa_'+runtime,'runtime':runtime,'mode':'web','command':'','env':{}})
        app=result['id'];port=result['result']['port']
        for attempt in range(60):
            try:
                with urllib.request.urlopen('http://127.0.0.1:'+str(port),timeout=2) as response:
                    body=response.read().decode()
                    assert response.status==200
                    break
            except Exception:
                if attempt==59:raise
                time.sleep(1)
        assert 'CGPanel' in body,(runtime,body)
        identity=admin.call('/resource/'+app+'/terminal','POST',{'command':'id'})['output']
        assert 'uid=1000' in identity,(runtime,identity)
        print('PASS',runtime,'HTTP 200; UID 1000',flush=True)
    finally:
        if app:admin.call('/resource/'+app,'DELETE')
