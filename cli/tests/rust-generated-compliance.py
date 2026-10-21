#!/usr/bin/env python3
"""Generate actual Rust services/workers and verify compile-time storage artifacts."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("--cli", type=Path, required=True)
args = parser.parse_args()
cli = str(args.cli.resolve())
repo = Path(__file__).resolve().parents[2]

def run(*command, cwd=None, expected=0):
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True)
    assert result.returncode == expected, (command, result.stdout[-6000:], result.stderr[-6000:])
    return result.stdout

with tempfile.TemporaryDirectory(prefix="rust-generated-compliance-") as temporary:
    app = Path(temporary) / "app"
    run(cli, "init", "application", "classification-app", "--path", str(app), "--language", "rust", "-D", "Classification fixture", "-A", "ForkLaunch", "-L", "MIT")
    run(cli, "init", "service", "health", "--path", str(app), "--language", "rust", "-D", "Health")
    run(cli, "init", "worker", "jobs", "--path", str(app), "--language", "rust", "-t", "database", "-d", "postgresql", "-D", "Jobs")
    def build():
        run("python3", str(repo / "cli/tests/fixtures/worker_runtime_lock.py"), "-c", "cargo +stable check --offline -p health -p jobs", cwd=app)
    def validate(expected=0):
        return json.loads(run(cli, "compliance", "validate-artifacts", "--path", str(app), "--json", expected=expected))
    assert all(p["status"] == "missing" for p in validate(1)["projects"])
    build()
    report = validate()
    assert len(report["projects"]) == 2
    health = json.loads((app / "src/health/compliance.json").read_text())
    jobs = json.loads((app / "src/jobs/compliance.json").read_text())
    assert health["entities"] == []  # Explicit health-only declaration in source.
    envelope = next(e for e in jobs["entities"] if e["entity"] == "SealedEvent")
    assert {f["member"]: f["classification"] for f in envelope["fields"]} == {"channel": "none", "context": "none", "ciphertext": "pii"}
    assert (app / "src/jobs/events/event-contract.json").exists() or list((app / "src/jobs").glob("**/event-contract.json")), "existing event build output preserved"
    # Generated ownership hook is live: a declared storage change recompiles metadata.
    (app / "src/health/persistence/compliance_schema.rs").write_text('''use forklaunch_core::persistence::compliance::{ComplianceEntity,ComplianceMetadata};
#[allow(dead_code)]
#[derive(ComplianceMetadata)]
struct Contact { #[compliance(level="pii")] email:String }
pub fn declarations()->Vec<ComplianceEntity>{vec![ComplianceEntity::of::<Contact>("contacts","postgres")]}
''')
    (app / "src/jobs/compliance.json").unlink()
    assert next(p for p in validate(1)["projects"] if p["project"] == "jobs")["status"] == "missing"
    build()
    report = validate()
    contact = next(p for p in report["projects"] if p["project"] == "health")["declarations"]["entities"][0]
    assert contact["storage"] == "contacts"
    assert contact["fields"][0]["classification"] == "pii"
print("PASS: fresh service/worker compile artifacts, explicit health scope, actual queue metadata, deleted-file regeneration and schema evolution")
