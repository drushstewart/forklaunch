import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, writeFile, copyFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const temporary = await mkdtemp(path.join(tmpdir(), 'fl-sdk-typed-requests-'));
const env = { ...process.env, RUSTUP_TOOLCHAIN: 'stable', CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR ?? path.join(root, 'framework-rs/target'), CARGO_BUILD_JOBS: '1', CARGO_INCREMENTAL: '0' };
function cargo(args) { execFileSync('cargo', args, { cwd: root, env, stdio: 'inherit', timeout: 600000 }); }
try {
  await mkdir(path.join(temporary, 'src'));
  await mkdir(path.join(temporary, 'tests'));
  cargo(['run', '--quiet', '--manifest-path', path.join(root, 'framework-rs/Cargo.toml'), '-p', 'forklaunch-core','--features','build,asyncapi-import', '--example', 'rust_sdk_typed_requests', '--', path.join(temporary, 'src/lib.rs')]);
  await writeFile(path.join(temporary, 'Cargo.toml'), `[package]\nname="typed-request-proof"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\nreqwest={version="0.13.2",features=["json","query"]}\nserde={version="1",features=["derive"]}\nserde_json="1"\nforklaunch-universal-sdk={path=${JSON.stringify(path.join(root,"framework-rs/crates/universal-sdk"))}}\nforklaunch-core={path=${JSON.stringify(path.join(root,"framework-rs/crates/core"))},features=["schema-validation"]}\ntokio={version="1",features=["macros","rt-multi-thread"]}\n`);
  await copyFile(path.join(root, 'cli/tests/fixtures/rust_sdk_typed_requests.rs'), path.join(temporary, 'tests/typed.rs'));
  await copyFile(path.join(root, 'framework-rs/Cargo.lock'), path.join(temporary, 'Cargo.lock'));
  cargo(['test', '--offline', '--manifest-path', path.join(temporary, 'Cargo.toml')]);
  // A schema-object request cannot accidentally use the historical encoded-string API.
  await writeFile(path.join(temporary, 'src/main.rs'), `use typed_request_proof::*;\nfn main() { let _ = Client::new("http://localhost").create_request(CreateRequest { body: Some(String::from("{}")), ..Default::default() }); }\n`);
  const rejected = spawnSync('cargo', ['check', '--offline', '--manifest-path', path.join(temporary, 'Cargo.toml'), '--bin', 'typed-request-proof'], { cwd: root, env, encoding: 'utf8', timeout: 600000 });
  assert.equal(rejected.status, 101, rejected.stderr);
  assert.match(rejected.stderr, /expected `CreateRequestBody`, found `String`/);
  const source = await readFile(path.join(temporary, 'src/lib.rs'), 'utf8');
  assert.match(source, /pub type CreateResponse = ::std::string::String;/);
  console.log('PASS typed JSON request structs: compiled serialization and HTTP/auth-hook tests; raw escape hatch; object-body String compile rejection; unchanged response types');
} finally {
  await rm(temporary, { recursive: true, force: true });
}
