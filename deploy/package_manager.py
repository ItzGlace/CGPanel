"""Bounded APT preparation without interrupting another package manager.

APT's package-index lock is a POSIX record lock; DPkg::Lock::Timeout does not
cover apt-get update. Check both index and dpkg locks and retry only a proven
lock race after the check. Existing lock files are never removed or truncated.
"""
import errno
import fcntl
import os
import pathlib
import subprocess
import time

LOCKS = ('/var/lib/apt/lists/lock', '/var/lib/dpkg/lock-frontend', '/var/lib/dpkg/lock', '/var/cache/apt/archives/lock')


def locked_paths(paths=LOCKS):
    busy = []
    for name in paths:
        try:
            descriptor = os.open(name, os.O_RDWR)
        except FileNotFoundError:
            continue
        try:
            try:
                fcntl.lockf(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError as error:
                if error.errno not in (errno.EACCES, errno.EAGAIN):
                    raise
                busy.append(str(name))
        finally:
            os.close(descriptor)
    return busy


def install_packages(packages, *, timeout=1200, locks=LOCKS, poll=2, runner=subprocess.run, log=print):
    if not packages:
        return
    if os.environ.get("CGPANEL_APT_CONTEXT")=="1":
        raise RuntimeError("Missing package dependencies in APT context: "+", ".join(packages)+". Repair package dependencies with apt-get -f install; nested APT is forbidden.")
    deadline = time.monotonic() + timeout
    announced = False

    def wait():
        nonlocal announced
        while True:
            busy = locked_paths(locks)
            if not busy:
                return
            if time.monotonic() >= deadline:
                raise TimeoutError('Package manager is still busy: ' + ', '.join(busy) + '. Retry after that package operation finishes.')
            if not announced:
                log('Waiting for the package manager to finish: ' + ', '.join(busy), flush=True)
                announced = True
            time.sleep(min(poll, max(0, deadline - time.monotonic())))

    def apt(arguments):
        nonlocal announced
        while True:
            wait()
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError('Package preparation timed out; CGPanel has not been stopped')
            result = runner(['apt-get', '-o', 'DPkg::Lock::Timeout=60', *arguments],
                            env={**os.environ, 'DEBIAN_FRONTEND': 'noninteractive'},
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                            timeout=remaining, check=False)
            output = result.stdout or ''
            if output:
                log(output, end='' if output.endswith('\n') else '\n', flush=True)
            if result.returncode == 0:
                return
            # Another APT may have won between our check and apt-get acquiring its
            # own lock. Never retry repository/signature/installation failures here.
            lower = output.lower()
            if any(marker in lower for marker in ('could not get lock', 'unable to acquire the dpkg frontend lock', 'unable to lock directory', 'unable to lock the administration directory')):
                if time.monotonic() >= deadline:
                    raise TimeoutError('Package manager remained busy during update preparation')
                if not announced:
                    log('Waiting for another package operation before retrying APT', flush=True)
                    announced = True
                time.sleep(min(poll, max(0, deadline - time.monotonic())))
                continue
            raise RuntimeError('apt-get ' + arguments[0] + ' failed (exit ' + str(result.returncode) + '):\n' + output[-6000:])

    log('Preparing required packages: ' + ', '.join(packages), flush=True)
    apt(['update','-o','APT::Update::Error-Mode=any'])
    apt(['install', '-y', *packages])
