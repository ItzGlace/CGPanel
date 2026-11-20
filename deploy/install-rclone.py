#!/usr/bin/env python3
"""Install pinned rclone from the package, or verified upstream download sources."""
import argparse
import hashlib
import io
import os
import pathlib
import platform
import tempfile
import urllib.error
import urllib.request
import zipfile

VERSION = '1.75.1'
CHECKSUMS = {
    'amd64': '982b5aa772841168f8e380f139e9e787b2a105403e32b94da8676a0e1c0a13ab',
    'arm64': '03f2504174034b6d004152ed7369251c9a9ec1f7e0836eda420f5c7a5ec0dff9',
}
SOURCES = (
    'https://raw.githubusercontent.com/ItzGlace/CGPanel/apt/tools/',
    f'https://github.com/rclone/rclone/releases/download/v{VERSION}/',
    f'https://downloads.rclone.org/v{VERSION}/',
)
MAX_ARCHIVE = 100 * 1024 * 1024
MAX_BINARY = 160 * 1024 * 1024


def architecture():
    try:
        return {'x86_64': 'amd64', 'aarch64': 'arm64'}[platform.machine()]
    except KeyError as error:
        raise RuntimeError('Supported rclone architectures: amd64 and arm64') from error


def archive_name(arch):
    if arch not in CHECKSUMS:
        raise ValueError('Unsupported architecture')
    return f'rclone-v{VERSION}-linux-{arch}.zip'


def fetch(url):
    request = urllib.request.Request(url, headers={'User-Agent': 'CGPanel-installer'})
    with urllib.request.urlopen(request, timeout=120) as response:
        data = response.read(MAX_ARCHIVE + 1)
    if len(data) > MAX_ARCHIVE:
        raise ValueError('rclone archive exceeds the size limit')
    return data


def verify(data, expected):
    if len(data) > MAX_ARCHIVE or hashlib.sha256(data).hexdigest() != expected:
        raise ValueError('rclone checksum mismatch; existing installation was retained')
    return data


def download(arch, *, fetcher=fetch, sources=SOURCES, expected=None):
    name = archive_name(arch)
    expected = expected or CHECKSUMS[arch]
    failures = []
    for base in sources:
        print('Fetching pinned rclone from ' + base, flush=True)
        try:
            data = fetcher(base + name)
        except (urllib.error.URLError, OSError) as error:
            failures.append(f'{base}: {error}')
            continue
        # A checksum failure is terminal, rather than bypassing integrity checks.
        return verify(data, expected)
    raise RuntimeError('Cannot download verified rclone from any configured source. ' + '; '.join(failures))


def atomic(path, data, mode):
    path = pathlib.Path(path)
    path.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=path.name + '.', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'wb') as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        os.chmod(temporary, mode)
        os.replace(temporary, path)
    finally:
        pathlib.Path(temporary).unlink(missing_ok=True)


def install(arch, *, target='/usr/local/lib/cgpanel/rclone', bundled=None,
            fetcher=fetch, expected=None):
    expected = expected or CHECKSUMS[arch]
    if bundled is not None and pathlib.Path(bundled).is_file():
        data = verify(pathlib.Path(bundled).read_bytes(), expected)
    else:
        data = download(arch, fetcher=fetcher, expected=expected)
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        member = archive.getinfo(archive_name(arch).removesuffix('.zip') + '/rclone')
        if member.file_size > MAX_BINARY:
            raise ValueError('rclone binary exceeds the size limit')
        binary = archive.read(member)
    atomic(target, binary, 0o755)
    print('Installed verified upstream rclone ' + VERSION)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--download-only', type=pathlib.Path,
                        help='Save a verified archive for an offline package build')
    arguments = parser.parse_args()
    arch = architecture()
    if arguments.download_only:
        if arguments.download_only.is_file():
            verify(arguments.download_only.read_bytes(), CHECKSUMS[arch])
        else:
            atomic(arguments.download_only, download(arch), 0o644)
        print('Saved verified rclone archive: ' + str(arguments.download_only))
    else:
        bundled = pathlib.Path(__file__).resolve().parents[1] / 'tools' / archive_name(arch)
        install(arch, bundled=bundled)


if __name__ == '__main__':
    main()
