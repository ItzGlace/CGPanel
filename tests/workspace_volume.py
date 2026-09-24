#!/usr/bin/env python3
"""Root integration test on a dedicated development host; removes its own fixture."""
import json, os, pathlib, shutil, subprocess, sys, uuid

assert os.geteuid() == 0
assert '--dedicated-test-host' in sys.argv
tenant = uuid.uuid4().hex
app = uuid.uuid4().hex
username = 'cg_' + tenant[:20]
base = pathlib.Path('/var/lib/cgpanel-volumes')
home = pathlib.Path('/srv/cgpanel/tenants') / tenant
workspace = home / 'apps' / app
helper = pathlib.Path(__file__).resolve().parents[1] / 'deploy/workspace-volume.py'
original_fstab = pathlib.Path('/etc/fstab').read_text()
def run(*args, check=True):
    return subprocess.run(args, check=check, text=True, capture_output=True)
try:
    run('useradd', '--no-create-home', '--home-dir', str(home), username)
    workspace.mkdir(parents=True)
    (workspace / 'keep.txt').write_text('preserve this website\n')
    run('chown', '-R', username + ':' + username, str(home))
    result = run('python3', str(helper), tenant, app, '128')
    info = json.loads(result.stdout)
    assert info['enforced'] and os.path.ismount(workspace)
    assert (workspace / 'keep.txt').read_text() == 'preserve this website\n'
    fill = run('runuser', '-u', username, '--', 'dd', 'if=/dev/zero',
               'of=' + str(workspace / 'fill'), 'bs=1M', 'count=180', check=False)
    assert fill.returncode != 0 and 'No space left on device' in fill.stderr, fill
    (workspace / 'fill').unlink()
    run('python3', str(helper), tenant, app, '192')
    assert json.loads((base / (app + '.json')).read_text())['size_mb'] == 192
    shrink = run('python3', str(helper), tenant, app, '128', check=False)
    assert shrink.returncode != 0 and 'Volumes can grow' in shrink.stderr
    assert (workspace / 'keep.txt').read_text() == 'preserve this website\n'
    print('PASS: migration preserves files, tenant writes hit ENOSPC, online growth, shrink rejection')
finally:
    if os.path.ismount(workspace): run('umount', str(workspace))
    pathlib.Path('/etc/fstab').write_text(original_fstab)
    run('systemctl', 'daemon-reload')
    shutil.rmtree(home, ignore_errors=True)
    for suffix in ('.img', '.json'):
        (base / (app + suffix)).unlink(missing_ok=True)
    run('userdel', username, check=False)
