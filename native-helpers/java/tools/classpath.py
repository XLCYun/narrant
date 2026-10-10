#!/usr/bin/env python3
"""Verify and print the fixed helper JAR classpath in stable lockfile order."""

import hashlib
import json
import os
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
directory = Path(sys.argv[1]).resolve()
items = json.loads((root / "dependencies.lock.json").read_text())
classpath = []
for item in items:
    path = directory / item["file"]
    if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != item["sha256"]:
        raise SystemExit(f"missing or mismatched fixed JAR: {path}")
    classpath.append(str(path))
print(os.pathsep.join(classpath))
