"""Exercise the shipped helpers against an extracted bundle on its native OS."""
from __future__ import annotations

import json
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
        if platform == 'linux':
            # Hosted runners may define XDG_CONFIG_HOME outside the disposable HOME.
            # Pin it to the fixture so setup cannot write into runner-owned state and the
            # acceptance check observes the same platform-native path as the child process.
            env['XDG_CONFIG_HOME'] = str(home / '.config')
            Path(env['XDG_CONFIG_HOME']).mkdir(parents=True, exist_ok=True)
        if platform == 'windows':
            # Keep first-run setup completely inside this disposable user fixture.
            env['USERPROFILE'] = str(home)
            env['APPDATA'] = str(home / 'AppData' / 'Roaming')
            env['LOCALAPPDATA'] = str(home / 'AppData' / 'Local')
            Path(env['APPDATA']).mkdir(parents=True, exist_ok=True)
            Path(env['LOCALAPPDATA']).mkdir(parents=True, exist_ok=True)
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

        suffix = '.exe' if platform == 'windows' else ''
        semwright = prefix / 'bin' / ('semwright' + suffix)
        expected_mcp = prefix / 'bin' / ('semwright-mcp' + suffix)
        if platform == 'windows':
            setup_dir = Path(env['APPDATA']) / 'Semwright' / 'config'
        elif platform == 'macos':
            setup_dir = home / 'Library' / 'Application Support' / 'Semwright' / 'config'
        else:
            setup_dir = home / '.config' / 'semwright'
        setup_config = setup_dir / 'daemon.toml'
        setup_mcp = setup_dir / 'mcp-client.json'

        dry = subprocess.run(
            [str(semwright), '--dry-run', '--json', 'setup'],
            env=env, capture_output=True, text=True, timeout=30, check=True,
        )
        dry_report = json.loads(dry.stdout)
        if (dry_report.get('setup') != 'dry-run'
                or dry_report.get('authority_changed') is not False
                or dry_report.get('service_started') is not False
                or dry_report.get('permissions_granted') is not False
                or dry_report.get('config', {}).get('status') != 'would-create'
                or dry_report.get('mcp_client_snippet', {}).get('status') != 'would-create'
                or setup_config.exists() or setup_mcp.exists()):
            raise RuntimeError('semwright setup dry-run was not side-effect-free')

        first_setup = subprocess.run(
            [str(semwright), '--json', 'setup'],
            env=env, capture_output=True, text=True, timeout=30, check=True,
        )
        first_report = json.loads(first_setup.stdout)
        if (first_report.get('setup') != 'complete'
                or first_report.get('authority_changed') is not False
                or first_report.get('service_started') is not False
                or first_report.get('permissions_granted') is not False
                or first_report.get('config', {}).get('status') != 'created'
                or first_report.get('mcp_client_snippet', {}).get('status') != 'created'):
            raise RuntimeError('semwright setup did not create the bounded first-run state')
        if setup_config.read_text(encoding='utf-8').splitlines()[-2:] != ['[policy]', 'profile = "observe"']:
            raise RuntimeError('semwright setup did not create observe-only policy')
        snippet = json.loads(setup_mcp.read_text(encoding='utf-8'))
        if Path(snippet['mcpServers']['semwright']['command']) != expected_mcp:
            raise RuntimeError('semwright setup MCP snippet does not bind the installed sibling binary')
        before_config = setup_config.read_bytes()
        before_mcp = setup_mcp.read_bytes()

        second_setup = subprocess.run(
            [str(semwright), '--json', 'setup'],
            env=env, capture_output=True, text=True, timeout=30, check=True,
        )
        second_report = json.loads(second_setup.stdout)
        if (second_report.get('config', {}).get('status') != 'kept-existing'
                or second_report.get('mcp_client_snippet', {}).get('status') != 'kept-existing'
                or setup_config.read_bytes() != before_config
                or setup_mcp.read_bytes() != before_mcp):
            raise RuntimeError('semwright setup is not idempotent/non-overwriting')

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
    return {'install': True, 'setup': True, 'installed_smoke': True, 'uninstall': True,
            'cleanup': True, 'overwrite_refused': True, 'changed_files_preserved': True,
            'unknown_files_preserved': True}


def verify_checksums_installed(prefix: Path, records: dict[str, str]) -> None:
    from bundle_contract import digest
    for relative, sha in records.items():
        path = prefix / relative
        if not path.is_file() or path.is_symlink() or digest(path) != sha:
            raise RuntimeError(f'installed bundle digest mismatch: {relative}')
