#!/usr/bin/env python3
"""Replay acceptance against this repository's running local Compose project."""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def run(command, script=None):
    print(f"Checking {script or command[-1]}", flush=True)
    subprocess.run(
        command, cwd=ROOT, check=True, timeout=240,
        input=(ROOT / "scripts" / script).read_bytes() if script else None,
    )


for name in ("check-auth.mjs", "check-auth-security.mjs", "check-api.mjs",
             "check-mcp.mjs", "check-live-auth.mjs"):
    run(["docker", "compose", "exec", "-T",
         "-e", "FIELDNOTES_PG_PACKAGE=pg", "-e", "FIELDNOTES_WS_PACKAGE=ws",
         "auth", "node", "--input-type=module"], name)
run(["docker", "compose", "exec", "-T", "postgres", "sh", "-s"], "check-rls.sh")
run(["sh", "scripts/check-storage.sh"])
print("PASS local auth, HTTP, MCP, worker, live-channel and storage acceptance")
