#!/usr/bin/env python3
"""Remove only unchanged files recorded by install.py; retain all user data."""
import hashlib
import json
import os
import stat
from pathlib import Path
if os.getuid()==0:raise SystemExit('Run as the user who installed Semwright, not root')
home=Path.home();manifest=home/'.local/share/semwright-install/manifest.json'
fd=os.open(manifest,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
with os.fdopen(fd) as f:
    m=os.fstat(f.fileno())
    if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.getuid() or m.st_mode&0o077:raise SystemExit('Unsafe install manifest')
    files=json.load(f)
allowed={'computerctl','semwrightd','semwright-mcp','semwright-inspect','semwright-sandbox'}
for name,digest in files.items():
    path=Path(name)
    if path.parent!=home/'.local/bin' or path.name not in allowed:raise SystemExit('Unexpected install manifest path')
    if path.exists() or path.is_symlink():
        m=path.lstat()
        if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.getuid() or hashlib.sha256(path.read_bytes()).hexdigest()!=digest:raise SystemExit('File changed; refusing to remove '+str(path))
for name in files:
    path=Path(name)
    if path.exists():path.unlink()
manifest.unlink()
print('Removed recorded unchanged binaries. User config/state, desktop extensions and service units were retained.')
