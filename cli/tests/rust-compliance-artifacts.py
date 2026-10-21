#!/usr/bin/env python3
"""Exercise declaration validation and offline audit through the real CLI.
Uses an existing generated application's manifest as the fixture base; never edits
that application or accesses its credentials, databases, or containers.
"""
import argparse
import copy
import json
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("--cli", type=Path, required=True)
parser.add_argument("--manifest", type=Path, required=True)
args = parser.parse_args()
cli = str(args.cli.resolve())
valid = {"schemaVersion": 1, "coverage": "declared-storage-members-only",
         "runtimeEnforcement": "requires-independent-verification", "lifecycleCoverage": "unknown",
         "entities": [{"entity": "Contact", "storage": "contacts", "kind": "postgres",
                       "fields": [{"member": "email", "column": "email_ciphertext", "classification": "pii", "nullable": True}]}]}
with tempfile.TemporaryDirectory(prefix="rust-compliance-cli-") as temporary:
    root = Path(temporary)
    (root / ".forklaunch").mkdir()
    (root / ".forklaunch/manifest.toml").write_text(args.manifest.read_text())
    # Preset manifest owns these Rust runtime paths. The fixture classifications
    # are deliberately synthetic; a parsed declaration is never encryption proof.
    for name in ("notes", "insights"):
        folder = root / "src" / name
        folder.mkdir(parents=True)
        (folder / "Cargo.toml").write_text(f'[package]\nname="{name}"\nversion="0.1.0"\n')
        (folder / "compliance.json").write_text(json.dumps(valid))

    def run(command, expected=0):
        result = subprocess.run([cli, "compliance", *command, "--path", str(root), "--json"], text=True, capture_output=True)
        assert result.returncode == expected, (result.returncode, result.stderr, result.stdout)
        return json.loads(result.stdout)

    report = run(["validate-artifacts"])
    assert len(report["projects"]) == 2
    assert all(p["status"] == "valid-declarations" for p in report["projects"])
    report = run(["audit", "--offline"])
    assert len(report["rustDeclarations"]["projects"]) == 2
    assert all(p["encryptionVerification"] == "unknown" for p in report["rustDeclarations"]["projects"])
    assert "riskScore" not in report
    bad = copy.deepcopy(valid)
    bad["runtimeEnforcement"] = "certified"
    (root / "src/notes/compliance.json").write_text(json.dumps(bad))
    report = run(["validate-artifacts"], 1)
    assert next(p for p in report["projects"] if p["project"] == "notes")["status"] == "invalid"
    (root / "src/notes/compliance.json").write_text(json.dumps(valid))
    (root / "src/insights/compliance.json").unlink()
    report = run(["validate-artifacts"], 1)
    assert next(p for p in report["projects"] if p["project"] == "insights")["status"] == "missing"
    report = run(["audit", "--offline"])
    assert next(p for p in report["rustDeclarations"]["projects"] if p["project"] == "insights")["status"] == "missing"
print("PASS: real CLI valid/invalid/missing declarations, offline audit, unknown runtime coverage, no invented score")
