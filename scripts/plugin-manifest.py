#!/usr/bin/env python3
"""Pin an example plugin manifest to an actual compiled, user-owned ELF file."""
import argparse
import hashlib
import json
import os
import stat
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('executable',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    try:
        path=a.executable.absolute()
        fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
        with os.fdopen(fd,'rb') as f:
            m=os.fstat(f.fileno())
            if not stat.S_ISREG(m.st_mode) or m.st_uid!=os.getuid() or m.st_mode&0o022 or m.st_nlink!=1 or not m.st_mode&0o111:
                p.error('Executable must be a single-link user-owned executable regular file, not group/world-writable')
            if m.st_size>128*1024*1024:p.error('Executable exceeds 128 MiB')
            data=f.read(128*1024*1024+1)
        if not data.startswith(b'\x7fELF'):p.error('Not an ELF executable; compile the example plugin first')
        manifest=json.loads((ROOT/'adapters/example-plugin/manifest.template.json').read_text())
        manifest.update(executable=str(path),sha256=hashlib.sha256(data).hexdigest())
        fd=os.open(a.output,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
        with os.fdopen(fd,'w') as f:json.dump(manifest,f,indent=2);f.write('\n');f.flush();os.fsync(f.fileno())
        print(manifest['sha256'])
    except OSError as e:p.exit(1,f'Cannot create manifest: {e}\n')
if __name__=='__main__':main()
