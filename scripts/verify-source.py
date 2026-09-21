#!/usr/bin/env python3
"""Validate source/data packaging, not Rust compilation or compositor compatibility."""
import ast
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path
import jsonschema
import yaml
ROOT=Path(__file__).resolve().parents[1]
def main():
    files=[p for p in ROOT.rglob('*') if p.is_file() and not any(s in {'target','.git','node_modules','__pycache__'} for s in p.relative_to(ROOT).parts)]
    counts={'json':0,'toml':0,'yaml':0,'python_syntax':0,'javascript_syntax':0,'shell_syntax':0,'json_schemas':0}
    for p in files:
        if p.suffix=='.json':json.loads(p.read_text());counts['json']+=1
        if p.suffix=='.toml':tomllib.loads(p.read_text());counts['toml']+=1
        if p.suffix in {'.yml','.yaml'}:yaml.safe_load(p.read_text());counts['yaml']+=1
        if p.suffix=='.py':ast.parse(p.read_text(),filename=str(p));counts['python_syntax']+=1
        if p.suffix in {'.js','.mjs'}:
            subprocess.run(['node','--check',str(p)],check=True,capture_output=True);counts['javascript_syntax']+=1
        if p.suffix=='.sh':subprocess.run(['bash','-n',str(p)],check=True);counts['shell_syntax']+=1
    commands=json.loads((ROOT/'schemas/commands.json').read_text())
    for c in commands:
        for k in ('input_schema','output_schema'):
            jsonschema.Draft202012Validator.check_schema(c[k]);counts['json_schemas']+=1
    for p in (ROOT/'schemas').rglob('*.schema.json'):
        jsonschema.Draft202012Validator.check_schema(json.loads(p.read_text()));counts['json_schemas']+=1
    subprocess.run([sys.executable,str(ROOT/'scripts/sync-contracts.py'),'--check'],check=True)
    # All local Markdown document links must resolve. Anchor targets are not checked.
    broken=[]
    for p in files:
        if p.suffix!='.md' or 'requirements' in p.parts:continue
        for link in re.findall(r'(?<!!)\[[^\]]*\]\(([^)]+)\)',p.read_text()):
            if '://' in link or link.startswith(('#','mailto:')):continue
            dest=link.split('#',1)[0]
            if dest and not (p.parent/dest).exists():broken.append(f'{p.relative_to(ROOT)} -> {dest}')
    if broken:raise ValueError('Broken local links: '+', '.join(broken))
    # Keep configuration examples tied to the actual owner-config keys.
    allowed={'policy','applications','browser','blender_socket','plugins','plugin_network','audit_max_bytes','audit_retention'}
    for p in (ROOT/'config').glob('*.toml'):
        extra=set(tomllib.loads(p.read_text()))-allowed
        if extra:raise ValueError(f'Unknown configuration keys in {p.name}: {extra}')
    print(json.dumps({'source_validation':'PASS','counts':counts,'rust_compiled':False},indent=2))
if __name__=='__main__':
    try:main()
    except (ValueError,OSError,subprocess.CalledProcessError,SyntaxError) as e:
        print(f'Source validation failed: {e}',file=sys.stderr);sys.exit(1)
