#!/usr/bin/env python3
"""Reproduce the pinned compiler target, correcting its missing singlethread flag.

Rust nightly-2026-09-04's VEX std selects no_threads synchronization but the
built-in target leaves singlethread=false. This makes build-std fail at its
new target_has_threads assertions. VEX std does not implement Rust threads.
No CPU, ABI, memory map, linker script, or feature is changed here.
"""
import json
from pathlib import Path
import subprocess

original = json.loads(subprocess.check_output([
    "rustc", "-Zunstable-options", "--print", "target-spec-json", "--target", "thumbv7a-vex-v5"
]))
assert original["os"] == "vexos" and original["cpu"] == "cortex-a9"
original["singlethread"] = True
path = Path(__file__).resolve().parents[1] / ".cargo/thumbv7a-vex-v5-chair.json"
path.write_text(json.dumps(original, indent=2) + "\n")
