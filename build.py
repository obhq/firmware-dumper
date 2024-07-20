#!/usr/bin/env python3
import json
import os
from subprocess import Popen, PIPE, run
import sys

# Build.
args = [
    "cargo", "+nightly", "build",
    "--target", "x86_64-unknown-none",
    "-r",
    "-Z", "build-std=alloc,core,panic_abort",
    "-Z", "build-std-features=panic_immediate_abort",
    "--message-format", "json-render-diagnostics"
]

with Popen(args, cwd="dumper", env=dict(os.environ, RUSTFLAGS="--cfg fw=\"1100\""), stdout=PIPE) as proc:
    for line in proc.stdout:
        line = json.loads(line)
        reason = line["reason"]
        if reason == "build-finished":
            if line["success"]:
                break
            else:
                sys.exit(1)
        elif reason == "compiler-artifact":
            if line["target"]["name"] == "dumper":
                out = line["executable"]

# Create payload.
run(["rustup", "run", "nightly", "objcopy", "-O", "binary", out, "firmware-dumper.bin"], check=True)
