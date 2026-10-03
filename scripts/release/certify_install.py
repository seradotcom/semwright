"""Exercise the shipped helpers against an extracted bundle on its native OS."""
from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from bundle_contract import BINS, verify_checksums


def certify_install(stage: Path, platform: str, smoke=None) -> dict[str, bool]:
    stage = stage.resolve()
    records = verify_checksums(stage)
    # Windows prefixes must remain under the real user profile. POSIX tests use a private HOME.
    with tempfile.TemporaryDirectory(prefix='.semwright-install-cert-', dir=Path.home()) as folder:
        home = Path(folder) / 'home'
        home.mkdir(mode=0o700)
        env = dict(os.environ, HOME=str(home))
        prefix = home / 'Installed bundle with spaces'

        def helper(operation: str, expected_success: bool = True):
            if platform == 'windows':
                shell = shutil.which('pwsh') or shutil.which('powershell')
                if not shell:
                    raise RuntimeError('PowerShell required for native Windows install acceptance')
                command = [shell, '-NoProfile', '-NonInteractive', '-File',
                           str(stage / f'{operation}-Semwright.ps1'), '-Prefix', str(prefix)]
            else:
                command = [str(stage / f'{operation.lower()}.sh'), '--prefix', str(prefix)]
            result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=120)
            if (result.returncode == 0) != expected_success:
                raise RuntimeError(f'{operation} unexpected rc={result.returncode}: '
                                   f'{result.stdout}\n{result.stderr}')
            return result

        helper('Install')
        verify_checksums_installed(prefix, records)
        if smoke is not None:
            smoke(prefix / 'bin')
        else:
            for name in BINS[:4]:
                suffix = '.exe' if platform == 'windows' else ''
                subprocess.run([str(prefix / 'bin' / (name + suffix)), '--help'],
                               env=env, check=True, capture_output=True, timeout=15)
        helper('Install', False)  # Never replace an existing install, even its own.
        helper('Uninstall')
        if prefix.exists():
            raise RuntimeError('Clean uninstall left the installed prefix behind')

        helper('Install')
        target = prefix / 'bin' / ('semwright.exe' if platform == 'windows' else 'semwright')
        with target.open('ab') as stream:
            stream.write(b'\nsynthetic-install-tamper-test\n')
        unknown = prefix / 'USER-OWNED.txt'
        unknown.write_text('keep this user file\n')
        helper('Uninstall', False)
        if not target.exists() or not all((prefix / name).is_file() for name in records):
            raise RuntimeError('Tamper refusal deleted a managed file')
        shutil.copyfile(stage / 'bin' / target.name, target)
        helper('Uninstall')
        if unknown.read_text() != 'keep this user file\n':
            raise RuntimeError('Uninstaller modified an unowned file')
        if any((prefix / name).exists() for name in records) or (prefix / '.semwright-install-receipt').exists():
            raise RuntimeError('Uninstall left a managed file behind')
        # TemporaryDirectory owns fixture cleanup, never the product uninstaller.
    return {'install': True, 'installed_smoke': True, 'uninstall': True, 'cleanup': True,
            'overwrite_refused': True, 'changed_files_preserved': True, 'unknown_files_preserved': True}


def verify_checksums_installed(prefix: Path, records: dict[str, str]) -> None:
    from bundle_contract import digest
    for relative, sha in records.items():
        path = prefix / relative
        if not path.is_file() or path.is_symlink() or digest(path) != sha:
            raise RuntimeError(f'installed bundle digest mismatch: {relative}')
