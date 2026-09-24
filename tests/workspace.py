#!/usr/bin/env python3
"""File helper boundary/integrity tests. No server or root account required."""
import base64, json, os, pathlib, subprocess, tempfile, zipfile
binary=pathlib.Path(os.environ.get('CGPANEL_WORKSPACE_BIN','target/debug/cgpanel-workspace')).resolve()
with tempfile.TemporaryDirectory() as temporary:
    root=pathlib.Path(temporary)/'workspace';root.mkdir()
    outside=pathlib.Path(temporary)/'outside';outside.mkdir();(outside/'secret').write_text('DO NOT MODIFY')
    def call(operation,path='',ok=True,**data):
        result=subprocess.run([str(binary),str(root)],input=json.dumps(dict(operation=operation,path=path,**data)),text=True,capture_output=True)
        assert (result.returncode==0)==ok,(operation,path,result.stderr)
        return json.loads(result.stdout) if ok else None
    call('mkdir','public');call('touch','public/index.php')
    opened=call('read','public/index.php')
    call('save','public/index.php',content='<?php echo "hello";',revision=opened['revision'])
    call('save','public/index.php',ok=False,content='stale',revision=opened['revision'])
    assert call('read','public/index.php')['content']=='<?php echo "hello";'
    upload=call('upload_begin','public/large.bin')['upload'];data=os.urandom(196608)
    call('upload_chunk','public/large.bin',upload=upload,offset=0,data=base64.b64encode(data).decode())
    call('upload_chunk','public/large.bin',ok=False,upload=upload,offset=0,data='eA==')
    call('upload_finish','public/large.bin',upload=upload)
    assert base64.b64decode(call('download','public/large.bin')['data'])==data
    call('copy','public/index.php',destination='public/copy.php');call('move','public/copy.php',destination='public/renamed.php')
    call('move','public/renamed.php',ok=False,destination='public/index.php')
    trashed=call('trash','public/renamed.php')['trash_path'];call('move',trashed,destination='public/restored.php')
    call('archive','public',destination='site.zip');call('extract','site.zip',destination='unpacked')
    assert (root/'unpacked/public/index.php').read_text()=='<?php echo \"hello\";'
    call('copy','public',ok=False,destination='public/nested')
    with zipfile.ZipFile(root/'bad.zip','w') as archive:archive.writestr('../outside/secret','BAD')
    call('extract','bad.zip',ok=False,destination='bad-unpack')
    assert not (root/'bad-unpack').exists()
    (root/'escape').symlink_to(outside,target_is_directory=True)
    (root/'link').symlink_to(outside/'secret')
    os.link(outside/'secret',root/'hardlink')
    for path in ('../outside/secret','/etc/passwd','escape/secret','link','hardlink'):
        call('read',path,ok=False)
        call('save',path,ok=False,content='bad',revision='')
    call('mkdir','escape/new',ok=False);call('copy','escape',ok=False,destination='bad-copy')
    call('chmod','public/index.php',mode='4755',ok=False)
    assert (outside/'secret').read_text()=='DO NOT MODIFY'
    assert call('list')['total']>=4
    print('PASS: editor conflicts, upload offsets, binary integrity, copy/move/trash, traversal/symlink/hardlink rejection')
