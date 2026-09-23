#!/usr/bin/env python3
"""Install locally built binaries for the current login user. Never enables a service."""
import argparse
import hashlib
import json
import os
import shutil
import stat
import tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
BINS=('semwright','semwrightd','semwright-mcp','semwright-inspect','semwright-sandbox')
def private(path):
    path.mkdir(parents=True,exist_ok=True,mode=0o700)
    m=path.lstat()
    if not stat.S_ISDIR(m.st_mode) or m.st_uid!=os.getuid() or m.st_mode&0o022:raise ValueError(f'Unsafe destination directory: {path}')
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--bin-dir',type=Path,required=True);a=p.parse_args()
    if os.getuid()==0:p.error('Do not install or run Semwright as root')
    home=Path.home();bindir=home/'.local/bin';state=home/'.local/share/semwright-install';private(bindir);private(state)
    legacy=bindir/'computerctl'
    if legacy.exists() or legacy.is_symlink():p.error('Legacy ~/.local/bin/computerctl exists; uninstall or review the previous Semwright installation before installing the renamed CLI')
    if (state/'manifest.json').exists():p.error('An install manifest exists; uninstall or review it before replacing files')
    validated={}
    for name in BINS:
        src=a.bin_dir/name;fd=os.open(src,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
        with os.fdopen(fd,'rb') as f:
            m=os.fstat(f.fileno())
            if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.getuid() or m.st_mode&0o022 or m.st_size>128*1024*1024:p.error('Unsafe source executable')
            body=f.read(128*1024*1024+1)
        if body[:4]!=b'\x7fELF':p.error('Expected compiled ELF binaries, not source files')
        if (bindir/name).exists() or (bindir/name).is_symlink():p.error(f'Refusing to replace {bindir/name}')
        validated[name]=body
    installed={}
    try:
        for name,body in validated.items():
            dest=bindir/name;fd=os.open(dest,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o700)
            with os.fdopen(fd,'wb') as f:f.write(body);f.flush();os.fsync(f.fileno())
            installed[str(dest)]=hashlib.sha256(body).hexdigest()
        with (state/'manifest.json').open('x') as f:json.dump(installed,f,indent=2);f.write('\n')
        (state/'manifest.json').chmod(0o600)
    except Exception:
        for name,digest in installed.items():
            dest=Path(name)
            if dest.is_file() and not dest.is_symlink() and hashlib.sha256(dest.read_bytes()).hexdigest()==digest:dest.unlink()
        raise
    print('Installed five binaries in ~/.local/bin. No service, plugin, portal permission, or configuration was enabled.')
    print('Read docs/installation.md before starting semwrightd. Existing configuration and state were untouched.')
if __name__=='__main__':main()
