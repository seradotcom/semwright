#!/usr/bin/env python3
import hashlib, json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEMO = ROOT / 'demos' / 'launch-film'
data = json.loads((DEMO / 'ASSETS.json').read_text())

def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

checks = [
    (DEMO / data['semantic_source']['file'], data['semantic_source']['sha256']),
    (DEMO / data['runtime_lock']['file'], data['runtime_lock']['sha256']),
]
sound = next(x for x in data['assets'] if x['file'] == 'launch-sound.wav')
checks.append((ROOT / sound['source'], sound['generator_sha256']))
for path, expected in checks:
    assert sha(path.resolve()) == expected, path
print('launch source hashes verified')

import subprocess, tempfile
with tempfile.TemporaryDirectory() as tmp:
    wav = Path(tmp) / 'launch-sound.wav'
    subprocess.run([str(ROOT / sound['source']), str(wav)], check=True)
    assert sha(wav) == sound['sha256'], wav
print('deterministic launch audio verified')
