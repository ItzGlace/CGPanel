#!/usr/bin/env python3
"""Install the tested Certbot environment without resolving packages on the host."""
import argparse
import hashlib
import io
import json
import os
import pathlib
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request

MANIFEST = pathlib.Path(__file__).with_name('certbot-wheels.json')
MAX_ARCHIVE = 64 * 1024 * 1024
MAX_WHEELS = 96 * 1024 * 1024
SOURCES = (
    'https://raw.githubusercontent.com/ItzGlace/CGPanel/apt/tools/',
    'https://github.com/ItzGlace/CGPanel/raw/refs/heads/apt/tools/',
)


def manifest():
    return json.loads(MANIFEST.read_text())


def supported():
    if sys.version_info[:2] != (3, 12) or platform.machine() != 'x86_64' or sys.platform != 'linux':
        raise RuntimeError('The bundled Certbot environment requires Ubuntu 24.04 amd64 (Python 3.12)')


def verify(data, metadata):
    if len(data) > MAX_ARCHIVE or hashlib.sha256(data).hexdigest() != metadata['sha256']:
        raise ValueError('Certbot bundle checksum mismatch; the existing installation was retained')
    return data


def fetch(url):
    request = urllib.request.Request(url, headers={'User-Agent': 'CGPanel-installer'})
    with urllib.request.urlopen(request, timeout=120) as response:
        data = response.read(MAX_ARCHIVE + 1)
    if len(data) > MAX_ARCHIVE:
        raise ValueError('Certbot bundle exceeds the size limit')
    return data


def download(metadata, *, fetcher=fetch, sources=SOURCES):
    failures = []
    for source in sources:
        try:
            data = fetcher(source + metadata['archive'])
        except (urllib.error.URLError, OSError) as error:
            failures.append(str(error))
            continue
        return verify(data, metadata)
    raise RuntimeError('Could not fetch the verified Certbot bundle: ' + '; '.join(failures))


def atomic(path, data):
    path = pathlib.Path(path)
    path.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=path.name + '.', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'wb') as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        os.chmod(temporary, 0o644)
        os.replace(temporary, path)
    finally:
        pathlib.Path(temporary).unlink(missing_ok=True)


def unpack(data, metadata, folder):
    expected = {wheel['filename']: wheel for wheel in metadata['wheels']}
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
        members = archive.getmembers()
        if len(members) != len(expected) or {member.name for member in members} != set(expected):
            raise ValueError('Unexpected or duplicate files in the Certbot bundle')
        if sum(member.size for member in members) > MAX_WHEELS:
            raise ValueError('Certbot wheels exceed the size limit')
        for member in members:
            if not member.isfile() or pathlib.PurePosixPath(member.name).name != member.name:
                raise ValueError('Unsafe Certbot wheel member')
            wheel = archive.extractfile(member).read()
            if hashlib.sha256(wheel).hexdigest() != expected[member.name]['sha256']:
                raise ValueError('Certbot wheel checksum mismatch: ' + member.name)
            (folder / member.name).write_bytes(wheel)


def check(environment, metadata, *, runner=subprocess.run):
    runner([str(environment / 'bin/python'), '-I', '-m', 'pip', '--isolated', 'check'], check=True)
    result = runner([str(environment / 'bin/certbot'), '--version'], check=True,
                    capture_output=True, text=True)
    if result.stdout.strip() != 'certbot ' + metadata['version']:
        raise RuntimeError('Unexpected Certbot version after installation')
    # Discover entry points without contacting ACME, opening ports, or requiring
    # existing certificate configuration. Import native wheels as well.
    runner([str(environment / 'bin/python'), '-I', '-c',
            "import cryptography.hazmat.bindings._rust, pydantic_core; "
            "from importlib.metadata import entry_points; "
            "p=next(p for p in entry_points(group='certbot.plugins') if p.name=='dns-cloudflare'); "
            "assert p.load() is not None"], check=True)


def activate(environment, target):
    link = environment.parent / (environment.name + '.link')
    backup = None
    try:
        link.symlink_to(environment.name)
        if target.exists() and not target.is_symlink():
            # Preserve the directory from older installations, including any
            # partially installed dependencies, before adopting a stable link.
            backup = environment.parent / (environment.name + '.previous')
            target.rename(backup)
        try:
            os.replace(link, target)
        except BaseException:
            if backup is not None:
                backup.rename(target)
            raise
    finally:
        link.unlink(missing_ok=True)


def install(metadata, *, prefix=pathlib.Path('/opt/cgpanel-tools'), bundled=None,
            fetcher=fetch, runner=subprocess.run):
    prefix = pathlib.Path(prefix).resolve()
    target = prefix / 'certbot'
    marker = '.cgpanel-bundle-sha256'
    if target.is_symlink() and target.resolve().parent == prefix:
        existing = target.resolve()
        if (existing / marker).is_file() and (existing / marker).read_text().strip() == metadata['sha256']:
            check(existing, metadata, runner=runner)
            print('Verified bundled Certbot ' + metadata['version'] + ' is already installed')
            return
    # Validate all downloaded or bundled bytes before changing the environment.
    if bundled is not None and pathlib.Path(bundled).is_file():
        data = verify(pathlib.Path(bundled).read_bytes(), metadata)
    else:
        data = download(metadata, fetcher=fetcher)
    with tempfile.TemporaryDirectory(prefix='cgpanel-certbot-wheels-') as temporary:
        wheels = pathlib.Path(temporary)
        unpack(data, metadata, wheels)
        requirements = wheels / 'requirements.txt'
        requirements.write_text(''.join(wheel['requirement'] + ' --hash=sha256:' + wheel['sha256'] + '\n'
                                       for wheel in metadata['wheels']))
        prefix.mkdir(mode=0o755, parents=True, exist_ok=True)
        environment = pathlib.Path(tempfile.mkdtemp(prefix='certbot-' + metadata['version'] + '-', dir=prefix))
        try:
            runner([sys.executable, '-I', '-m', 'venv', str(environment)], check=True)
            runner([str(environment / 'bin/python'), '-I', '-m', 'pip', '--isolated',
                    '--disable-pip-version-check', 'install', '--no-index', '--no-cache-dir',
                    '--require-hashes', '--only-binary=:all:', '--find-links', str(wheels),
                    '-r', str(requirements)], check=True,
                   env={**os.environ, 'PIP_CONFIG_FILE': os.devnull})
            check(environment, metadata, runner=runner)
            (environment / marker).write_text(metadata['sha256'] + '\n')
            environment.chmod(0o755)
            activate(environment, target)
        except BaseException:
            if not target.is_symlink() or target.resolve() != environment:
                shutil.rmtree(environment)
            raise
    print('Installed verified offline Certbot ' + metadata['version'] + ' with Cloudflare DNS support')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--download-only', type=pathlib.Path)
    arguments = parser.parse_args()
    metadata = manifest()
    if arguments.download_only:
        if arguments.download_only.is_file():
            verify(arguments.download_only.read_bytes(), metadata)
        else:
            atomic(arguments.download_only, download(metadata))
        print('Saved verified Certbot bundle: ' + str(arguments.download_only))
    else:
        supported()
        bundled = pathlib.Path(__file__).resolve().parents[1] / 'tools' / metadata['archive']
        install(metadata, bundled=bundled)


if __name__ == '__main__':
    main()
