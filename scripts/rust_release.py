#!/usr/bin/env python3
"""Prepare and validate ForkLaunch crate releases without publishing implicitly."""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import time
import tomllib
from urllib.error import HTTPError
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[1]
REGISTRY = "https://github.com/rust-lang/crates.io-index"


def cargo_manifests(root: Path, scope: str = "framework") -> dict[str, tuple[Path, dict]]:
    result = {}
    if scope not in ("framework", "all"):
        raise ValueError(f"Unsupported release scope: {scope}")
    workspaces = ("framework-rs",) if scope == "framework" else ("framework-rs", "blueprint-rs")
    for workspace in workspaces:
        inherited = tomllib.loads((root / workspace / "Cargo.toml").read_text())["workspace"]["package"]
        for path in sorted((root / workspace / "crates").rglob("Cargo.toml")):
            manifest = tomllib.loads(path.read_text())
            package = manifest.get("package", {})
            name = package.get("name", "")
            if not name.startswith("forklaunch-"):
                continue
            if not re.fullmatch(r"forklaunch-[a-z0-9-]+", name) or name in result:
                raise ValueError(f"Invalid or duplicate package name: {name}")
            for key, value in list(package.items()):
                if isinstance(value, dict) and value.get("workspace"):
                    package[key] = inherited[key]
            result[name] = (path, manifest)
    return result


def dependency_tables(manifest: dict):
    for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
        yield kind, None, manifest.get(kind, {})
    for target, values in manifest.get("target", {}).items():
        for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
            yield kind, target, values.get(kind, {})


def release_plan(root: Path = ROOT, scope: str = "framework") -> list[dict]:
    manifests = cargo_manifests(root, scope)
    paths = {path.resolve(): name for name, (path, _) in manifests.items()}
    packages = {}
    for name, (path, manifest) in manifests.items():
        package = manifest["package"]
        if package.get("publish") is False:
            continue
        if package.get("publish") not in (None, ["crates-io"]):
            raise ValueError(f"Unsupported publication registry: {name}")
        for key in ("version", "description", "license", "repository"):
            if not isinstance(package.get(key), str) or not package[key]:
                raise ValueError(f"Missing {key}: {name}")
        dependencies = set()
        for kind, _, table in dependency_tables(manifest):
            for alias, spec in table.items():
                if not isinstance(spec, dict):
                    continue
                if "git" in spec:
                    raise ValueError(f"Unpublishable Git dependency: {name}/{alias}")
                if "path" not in spec:
                    continue
                target = (path.parent / spec["path"] / "Cargo.toml").resolve()
                if target not in paths:
                    raise ValueError(f"Uninventoried path dependency outside {scope} release scope: {name}/{alias}")
                target_name = paths[target]
                target_package = manifests[target_name][1]["package"]
                if target_package.get("publish") is False:
                    raise ValueError(f"Public crate depends on private sample: {name}/{alias}")
                if spec.get("package", alias) != target_name:
                    raise ValueError(f"Dependency package name mismatch: {name}/{alias}")
                local_dev = kind == "dev-dependencies" and "version" not in spec
                if not local_dev and spec.get("version", "").removeprefix("=") != target_package["version"]:
                    raise ValueError(f"Missing or stale path dependency version: {name}/{alias}")
                # Cargo omits unversioned dev paths when packaging. Versioned
                # dev dependencies must exist when `publish` resolves its lock.
                if not local_dev:
                    dependencies.add(target_name)
        packages[name] = {
            "name": name, "version": package["version"],
            "manifest": str(path.relative_to(root)),
            "dependencies": sorted(dependencies),
        }
    pending = {name: set(package["dependencies"]) for name, package in packages.items()}
    order = []
    while pending:
        ready = sorted(name for name, dependencies in pending.items() if not dependencies)
        if not ready:
            raise ValueError(f"Publication dependency cycle: {sorted(pending)}")
        order.extend(packages[name] for name in ready)
        for name in ready:
            del pending[name]
        for dependencies in pending.values():
            dependencies.difference_update(ready)
    return order


def run(command: list[str], **kwargs):
    subprocess.run(command, check=True, **kwargs)


def prepare(output: Path, allow_dirty: bool, scope: str = "framework"):
    plan = release_plan(scope=scope)
    dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT).strip())
    if not allow_dirty and dirty:
        raise ValueError("Release preparation requires a clean checkout (or explicit --allow-dirty for local validation)")
    output.mkdir(parents=True, exist_ok=False)
    target = output / "cargo-target"
    for package in plan:
        command = ["cargo", "package", "--manifest-path", str(ROOT / package["manifest"]),
                   "--no-verify", "--exclude-lockfile", "--target-dir", str(target)]
        if allow_dirty:
            command.append("--allow-dirty")
        run(command, cwd=ROOT)
        archive = target / "package" / f"{package['name']}-{package['version']}.crate"
        package["archive"] = str(archive.relative_to(output))
        package["sha256"] = hashlib.sha256(archive.read_bytes()).hexdigest()
        with tarfile.open(archive) as contents:
            member = f"{package['name']}-{package['version']}/Cargo.toml"
            normalized = tomllib.loads(contents.extractfile(member).read().decode())
            for _, _, table in dependency_tables(normalized):
                if any(isinstance(spec, dict) and "path" in spec for spec in table.values()):
                    raise ValueError(f"Packaged path dependency remains: {package['name']}")
    parser = ROOT / "framework-rs/crates/vendor/serde_json"
    parser_archive = output / "serde_json-1.0.151-forklaunch-literal-keys.tar.gz"
    with tarfile.open(parser_archive, "w:gz") as archive:
        for path in sorted(parser.rglob("*")):
            if path.is_file() and "target" not in path.relative_to(parser).parts:
                archive.add(path, arcname=str(Path("serde_json") / path.relative_to(parser)))
    report = {
        "schemaVersion": 1,
        "scope": scope,
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "dirty": dirty,
        "packages": plan,
        "requiredRootPatch": {
            "package": "serde_json", "version": "1.0.151", "archive": parser_archive.name,
            "sha256": hashlib.sha256(parser_archive.read_bytes()).hexdigest(),
        },
    }
    (output / "release.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Prepared {len(plan)} registry crates and the required parser patch in {output}")


def publication_preflight(output: Path, tag: str):
    from rust_registry import checked_archive
    report_bytes = (output / "release.json").read_bytes()
    report = json.loads(report_bytes)
    verified = json.loads((output / "verification.json").read_text())
    if report.get("schemaVersion") != 1 or report.get("dirty") is not False:
        raise ValueError("Publication requires a clean release report")
    if (verified.get("schemaVersion") != 1 or verified.get("httpMcpStartup") is not True
            or verified.get("releaseSha256") != hashlib.sha256(report_bytes).hexdigest()
            or verified.get("packages") != len(report["packages"])):
        raise ValueError("Missing or mismatched registry/startup verification")
    if (any(package["name"] == "forklaunch-regress" for package in report["packages"])
            and verified.get("boundedRegex") is not True):
        raise ValueError("Missing bounded regex registry verification")
    if (any(package["name"] == "forklaunch-worker-interface" for package in report["packages"])
            and verified.get("nativeHttpWorkerFeatures") is not True):
        raise ValueError("Missing native HTTP/worker feature registry verification")
    if report.get("scope") != "framework" or any(
        not package["manifest"].startswith("framework-rs/crates/")
        for package in report["packages"]
    ):
        raise ValueError("Only framework publication is enabled; blueprint crates cannot be uploaded")
    plan = release_plan(scope="framework")
    prepared = [{key: package[key] for key in ("name", "version", "manifest", "dependencies")}
                for package in report["packages"]]
    if prepared != plan:
        raise ValueError("Prepared package inventory differs from checkout")
    version = tomllib.loads((ROOT / "framework-rs/Cargo.toml").read_text())["workspace"]["package"]["version"]
    if tag != f"rust-v{version}":
        raise ValueError(f"Publication requires tag rust-v{version}")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    tagged = subprocess.check_output(["git", "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}"], cwd=ROOT, text=True).strip()
    if head != report["commit"] or tagged != head:
        raise ValueError("Release report, tag and checkout must identify the same commit")
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT).strip():
        raise ValueError("Publication requires a clean checkout")
    for package in report["packages"]:
        checked_archive(output, package)
    checked_archive(output, report["requiredRootPatch"])
    return report


def published_version(package: dict):
    name = package["name"]
    url = f"https://index.crates.io/{name[:2]}/{name[2:4]}/{name}"
    try:
        with urlopen(url, timeout=20) as response:
            entries = response.read(8 * 1024 * 1024).decode().splitlines()
    except HTTPError as error:
        if error.code == 404:
            return None
        raise
    for line in entries:
        entry = json.loads(line)
        if entry["vers"] == package["version"]:
            if entry.get("yanked"):
                raise ValueError(f"Refusing to reuse yanked version: {name} {package['version']}")
            return entry
    return None


def wait_for_version(package: dict, timeout: float = 180):
    deadline = time.monotonic() + timeout
    while True:
        entry = published_version(package)
        if entry is not None:
            return entry
        if time.monotonic() >= deadline:
            raise ValueError(f"Published version is not yet available: {package['name']}; rerun to resume")
        time.sleep(2)


def verify_published_archive(output: Path, package: dict, entry: dict) -> str:
    from rust_registry import checked_archive, safe_members
    name, version = package["name"], package["version"]
    with urlopen(f"https://static.crates.io/crates/{name}/{name}-{version}.crate", timeout=30) as response:
        published = response.read(32 * 1024 * 1024)
    checksum = hashlib.sha256(published).hexdigest()
    if checksum != entry["cksum"]:
        raise ValueError(f"Published archive checksum mismatch: {name}")

    def files(data):
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
            return {member.name: archive.extractfile(member).read()
                    for member in safe_members(archive)
                    if member.isfile() and member.name != f"{name}-{version}/Cargo.lock"}

    # Cargo may add a resolved lockfile when publishing after dependencies become
    # available. Every source/manifest/VCS-info file must match the verified package.
    if files(published) != files(checked_archive(output, package)):
        raise ValueError(f"Existing registry version differs from verified release: {name} {version}")
    return checksum


def publish(output: Path, tag: str, upload: bool):
    report = publication_preflight(output, tag)
    # Cargo resolves credentials through its configured provider, including
    # cargo login and registry-specific environment variables. Never print them.
    receipts = []
    for package in report["packages"]:
        command = ["cargo", "publish", "--manifest-path", str(ROOT / package["manifest"]),
                   "--registry", "crates-io", "--no-verify"]
        if upload:
            entry = published_version(package)
            reused = entry is not None
            if entry is None:
                print(f"Publishing {package['name']} {package['version']}", flush=True)
                run(command, cwd=ROOT)
                entry = wait_for_version(package)
            checksum = verify_published_archive(output, package, entry)
            receipts.append({"name": package["name"], "version": package["version"],
                             "registrySha256": checksum, "reusedMatchingVersion": reused})
            progress = {"schemaVersion": 1, "scope": "framework", "tag": tag,
                        "releaseSha256": hashlib.sha256((output / "release.json").read_bytes()).hexdigest(),
                        "packages": receipts, "complete": len(receipts) == len(report["packages"])}
            temporary = output / "publication.json.tmp"
            temporary.write_text(json.dumps(progress, indent=2) + "\n")
            temporary.replace(output / "publication.json")
            print(f"Verified registry archive {package['name']} ({len(receipts)}/{len(report['packages'])})", flush=True)
        else:
            print(" ".join(command))
    print("Published release in dependency order" if upload else "Preflight passed; commands shown only, no uploads performed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["plan", "prepare", "verify", "publish"])
    parser.add_argument("--output", type=Path)
    parser.add_argument("--target-dir", type=Path)
    parser.add_argument("--allow-dirty", action="store_true")
    parser.add_argument("--scope", choices=["framework", "all"], default="framework",
                        help="Package inventory scope; uploads are restricted to framework")
    parser.add_argument("--tag")
    parser.add_argument("--upload", action="store_true", help="Actually upload crates (publish only)")
    args = parser.parse_args()
    if args.command == "plan":
        print(json.dumps(release_plan(scope=args.scope), indent=2))
    elif args.command == "prepare":
        if args.output is None:
            parser.error("prepare requires --output (a new directory)")
        prepare(args.output.resolve(), args.allow_dirty, args.scope)
    elif args.command == "verify":
        if args.output is None or args.target_dir is None:
            parser.error("verify requires --output (prepared release) and --target-dir")
        from rust_registry import verify
        verify(args.output.resolve(), args.target_dir.resolve())
    else:
        if args.scope != "framework":
            parser.error("Only framework publication is enabled")
        if args.output is None or args.tag is None:
            parser.error("publish requires --output (verified release) and --tag")
        publish(args.output.resolve(), args.tag, args.upload)


if __name__ == "__main__":
    main()
