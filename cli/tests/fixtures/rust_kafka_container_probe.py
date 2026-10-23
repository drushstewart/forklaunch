"""Exercise the generated encrypted Kafka boundary in the actual Linux image."""

import hashlib
import json
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path


run_root, image, network, broker, worker = sys.argv[1:]
run_root = Path(run_root)
tenant = "11111111-1111-4111-8111-111111111111"
operation = "22222222-2222-4222-8222-222222222222"
resource = "33333333-3333-4333-8333-333333333333"
private_marker = "container-private-work-payload"
key = "7b" * 32
control_token = "container-parity-control-token-32-chars"
environment = {
    "KAFKA_BROKERS": "broker:9092",
    "WORKER_NAMESPACE": "container-parity",
    "TENANT_ID": tenant,
    "ENCRYPTION_KEY_HEX": key,
    "ENCRYPTION_KEY_ID": "container",
    "ENCRYPTION_LEGACY_KEY_HEX": "disabled",
    # This isolated test broker is local development. Payload encryption remains
    # mandatory; the default verified-TLS admission policy is tested below.
    "WORKER_ALLOW_PLAINTEXT_DEVELOPMENT": "true",
    "KAFKA_TRANSACTIONAL_ID": "container-cli",
    "KAFKA_PUBLISHER_ID": "container-enqueue",
    "KAFKA_OPERATION_TIMEOUT_MS": "15000",
    # Required at startup since 7248a878d.
    "RUST_LOG": "info",
    "WORKER_QUEUE": "jobs",
    "KAFKA_GROUP_ID": "workers",
    "KAFKA_PARTITIONS": "3",
    "KAFKA_REPLICATION_FACTOR": "1",
    "KAFKA_SESSION_TIMEOUT_MS": "10000",
    "WORKER_MAX_PAYLOAD_BYTES": "1048576",
    "WORKER_MAX_ATTEMPTS": "3",
    "WORKER_BACKOFF_SECONDS": "30",
    "WORKER_HANDLER_TIMEOUT_MS": "300000",
    "WORKER_DRAIN_MS": "30000",
}
payload = {
    "resourceId": resource,
    "revision": 1,
    "message": json.dumps({"privateMarker": private_marker}),
}


def command(*args, timeout=45, check=True):
    result = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    if check and result.returncode:
        raise AssertionError(f"Command failed ({result.returncode}): {result.stdout}{result.stderr}")
    return result


def env_args(overrides=None):
    values = environment | (overrides or {})
    return [item for name, value in values.items() for item in ("--env", f"{name}={value}")]


def worker_command(*args, overrides=None, rejected=False):
    name = f"{network}-command"
    try:
        result = command(
            "docker", "run", "--rm", "--name", name, "--network", network,
            *env_args(overrides), image, "/app/bin/jobs", *args, check=False,
        )
    finally:
        command("docker", "rm", "-f", name, check=False)
    output = result.stdout + result.stderr
    assert private_marker not in output and key not in output, "Sensitive data escaped diagnostics"
    if rejected:
        assert result.returncode != 0, "Invalid worker configuration/input was admitted"
    else:
        assert result.returncode == 0, output
    return output if rejected else result.stdout


def reject(label, *args, overrides=None, error="worker_envelope_rejected"):
    output = worker_command(*args, overrides=overrides, rejected=True)
    assert error in output, f"Unexpected failure for {label}: {output}"
    print(f"PASS rejected {label}", flush=True)


def logs():
    result = command("docker", "logs", worker)
    output = result.stdout + result.stderr
    assert private_marker not in output and key not in output, "Sensitive data escaped worker logs"
    return output


def wait_for(predicate, label, seconds=60):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(1)
    raise AssertionError(f"Timed out waiting for {label}")


worker_command("--migrate")


def inspection_ready():
    # A fresh Redpanda creates its internal offsets topic on the first group
    # lookup and can return a coordinator-loading error during that request.
    # Treat only this bounded initial lookup as broker readiness. Every later
    # inspection, including the rejection assertions, must succeed directly.
    try:
        pending = worker_command("--peek", "10")
    except AssertionError as error:
        if "kafka_storage_error" not in str(error):
            raise
        print("Waiting for fresh broker consumer-offset coordinator", flush=True)
        return False
    assert json.loads(pending) == [], "Fresh broker unexpectedly contains work"
    return True


wait_for(inspection_ready, "broker consumer-offset coordinator")
# An empty required setting is reported as missing since 7248a878d.
reject("missing encryption key at startup", overrides={"ENCRYPTION_KEY_HEX": ""}, error="config_missing")
reject("nil tenant at startup", overrides={"TENANT_ID": "00000000-0000-0000-0000-000000000000"})
reject("remote plaintext broker under default TLS policy", overrides={"WORKER_ALLOW_PLAINTEXT_DEVELOPMENT": "false"}, error="Configuration")
reject("invalid operation UUID", "--enqueue", "container-job", json.dumps(payload))
reject("invalid revision", "--enqueue", operation, json.dumps(payload | {"revision": 0}))
reject("nil resource UUID", "--enqueue", operation, json.dumps(payload | {"resourceId": "00000000-0000-0000-0000-000000000000"}))
reject("undeclared input field", "--enqueue", operation, json.dumps(payload | {"unexpected": True}))
assert json.loads(worker_command("--peek", "10")) == [], "Rejected work reached the broker"
worker_command("--enqueue", operation, json.dumps(payload))

# Observe the real broker record before any worker can consume it. The topic's
# length-prefixed hash is the public namespace/tenant/queue scoping algorithm.
digest = hashlib.sha256()
for part in (environment["WORKER_NAMESPACE"], tenant, "jobs"):
    encoded = part.encode()
    digest.update(len(encoded).to_bytes(8, "big"))
    digest.update(encoded)
topic = f"flw.{digest.hexdigest()}"
stored = command(
    "docker", "exec", broker, "rpk", "topic", "consume", topic,
    "--num", "1", "--offset", "start", "--read-committed", "--format", "%v\\n",
).stdout
assert private_marker not in stored and key not in stored, "Broker stored plaintext"
record = json.loads(stored)
assert record["version"] == 1 and record["tenant_id"] == tenant
job = record["job"]
assert job["id"] == operation and job["queue"] == "jobs" and job["attempts"] == 0
sealed = json.loads(job["payload_json"])
assert set(sealed) == {"channel", "context", "ciphertext"}
assert sealed["channel"] == "work-v1"
assert sealed["context"] == {"tenantId": tenant, "resourceId": resource, "operationId": operation, "revision": 1}
assert sealed["ciphertext"].startswith("v3.container.")
(run_root / "kafka-stored-record.json").write_text(stored)
assert json.loads(worker_command("--peek", "10")) == [{"id": operation, "queue": "jobs", "attempts": 0}]
print("PASS broker ciphertext, authenticated context, and payload-free inspection", flush=True)

command(
    "docker", "run", "--detach", "--name", worker, "--network", network,
    "--publish", "127.0.0.1::8000", *env_args({
        "KAFKA_TRANSACTIONAL_ID": "container-worker",
        "WORKER_HTTP_ADDR": "0.0.0.0:8000",
        "WORKER_CONTROL_TOKEN": control_token,
    }),
    "--health-cmd", "curl --fail --silent http://127.0.0.1:8000/health >/dev/null",
    "--health-interval", "1s", "--health-timeout", "3s", "--health-retries", "30",
    image, "/app/bin/jobs",
)


def healthy():
    state = json.loads(command("docker", "inspect", "--format", "{{json .State}}", worker).stdout)
    assert state["Running"], logs()
    return state["Health"]["Status"] == "healthy"


wait_for(healthy, "worker health")
wait_for(lambda: f"test-handler-started {operation}" in logs(), "authenticated application handler")
wait_for(lambda: json.loads(worker_command("--peek", "10")) == [], "committed delivery")
address = command("docker", "port", worker, "8000/tcp").stdout.strip()
with urllib.request.urlopen(f"http://{address}/ready", timeout=10) as response:
    assert response.status == 200
try:
    urllib.request.urlopen(urllib.request.Request(f"http://{address}/control/pause", method="POST"), timeout=10)
except urllib.error.HTTPError as error:
    assert error.code == 401
else:
    raise AssertionError("Unauthenticated control request accepted")
with urllib.request.urlopen(urllib.request.Request(
    f"http://{address}/control/pause", method="POST",
    headers={"Authorization": f"Bearer {control_token}"},
), timeout=10) as response:
    assert 200 <= response.status < 300
command("docker", "stop", "--time", "15", worker)
assert command("docker", "inspect", "--format", "{{.State.ExitCode}}", worker).stdout.strip() == "0"
assert command("docker", "inspect", "--format", "{{.Config.User}}", worker).stdout.strip() == "65532:65532"
(run_root / "kafka-worker.log").write_text(logs())
assert "completed=1" in logs(), "Authenticated job did not complete successfully"
print("PASS encrypted processing, committed delivery, readiness, authenticated controls, nonroot UID and graceful SIGTERM", flush=True)
