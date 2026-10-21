#!/usr/bin/env python3
"""Fresh ordinary CLI mixed-app production image + HTTP/MCP startup proof.

Requires a built CLI, the selected Node/pnpm or Bun runtime, and local Docker.
Uses disposable containers, local
ports and an isolated PostgreSQL database; never deploys to an external host.
"""
import json
import os
from pathlib import Path
import shutil
import shlex
import sys
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

REPO = Path(__file__).resolve().parents[2]
CLI = Path(os.environ.get("FORKLAUNCH_CLI", REPO / "cli/target/debug/forklaunch"))
HTTP_FRAMEWORK = os.environ.get("FORKLAUNCH_HTTP_FRAMEWORK", "express")
assert HTTP_FRAMEWORK in ("express", "hyper-express")
RUNTIME = os.environ.get("FORKLAUNCH_TYPESCRIPT_RUNTIME", "node")
assert RUNTIME in ("node", "bun")
PM = "bun" if RUNTIME == "bun" else "pnpm"
ROOT = Path(tempfile.mkdtemp(prefix=f"forklaunch-{RUNTIME}-image-"))
APP = ROOT / "app"
PREFIX = f"forklaunch-{RUNTIME}-proof-" + uuid.uuid4().hex[:10]
IMAGE = PREFIX + ":local"
BUILDER_IMAGE = PREFIX + ":builder"
PURE_NODE = bool(os.environ.get("FORKLAUNCH_PURE_NODE"))
DOCKERFILE = "Dockerfile" if PURE_NODE else "Dockerfile.typescript"
NETWORK, DATABASE, SERVICE = (PREFIX + suffix for suffix in ("-net", "-db", "-api"))


def run(*args, cwd=APP, check=True, timeout=900):
    result = subprocess.run(args, cwd=cwd, text=True, capture_output=True,
                            timeout=timeout, env={**os.environ, "FORKLAUNCH_SKIP_FORMAT": "1", "CARGO_INCREMENTAL": "0", "CARGO_TARGET_DIR": str(REPO / "cli/target")})
    if check and result.returncode:
        raise RuntimeError(f"Command failed: {args[0:3]}\n{result.stdout[-5000:]}\n{result.stderr[-5000:]}")
    return (result.stdout + result.stderr).strip() if args[:2] == ("docker", "logs") else result.stdout.strip()


def cli(*args):
    return run(str(CLI), *args, "--path", str(APP), cwd=ROOT)


def docker(*args, **kwargs):
    return run("docker", *args, **kwargs)


try:
    cli("init", "application", "image-proof", "--language", "typescript", "--modules-path", "modules",
        "--database", "postgresql", "--validator", "zod", "--formatter", "prettier", "--linter", "eslint",
        "--http-framework", HTTP_FRAMEWORK, "--runtime", RUNTIME, "--test-framework", "vitest",
        "-D", "Shared image proof", "-A", "ForkLaunch", "-L", "MIT")
    # Contract builds exist for Rust consumers; a TypeScript-only app keeps tsc -b.
    cli("init", "service", "consumer", "--language", "rust", "-D", "Rust consumer")
    cli("init", "service", "api", "--language", "typescript", "-d", "postgresql", "-D", "Image API")
    cli("init", "worker", "jobs", "--language", "typescript", "-t", "database", "-d", "postgresql", "-D", "Image worker")
    cli("init", "library", "helpers", "--language", "typescript", "-D", "Shared library")
    if not PURE_NODE:
        cli("init", "library", "models", "--language", "rust", "-D", "Native library")
        assert (APP / "Dockerfile.rust").is_file()
        assert (APP / "Dockerfile.typescript").is_file()
    analysis = cli("analyze", "--pretty")
    assert "FLR-LAYOUT-006" not in analysis and "FLR-LAYOUT-007" not in analysis, analysis
    for name, output in [("core", "lib"), ("monitoring", "lib"), ("helpers", "lib"), ("client-sdk", "dist")]:
        package = json.loads((APP / f"modules/{name}/package.json").read_text())
        assert package["main"] == f"{output}/index.js", (name, package.get("main"))
        assert package["types"] == f"{output}/index.d.ts", (name, package.get("types"))
    if os.environ.get("FORKLAUNCH_COMPILE_CONTRACTS"):
        run(PM, "install", "--ignore-scripts", cwd=APP / "modules")
        run(sys.executable, str(REPO / "cli/tests/fixtures/worker_runtime_lock.py"), "-c", shlex.join([PM, "run", "build"]), cwd=APP / "modules")
        cli("sdk", "build", "--install-hooks", "--service", "api")
        cli("sdk", "build", "--install-hooks", "--service", "jobs")
        docker_source = (APP / DOCKERFILE).read_text()
        if PURE_NODE:
            assert not (APP / "modules/Dockerfile").exists()
            assert not (APP / "Dockerfile.rust").exists()
        assert "AS contract-compiler" in docker_source
        assert "cargo build --release --locked" in docker_source
        assert "ENV FORKLAUNCH_CONTRACT_COMPILER=" in docker_source
        print("PASS explicit contract build hook and pinned Linux compiler stage", flush=True)
    run(PM, "install", "--lockfile-only", "--ignore-scripts", cwd=APP / "modules")
    locked = (APP / ("modules/bun.lock" if RUNTIME == "bun" else "modules/pnpm-lock.yaml")).read_bytes()
    print("PASS fresh ordinary CLI generation and locked workspace", flush=True)
    docker("build", "-f", DOCKERFILE, "-t", IMAGE, ".", timeout=1800)
    assert (APP / ("modules/bun.lock" if RUNTIME == "bun" else "modules/pnpm-lock.yaml")).read_bytes() == locked
    if os.environ.get("FORKLAUNCH_COMPILE_CONTRACTS"):
        docker("build", "-f", DOCKERFILE, "--target", "builder", "-t", BUILDER_IMAGE, ".", timeout=1800)
        docker("run", "--rm", "--entrypoint", RUNTIME, BUILDER_IMAGE, "--input-type=module", "-e", r"""
import fs from 'node:fs';
import assert from 'node:assert/strict';
for (const name of ['api', 'jobs']) {
  const config = JSON.parse(fs.readFileSync('/app/.forklaunch/build.json')).contracts[name];
  const contract = JSON.parse(fs.readFileSync(`/app/${config.contract}`));
  assert.equal(contract.sdkCompiler, 'forklaunch.sdk.http.v1');
  assert.equal(contract.routes.length, 2);
  assert.ok(fs.readFileSync(`/app/${config.typescript}`, 'utf8').includes('export'));
  assert.ok(fs.readFileSync(`/app/${config.rust}`, 'utf8').includes('pub'));
  assert.ok(Object.values(JSON.parse(fs.readFileSync(`/app/${config.openapi}`))).some(document => document.openapi));
}
""")
        print("PASS Linux builder emits both SDK languages and contract/OpenAPI assets for service and worker", flush=True)
    if RUNTIME == "bun":
        docker("run", "--rm", "--entrypoint", "sh", IMAGE, "-c",
               'test "$(id -u)" = 1000 && '
               'test -s /app/api/production/server.mjs && test -s /app/jobs/production/worker.mjs && '
               'bun /app/api/production/migrate.mjs && bun /app/jobs/production/migrate.mjs && '
               'test ! -d /app/node_modules/typescript && test ! -e /usr/local/bin/forklaunch-contract-compiler && '
               'test ! -f /app/api/server.ts && test -s /app/core/lib/index.js')
    else:
        docker("run", "--rm", "--entrypoint", "sh", IMAGE, "-c",
               'test "$(id -u)" = 1000 && '
               'node --check /app/api/production/server.mjs && '
               'node --check /app/jobs/production/server.mjs && '
               'node --check /app/jobs/production/worker.mjs && '
               'node /app/api/production/migrate.mjs && node /app/jobs/production/migrate.mjs && '
               'test -z "$(find /app/api/node_modules/.pnpm /app/jobs/node_modules/.pnpm -maxdepth 1 -name \'typescript@*\' -print)"')
    print("PASS compiled process inventory, migration commands, nonroot runtime, no TS compiler", flush=True)
    docker("network", "create", NETWORK)
    password = uuid.uuid4().hex
    docker("run", "-d", "--name", DATABASE, "--network", NETWORK,
           "-e", "POSTGRES_PASSWORD=" + password, "-e", "POSTGRES_DB=proof", "postgres:17.11")
    for _ in range(60):
        if "accepting connections" in docker("exec", DATABASE, "pg_isready", "-U", "postgres", check=False):
            break
        time.sleep(.5)
    else:
        raise RuntimeError("Disposable PostgreSQL did not become ready")
    docker("run", "-d", "--name", SERVICE, "--network", NETWORK,
           "-p", "127.0.0.1::8000", "-p", "127.0.0.1::10000", "--env-file", str(APP / "modules/api/.env.local"),
           "-e", "DB_HOST=" + DATABASE, "-e", "DB_USER=postgres", "-e", "DB_PASSWORD=" + password,
           "-e", "DB_NAME=proof", "-e", "DB_PORT=5432", "-e", "NODE_ENV=development",
           "-e", "HOST=0.0.0.0", "-e", "PORT=8000", "-e", "HMAC_SECRET_KEY=" + uuid.uuid4().hex,
           "-e", "ENCRYPTION_KEY=" + uuid.uuid4().hex + uuid.uuid4().hex,
           IMAGE, RUNTIME, "--enable-source-maps", "/app/api/production/server.mjs")
    http_port = docker("port", SERVICE, "8000/tcp").rsplit(":", 1)[1]
    mcp_port = docker("port", SERVICE, "10000/tcp").rsplit(":", 1)[1]
    for _ in range(120):
        if docker("inspect", "--format", "{{.State.Running}}", SERVICE) != "true":
            raise RuntimeError("Generated service exited before readiness\n" + docker("logs", SERVICE, check=False))
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{http_port}/health", timeout=2) as response:
                assert response.status == 200
            break
        except (OSError, urllib.error.URLError):
            time.sleep(.5)
    else:
        raise RuntimeError("Generated HTTP readiness timed out")
    request = urllib.request.Request(f"http://127.0.0.1:{mcp_port}/mcp", data=json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "shared-image-proof", "version": "1.0"}
        }
    }).encode(), headers={"Content-Type": "application/json", "Accept": "application/json, text/event-stream"})
    with urllib.request.urlopen(request, timeout=10) as response:
        assert response.status == 200
        # Streaming responses keep the connection open; the initialization result
        # arrives in the first data frame, so do not read an unbounded stream.
        if "text/event-stream" in response.headers.get("Content-Type", ""):
            payload = response.readline() + response.readline() + response.readline()
        else:
            payload = response.read()
        assert b"serverInfo" in payload, payload
    print("PASS generated shared-image HTTP health and MCP initialization", flush=True)
finally:
    if APP.exists():
        for name in (SERVICE, DATABASE):
            docker("rm", "-f", name, check=False)
        docker("network", "rm", NETWORK, check=False)
        docker("image", "rm", IMAGE, BUILDER_IMAGE, check=False)
    if os.environ.get("FORKLAUNCH_KEEP_FIXTURE"):
        print("Fixture retained:", ROOT, flush=True)
    else:
        shutil.rmtree(ROOT)
