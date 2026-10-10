#!/usr/bin/env python3
"""Fetch fixed helper build JARs; never resolve a reviewed project's dependencies."""

import hashlib
import json
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOCK = json.loads((ROOT / "dependencies.lock.json").read_text())
OUT = ROOT.parents[1] / ".tools" / "java-native" / "lib"
BASE = "https://repo.maven.apache.org/maven2"
OUT.mkdir(parents=True, exist_ok=True)

for item in LOCK:
    path = OUT / item["file"]
    if path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() == item["sha256"]:
        continue
    stem = "/".join([item["group"].replace(".", "/"), item["artifact"], item["version"], item["file"]])
    with urllib.request.urlopen(f"{BASE}/{stem}", timeout=120) as response:
        data = response.read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != item["sha256"]:
        raise RuntimeError(f"checksum mismatch: {item['file']}")
    path.write_bytes(data)
    print(f"{item['file']} {len(data)}", file=sys.stderr)
