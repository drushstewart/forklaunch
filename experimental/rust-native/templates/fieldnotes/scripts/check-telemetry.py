#!/usr/bin/env python3
"""Opt-in local OTLP proof; temporarily restarts this Compose project's API/worker.

Requires the existing generated local stack. Never publishes or exposes a collector.
Restores the ordinary Compose configuration afterward. Receipt contains no payloads.
"""
import json
from pathlib import Path
import subprocess
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
BASE = ["docker", "compose"]


def run(args, **kwargs):
    return subprocess.run(args, cwd=ROOT, check=True, timeout=180, **kwargs)


def node(source, *, capture=False, env=()):
    return run(BASE + ["exec", "-T", *env, "auth", "node", "--input-type=module"],
               input=source, text=True, capture_output=capture)


def main():
    name = "notes-telemetry-" + uuid.uuid4().hex
    remote = f"/tmp/{name}.cjs"
    port_file = f"/tmp/{name}.port"
    port = None
    overridden = False
    with tempfile.TemporaryDirectory(prefix=name) as temporary:
        override = Path(temporary) / "telemetry.json"
        try:
            run(BASE + ["cp", str(ROOT / "scripts/telemetry-recorder.cjs"), f"auth:{remote}"], stdout=subprocess.DEVNULL)
            run(BASE + ["exec", "-d", "auth", "node", remote, port_file])
            for _ in range(30):
                result = node("import fs from 'node:fs'; console.log(fs.existsSync(" + json.dumps(port_file) + ")?fs.readFileSync(" + json.dumps(port_file) + ",'utf8'):'');", capture=True)
                if result.stdout.strip():
                    port = int(result.stdout.strip())
                    break
                time.sleep(0.2)
            if port is None:
                raise RuntimeError("Local telemetry receiver did not start")
            environment = {"OTEL_EXPORTER_OTLP_ENDPOINT": f"http://auth:{port}",
                           "OTEL_EXPORTER_OTLP_PROTOCOL": "http/json",
                           "OTEL_BSP_SCHEDULE_DELAY": "100", "OTEL_BLRP_SCHEDULE_DELAY": "100",
                           "OTEL_METRIC_EXPORT_INTERVAL": "1000"}
            override.write_text(json.dumps({"services": {s: {"environment": environment} for s in ("notes", "insights")}}))
            overridden = True
            run(BASE + ["-f", "compose.yaml", "-f", str(override), "up", "-d", "--no-build", "--no-deps", "notes", "insights"], stdout=subprocess.DEVNULL)
            for attempt in range(90):
                ready = node("try {const r=await fetch('http://notes:8000/health');console.log(r.status===204?'ready':'waiting');}catch{console.log('waiting');}", capture=True)
                if ready.stdout.strip() == "ready":
                    break
                if attempt == 89:
                    raise RuntimeError("API did not restart")
                time.sleep(1)
            result = node((ROOT / "scripts/check-telemetry-request.mjs").read_text(), capture=True,
                          env=("-e", f"FIELDNOTES_TELEMETRY_PORT={port}"))
            receipt = json.loads(result.stdout)
            destination = ROOT / "artifacts/local-run/telemetry-receipt.json"
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(json.dumps(receipt, indent=2) + "\n")
            print("PASS local OTLP/HTTP JSON traces, metrics, logs; encrypted API-to-worker parent correlation; sensitive-data redaction")
        finally:
            try:
                if overridden:
                    run(BASE + ["up", "-d", "--no-build", "--no-deps", "notes", "insights"], stdout=subprocess.DEVNULL)
            finally:
                if port is not None:
                    node(f"await fetch('http://localhost:{port}/shutdown');", capture=True)
                run(BASE + ["exec", "-T", "-u", "0", "auth", "rm", "-f", remote, port_file], stdout=subprocess.DEVNULL)


if __name__ == "__main__":
    main()
