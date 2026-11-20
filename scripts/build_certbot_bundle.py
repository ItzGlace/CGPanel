#!/usr/bin/env python3
"""Reproduce the pinned Certbot archive from a directory of verified wheels."""
import argparse
import gzip
import hashlib
import io
import json
import pathlib
import tarfile

root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('wheels', type=pathlib.Path)
parser.add_argument('output', type=pathlib.Path)
arguments = parser.parse_args()
metadata = json.loads((root / 'deploy/certbot-wheels.json').read_text())
stream = io.BytesIO()
with tarfile.open(fileobj=stream, mode='w', format=tarfile.PAX_FORMAT) as archive:
    for wheel in metadata['wheels']:
        data = (arguments.wheels / wheel['filename']).read_bytes()
        assert hashlib.sha256(data).hexdigest() == wheel['sha256'], wheel['filename']
        member = tarfile.TarInfo(wheel['filename'])
        member.size = len(data)
        member.mode = 0o644
        archive.addfile(member, io.BytesIO(data))
data = gzip.compress(stream.getvalue(), mtime=0)
assert hashlib.sha256(data).hexdigest() == metadata['sha256'], 'Bundle is not identical to the pinned archive'
arguments.output.parent.mkdir(parents=True, exist_ok=True)
arguments.output.write_bytes(data)
print('Reproduced verified Certbot archive:', arguments.output)
