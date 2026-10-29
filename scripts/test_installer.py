"""Exercise the installer offline with release fixtures and mocked downloads."""
import hashlib
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

INSTALLER = Path(__file__).resolve().parents[1] / 'install-wmlm.sh'


class InstallerTests(unittest.TestCase):
    def run_installer(self, system='Linux', arch='x86_64', failure=''):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            mocks = root / 'mocks'
            mocks.mkdir()
            destination = root / 'install with spaces'
            destination.mkdir()
            binary = destination / 'wmlm'
            binary.write_bytes(b'old binary')
            payload = b'#!/bin/sh\necho fixture\n'
            archive = root / 'fixture.tar.gz'
            with tarfile.open(archive, 'w:gz') as bundle:
                entry = tarfile.TarInfo('unexpected' if failure == 'contents' else 'wmlm')
                entry.size = len(payload)
                if failure == 'symlink':
                    entry.type = tarfile.SYMTYPE
                    entry.linkname = '/tmp/should-not-be-used'
                    entry.size = 0
                bundle.addfile(entry, io.BytesIO(payload))
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            (root / 'checksum').write_text(('0' * 64 if failure == 'checksum' else digest) + '  fixture.tar.gz\n')
            (mocks / 'uname').write_text('#!/bin/sh\ncase "$1" in -s) echo "$TEST_SYSTEM";; -m) echo "$TEST_ARCH";; esac\n')
            (mocks / 'curl').write_text('''#!/bin/sh
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) output=$2; shift 2 ;;
        --proto|--tlsv1.2|-w) shift 2 ;;
        -*) shift ;;
        *) url=$1; shift ;;
    esac
done
printf '%s\\n' "$url" >> "$FIXTURES/requests"
case "$url" in
    */releases/latest) printf 'https://github.com/minitoolz/with-my-llm/releases/tag/v1.2.3' ;;
    *.sha256) cp "$FIXTURES/checksum" "$output" ;;
    *.tar.gz)
        [ "$TEST_FAILURE" != download ] || exit 22
        cp "$FIXTURES/fixture.tar.gz" "$output" ;;
    *) exit 22 ;;
esac
''')
            for mock in mocks.iterdir():
                mock.chmod(0o755)
            env = dict(os.environ, PATH=f'{mocks}:{os.environ["PATH"]}',
                       WMLM_INSTALL_DIR=str(destination), FIXTURES=str(root),
                       TEST_SYSTEM=system, TEST_ARCH=arch, TEST_FAILURE=failure)
            # Feed via stdin, matching curl | sh usage.
            result = subprocess.run(['sh'], input=INSTALLER.read_text(), env=env,
                                    text=True, capture_output=True)
            if failure or system == 'FreeBSD':
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertEqual(binary.read_bytes(), b'old binary')
            else:
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(binary.read_bytes(), payload)
                self.assertTrue(os.access(binary, os.X_OK))
                target_arch = 'aarch64' if arch in ('arm64', 'aarch64') else 'x86_64'
                target_os = 'apple-darwin' if system == 'Darwin' else 'unknown-linux-musl'
                self.assertIn(f'/v1.2.3/wmlm-{target_arch}-{target_os}.tar.gz',
                              (root / 'requests').read_text())
            self.assertEqual(list(destination.glob('.wmlm-install.*')), [])

    def test_platforms(self):
        for system, arch in [('Linux', 'x86_64'), ('Linux', 'aarch64'),
                             ('Darwin', 'x86_64'), ('Darwin', 'arm64')]:
            with self.subTest(system=system, arch=arch):
                self.run_installer(system, arch)

    def test_failures_preserve_existing_installation(self):
        for failure in ('checksum', 'download', 'contents', 'symlink'):
            with self.subTest(failure=failure):
                self.run_installer(failure=failure)

    def test_unsupported_platform(self):
        self.run_installer(system='FreeBSD')


if __name__ == '__main__':
    unittest.main()
