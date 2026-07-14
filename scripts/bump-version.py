#!/usr/bin/env python3
"""Bump the workspace version (root Cargo.toml) and the Tauri bundle version
in lockstep. Used by the release-pr workflow. Usage: bump-version.py 0.2.0"""
import json
import re
import sys

v = sys.argv[1]

cargo = open("Cargo.toml").read()
cargo = re.sub(r'(?m)^version = ".*"', f'version = "{v}"', cargo, count=1)
open("Cargo.toml", "w").write(cargo)

conf_path = "crates/vena-app/tauri.conf.json"
try:
    conf = json.load(open(conf_path))
    conf["version"] = v
    with open(conf_path, "w") as f:
        json.dump(conf, f, indent=2, ensure_ascii=False)
        f.write("\n")
except FileNotFoundError:
    pass

print(f"bumped to {v}")
