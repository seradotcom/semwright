#!/usr/bin/env python3
"""Fail closed: development snapshots cannot become release artifacts implicitly."""
import json
import sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
readiness=json.loads((ROOT/'release-readiness.json').read_text())
failed=[k for k,v in readiness['gates'].items() if v is not True]
if not (ROOT/'Cargo.lock').is_file():failed.append('Cargo.lock present')
if failed:
    print('Release blocked: '+', '.join(failed),file=sys.stderr);sys.exit(2)
print('Release admission metadata complete; CI must still run all gates on this commit.')
