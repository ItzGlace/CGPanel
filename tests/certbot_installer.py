#!/usr/bin/env python3
"""Integrity, transaction and retry behavior for the offline Certbot installer."""
import hashlib
import importlib.util
import io
import pathlib
import subprocess
import tarfile
import tempfile
import unittest
import unittest.mock
import urllib.error

root = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('certbot_installer', root / 'deploy/install-certbot.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class Tests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.folder = pathlib.Path(self.directory.name)
        self.prefix = self.folder / 'tools'
        self.target = self.prefix / 'certbot'
        self.target.mkdir(parents=True)
        (self.target / 'sentinel').write_text('previous working environment')
        self.wheel = b'test wheel contents'
        self.metadata = {'version': '5.10.0', 'archive': 'test.tar.gz', 'wheels': [
            {'filename': 'test-1-py3-none-any.whl', 'requirement': 'test==1',
             'sha256': hashlib.sha256(self.wheel).hexdigest()}]}
        self.bundle = self.folder / 'bundle.tar.gz'
        self.pack([('test-1-py3-none-any.whl', self.wheel)])

    def pack(self, entries):
        archive = io.BytesIO()
        with tarfile.open(fileobj=archive, mode='w:gz') as tar:
            for name, data in entries:
                member = tarfile.TarInfo(name)
                member.size = len(data)
                tar.addfile(member, io.BytesIO(data))
        data = archive.getvalue()
        self.metadata['sha256'] = hashlib.sha256(data).hexdigest()
        self.bundle.write_bytes(data)
        return data

    def runner(self, command, **kwargs):
        return subprocess.CompletedProcess(command, 0, stdout='certbot 5.10.0\n')

    def assert_previous_retained(self):
        self.assertFalse(self.target.is_symlink())
        self.assertEqual((self.target / 'sentinel').read_text(), 'previous working environment')
        self.assertEqual(list(self.prefix.iterdir()), [self.target])

    def test_offline_install_and_repeat_preserve_previous_environment(self):
        def no_network(url):
            raise AssertionError('Offline installation contacted a download source')
        module.install(self.metadata, prefix=self.prefix, bundled=self.bundle,
                       fetcher=no_network, runner=self.runner)
        installed = self.target.resolve()
        self.assertTrue(self.target.is_symlink())
        self.assertEqual(next(self.prefix.glob('*.previous/sentinel')).read_text(),
                         'previous working environment')
        module.install(self.metadata, prefix=self.prefix, fetcher=no_network, runner=self.runner)
        self.assertEqual(installed, self.target.resolve())

    def test_forbidden_source_falls_back_and_requires_hash(self):
        calls = []
        def fetch(url):
            calls.append(url)
            if len(calls) == 1:
                raise urllib.error.HTTPError(url, 403, 'Forbidden', {}, None)
            return self.bundle.read_bytes()
        self.assertEqual(module.download(self.metadata, fetcher=fetch), self.bundle.read_bytes())
        self.assertEqual(len(calls), 2)

    def test_bad_download_is_terminal_and_keeps_current_installation(self):
        calls = []
        def fetch(url):
            calls.append(url)
            return b'corrupt'
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            module.install(self.metadata, prefix=self.prefix, fetcher=fetch, runner=self.runner)
        self.assertEqual(len(calls), 1)
        self.assert_previous_retained()

    def test_corrupt_bundled_archive_never_runs_pip(self):
        self.bundle.write_bytes(b'corrupt')
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            module.install(self.metadata, prefix=self.prefix, bundled=self.bundle,
                           runner=lambda *args, **kwargs: self.fail('Ran a subprocess'))
        self.assert_previous_retained()

    def test_individual_wheel_hash_and_unexpected_members_are_rejected(self):
        for entries, message in [
            ([('test-1-py3-none-any.whl', b'tampered')], 'wheel checksum'),
            ([('../escape.whl', self.wheel)], 'Unexpected'),
            ([('test-1-py3-none-any.whl', self.wheel)] * 2, 'duplicate'),
        ]:
            with self.subTest(message=message):
                self.pack(entries)
                with self.assertRaisesRegex(ValueError, message):
                    module.install(self.metadata, prefix=self.prefix, bundled=self.bundle,
                                   runner=lambda *args, **kwargs: self.fail('Ran a subprocess'))
                self.assert_previous_retained()

    def test_pip_failure_preserves_previous_environment_and_cleans_staging(self):
        def fail(command, **kwargs):
            if 'install' in command:
                self.assertIn('--no-index', command)
                self.assertIn('--require-hashes', command)
                raise subprocess.CalledProcessError(1, command)
            return self.runner(command, **kwargs)
        with self.assertRaises(subprocess.CalledProcessError):
            module.install(self.metadata, prefix=self.prefix, bundled=self.bundle, runner=fail)
        self.assert_previous_retained()

    def test_activation_failure_restores_previous_directory(self):
        with unittest.mock.patch.object(module.os, 'replace', side_effect=OSError('activation failed')):
            with self.assertRaisesRegex(OSError, 'activation failed'):
                module.install(self.metadata, prefix=self.prefix, bundled=self.bundle, runner=self.runner)
        self.assert_previous_retained()


if __name__ == '__main__':
    unittest.main()
