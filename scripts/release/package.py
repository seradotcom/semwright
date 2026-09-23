#!/usr/bin/env python3
"""Build a local tarball and optional Debian package from already-built ELF binaries."""
import argparse
import hashlib
import shutil
import stat
import subprocess
import tarfile
import tempfile
import tomllib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
BINS=('semwright','semwrightd','semwright-mcp','semwright-inspect','semwright-sandbox')
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--bin-dir',type=Path,required=True);p.add_argument('--arch',choices=['x86_64','aarch64'],required=True);p.add_argument('--output',type=Path,default=ROOT/'dist');p.add_argument('--deb',action='store_true');a=p.parse_args()
    subprocess.run(['python3',str(ROOT/'scripts/release/assert-ready.py')],check=True)
    version=tomllib.loads((ROOT/'Cargo.toml').read_text())['workspace']['package']['version']
    a.output.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='semwright-package-') as temp:
        stage=Path(temp)/f'semwright-{version}-{a.arch}';(stage/'bin').mkdir(parents=True)
        checks={}
        for name in BINS:
            src=a.bin_dir/name;m=src.lstat()
            if not stat.S_ISREG(m.st_mode) or not m.st_mode&0o111:raise ValueError(f'Expected regular executable: {src}')
            with src.open('rb') as f:
                header=f.read(20)
            machine=int.from_bytes(header[18:20],'little') if len(header)==20 else 0
            if header[:4]!=b'\x7fELF' or machine!={'x86_64':62,'aarch64':183}[a.arch]:raise ValueError('ELF architecture mismatch')
            shutil.copy2(src,stage/'bin'/name);checks[name]=hashlib.sha256(src.read_bytes()).hexdigest()
        for name in ['README.md','VERIFY.md','LICENSE-MIT','LICENSE-APACHE','SECURITY.md']:shutil.copy2(ROOT/name,stage/name)
        shutil.copytree(ROOT/'packaging',stage/'packaging');shutil.copytree(ROOT/'config',stage/'config')
        (stage/'SHA256SUMS').write_text(''.join(f'{digest}  bin/{name}\n' for name,digest in checks.items()))
        tar=a.output/(stage.name+'.tar.gz')
        with tarfile.open(tar,'w:gz') as archive:archive.add(stage,arcname=stage.name)
        if a.deb:
            debroot=Path(temp)/'deb';(debroot/'DEBIAN').mkdir(parents=True);(debroot/'usr/bin').mkdir(parents=True)
            for name in BINS:shutil.copy2(stage/'bin'/name,debroot/'usr/bin'/name)
            docs=debroot/'usr/share/doc/semwright';docs.mkdir(parents=True)
            for name in ['LICENSE-MIT','LICENSE-APACHE','README.md']:shutil.copy2(ROOT/name,docs/name)
            arch={'x86_64':'amd64','aarch64':'arm64'}[a.arch]
            (debroot/'DEBIAN/control').write_text(f'Package: semwright\nVersion: {version.replace("-","~")}\nArchitecture: {arch}\nMaintainer: Semwright contributors\nDepends: libc6\nDescription: Policy-scoped semantic Linux automation\n No service is enabled by this package. Optional desktop dependencies are documented.\n')
            subprocess.run(['dpkg-deb','--root-owner-group','--build',str(debroot),str(a.output/f'semwright_{version}_{arch}.deb')],check=True)
    sums=[]
    for path in sorted(a.output.iterdir()):
        if path.suffix=='.deb' or path.name.endswith('.tar.gz'):sums.append(f'{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n')
    (a.output/'SHA256SUMS').write_text(''.join(sums))
    print('Built local packages; this script did not publish or install them.')
if __name__=='__main__':main()
