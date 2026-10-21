import assert from "node:assert/strict";
import { createDecipheriv } from "node:crypto";
// Pass the output of `cargo run -q -p fieldnotes-security --example envelope_vector`.
const envelope = process.argv[2];
assert.ok(envelope, "Rust-generated envelope argument required");
const parts = envelope.split(".");
const version = parts.shift();
assert.ok(version === "v1" || version === "v2");
const keyId = version === "v2" ? parts.shift() : undefined;
if (keyId !== undefined) assert.equal(keyId, "primary");
const [nonce, ciphertext, ...extra] = parts;
assert.equal(extra.length, 0);
const bytes = Buffer.from(ciphertext, "base64url");
const cipher = createDecipheriv(
  "aes-256-gcm",
  Buffer.alloc(32, 7),
  Buffer.from(nonce, "base64url"),
);
cipher.setAAD(
  Buffer.from(
    `fieldnotes:${version}:${keyId ? `${keyId}:` : ""}11111111-1111-4111-8111-111111111111:22222222-2222-4222-8222-222222222222:note-body`,
  ),
);
cipher.setAuthTag(bytes.subarray(-16));
assert.equal(
  Buffer.concat([
    cipher.update(bytes.subarray(0, -16)),
    cipher.final(),
  ]).toString("utf8"),
  "hello 📝",
);
console.log("Rust → Node authenticated envelope passed");
