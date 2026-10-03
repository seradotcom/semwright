"""The one portable payload contract; no unbounded repository/config tree copying."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat

ROOT = Path(__file__).resolve().parents[2]
CONTRACT = json.loads((ROOT / 'packaging/bundle-contract.json').read_text())
BINS = tuple(CONTRACT['binaries'])


def digest(path: Path) -> str:
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def add_bundle_files(root: Path, stage: Path, platform: str, epoch: int) -> None:
    files = {name: root / name for name in CONTRACT['documents']}
    files.update({'INSTALL.md': root / 'docs/installation.md',
                  'BUNDLE-CONTRACT.json': root / 'packaging/bundle-contract.json',
                  'config/observe.toml': root / 'config/observe.toml'})
    helpers = CONTRACT['windows_helpers' if platform == 'windows' else 'posix_helpers']
    files.update({name: root / 'packaging/portable' / name for name in helpers})
    for relative, source in files.items():
        if not stat.S_ISREG(source.lstat().st_mode):
            raise ValueError(f'nonregular bundle input: {source}')
        target = stage / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        target.chmod(0o755 if relative.endswith('.sh') else 0o644)
        os.utime(target, (epoch, epoch))


def write_checksums(stage: Path, epoch: int) -> None:
    paths = sorted(stage.rglob('*'), key=lambda p: p.relative_to(stage).as_posix())
    records = []
    for path in paths:
        if path.is_symlink():
            raise ValueError('bundle must not contain symlinks')
        if path.is_file() and path.name != 'SHA256SUMS':
            records.append(f'{digest(path)}  {path.relative_to(stage).as_posix()}\n')
    (stage / 'SHA256SUMS').write_text(''.join(records), encoding='utf-8', newline='\n')
    os.utime(stage / 'SHA256SUMS', (epoch, epoch))


def verify_checksums(stage: Path) -> dict[str, str]:
    if stage.is_symlink():
        raise ValueError('symlink bundle root')
    stage = stage.resolve()
    manifest = stage / 'SHA256SUMS'
    if manifest.is_symlink() or not manifest.is_file() or manifest.stat().st_size > 131072:
        raise ValueError('invalid internal checksum manifest')
    records = {}
    for line in manifest.read_text(encoding='utf-8').splitlines():
        match = re.fullmatch(r'([0-9a-f]{64})  ([A-Za-z0-9._/-]+)', line)
        if not match:
            raise ValueError('malformed internal checksum record')
        sha, name = match.groups()
        relative = PurePosixPath(name)
        if (relative.is_absolute() or any(part in ('.', '..', '') for part in name.split('/'))
                or name == 'SHA256SUMS' or name.casefold() in {n.casefold() for n in records}):
            raise ValueError('unsafe or duplicate checksum path')
        path = stage / name
        if any(parent.is_symlink() for parent in (path, *path.parents)) or not path.is_file():
            raise ValueError('nonregular checksum payload')
        if digest(path) != sha:
            raise ValueError(f'internal checksum mismatch: {name}')
        records[name] = sha
    actual = {p.relative_to(stage).as_posix() for p in stage.rglob('*')
              if p.is_file() and p != manifest}
    if not records or set(records) != actual:
        raise ValueError('checksum manifest must enumerate the complete payload')
    return records
