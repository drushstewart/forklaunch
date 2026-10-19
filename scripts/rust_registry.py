"""Loopback sparse registry for validating prepared crates before publication.

Only owned dependency registry locations change in staged archives. Original
release archives remain intact; external dependencies still use crates.io.
"""
from __future__ import annotations

import contextlib
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
import os
from pathlib import Path, PurePosixPath
import tarfile
import threading
import tomllib

from rust_release import ROOT, REGISTRY, dependency_tables, run


def toml_value(value):
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, (int, float)):
        return str(value)
    if isinstance(value, list):
        return "[" + ", ".join(map(toml_value, value)) + "]"
    if isinstance(value, dict):
        return "{ " + ", ".join(f"{toml_value(k)} = {toml_value(v)}" for k, v in value.items()) + " }"
    raise ValueError(f"Unsupported manifest value: {type(value)}")


def manifest_text(manifest):
    text = "\n".join(f"{toml_value(k)} = {toml_value(v)}" for k, v in manifest.items()) + "\n"
    if tomllib.loads(text) != manifest:
        raise ValueError("Manifest serialization changed values")
    return text.encode()


def checked_archive(directory, entry):
    path = (directory / entry["archive"]).resolve()
    if not path.is_relative_to(directory.resolve()):
        raise ValueError("Archive path escapes release directory")
    contents = path.read_bytes()
    if hashlib.sha256(contents).hexdigest() != entry["sha256"]:
        raise ValueError(f"Archive checksum mismatch: {path.name}")
    return contents


def safe_members(archive):
    for member in archive.getmembers():
        path = PurePosixPath(member.name)
        if path.is_absolute() or ".." in path.parts or not (member.isfile() or member.isdir()):
            raise ValueError(f"Unsafe archive member: {member.name}")
        yield member


def stage_package(contents, package, names):
    prefix = f"{package['name']}-{package['version']}/"
    with tarfile.open(fileobj=io.BytesIO(contents), mode="r:gz") as source:
        members = list(safe_members(source))
        if any(not member.name.startswith(prefix) for member in members):
            raise ValueError("Package archive has unexpected root")
        manifest = tomllib.loads(source.extractfile(prefix + "Cargo.toml").read().decode())
        if (manifest["package"]["name"], manifest["package"]["version"]) != (package["name"], package["version"]):
            raise ValueError("Archive package identity mismatch")
        dependencies = []
        for kind, target, table in dependency_tables(manifest):
            for alias, spec in table.items():
                if isinstance(spec, str):
                    spec = table[alias] = {"version": spec}
                name = spec.get("package", alias)
                if any(key in spec for key in ("path", "git", "registry", "registry-index")):
                    raise ValueError(f"Unexpected dependency source in prepared crate: {alias}")
                if name in names:
                    spec["registry"] = "forklaunch-staging"
                dependencies.append({
                    "name": alias, "req": spec["version"], "features": spec.get("features", []),
                    "optional": spec.get("optional", False),
                    "default_features": spec.get("default-features", True), "target": target,
                    "kind": {"dependencies": "normal", "build-dependencies": "build", "dev-dependencies": "dev"}[kind],
                    "registry": None if name in names else REGISTRY,
                    "package": spec.get("package"),
                })
        staged = io.BytesIO()
        with tarfile.open(fileobj=staged, mode="w:gz") as destination:
            for member in members:
                data = source.extractfile(member).read() if member.isfile() else None
                if member.name == prefix + "Cargo.toml":
                    data = manifest_text(manifest)
                    member.size = len(data)
                destination.addfile(member, io.BytesIO(data) if data is not None else None)
    data = staged.getvalue()
    index = {
        "name": package["name"], "vers": package["version"], "deps": dependencies,
        "cksum": hashlib.sha256(data).hexdigest(), "features": {},
        "features2": manifest.get("features", {}), "v": 2, "yanked": False,
    }
    for key in ("links", "rust-version"):
        if key in manifest["package"]:
            index[key.replace("-", "_")] = manifest["package"][key]
    return data, index


@contextlib.contextmanager
def registry(directory, report):
    responses = {}
    names = {package["name"] for package in report["packages"]}
    if len(names) != len(report["packages"]):
        raise ValueError("Duplicate release package")
    for package in report["packages"]:
        data, entry = stage_package(checked_archive(directory, package), package, names)
        name = package["name"]
        responses[f"/{name[:2]}/{name[2:4]}/{name}"] = (json.dumps(entry) + "\n").encode()
        responses[f"/api/v1/crates/{name}/{package['version']}/download"] = data

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            body = responses.get(self.path)
            if body is None:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Content-Type", "application/octet-stream")
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    base = f"http://127.0.0.1:{server.server_port}"
    responses["/config.json"] = json.dumps({"dl": base + "/api/v1/crates/{crate}/{version}/download"}).encode()
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield "sparse+" + base + "/"
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


def verify(directory: Path, target: Path):
    report = json.loads((directory / "release.json").read_text())
    consumer = directory / "registry-consumer"
    consumer.mkdir(exist_ok=False)
    patch = checked_archive(directory, report["requiredRootPatch"])
    with tarfile.open(fileobj=io.BytesIO(patch), mode="r:gz") as archive:
        members = list(safe_members(archive))
        if any(PurePosixPath(m.name).parts[0] != "serde_json" for m in members):
            raise ValueError("Unexpected parser artifact root")
        archive.extractall(consumer, members=members, filter="data")
    dependencies = {
        package["name"]: {"version": "=" + package["version"], "registry": "forklaunch-staging"}
        for package in report["packages"]
    }
    for name in ("forklaunch-jsonschema-protocol", "forklaunch-referencing-protocol", "forklaunch-bullmq-protocol"):
        if name in dependencies:
            dependencies[name]["default-features"] = False
    dependencies["forklaunch-axum"]["features"] = ["mcp", "native-server", "native-streams"]
    dependencies["forklaunch-core"]["features"] = ["postgres", "postgres09", "encryption", "otlp", "build", "typed-schema", "mappers"]
    native_worker = "forklaunch-worker-interface" in dependencies
    if native_worker:
        dependencies["forklaunch-worker-interface"]["features"] = ["native", "apalis", "schema", "telemetry"]
    bounded_regex = "forklaunch-regress" in dependencies
    if bounded_regex:
        dependencies["forklaunch-common"]["features"] = ["zod-regex"]
    dependencies.update({
        "tokio": {"version": "1", "features": ["rt-multi-thread", "macros", "signal", "net"]},
        "axum": "0.8", "serde_json": {"version": "=1.0.151", "features": ["raw_value", "arbitrary_precision"]},
    })
    (consumer / "Cargo.toml").write_bytes(manifest_text({
        "package": {"name": "forklaunch-registry-smoke", "version": "0.0.0", "edition": "2024", "publish": False},
        "features": {"bounded-regex": [], "default": ["bounded-regex"] if bounded_regex else []},
        "dependencies": dependencies, "patch": {"crates-io": {"serde_json": {"path": "serde_json"}}},
    }))
    (consumer / "src").mkdir()
    (consumer / "src/main.rs").write_text((ROOT / "scripts/rust_registry_smoke.rs").read_text())
    (consumer / ".cargo").mkdir()
    env = {**os.environ, "CARGO_INCREMENTAL": "0", "CARGO_BUILD_JOBS": os.environ.get("CARGO_BUILD_JOBS", "2")}
    with registry(directory, report) as index:
        (consumer / ".cargo/config.toml").write_bytes(manifest_text({"registries": {"forklaunch-staging": {"index": index}}}))
        run(["cargo", "build", "--target-dir", str(target)], cwd=consumer, env=env)
        import subprocess
        metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--locked"], cwd=consumer, env=env))
        resolved = {p["name"]: p for p in metadata["packages"]}
        for package in report["packages"]:
            actual = resolved[package["name"]]
            if actual["version"] != package["version"] or not actual["source"] or index.removeprefix("sparse+") not in actual["source"]:
                raise ValueError(f"Crate was not installed from staging registry: {package['name']}")
        for package in metadata["packages"]:
            if package["source"] is None and package["name"] not in ("forklaunch-registry-smoke", "serde_json"):
                raise ValueError(f"Unexpected local dependency: {package['name']}")
        run(["node", str(ROOT / "cli/tests/rust_mcp_service_startup.mjs"), str(target / "debug/forklaunch-registry-smoke")], cwd=consumer)
    (directory / "verification.json").write_text(json.dumps({
        "schemaVersion": 1, "releaseSha256": hashlib.sha256((directory / "release.json").read_bytes()).hexdigest(),
        "packages": len(report["packages"]), "httpMcpStartup": True,
        "boundedRegex": bounded_regex,
        "nativeHttpWorkerFeatures": native_worker,
        "scope": "local sparse registry, default features plus core PostgreSQL0.8+0.9/protected disclosure/OTLP, Axum MCP/native-server/native-streams " +
                 (" and native worker lifecycle/Apalis/schema/telemetry" if native_worker else "") +
                 (" and checked common regex" if bounded_regex else "") +
                 "; owned dependency registry metadata rewritten",
    }, indent=2) + "\n")
    print(f"Verified {len(report['packages'])} registry-installed crates and HTTP/MCP startup")
