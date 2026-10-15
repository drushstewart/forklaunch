// Release Cargo's build lock before the live fixture compiles its Rust SDK client.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const env={...process.env,CARGO_INCREMENTAL:'0',CARGO_TARGET_DIR:path.resolve(process.env.CARGO_TARGET_DIR??path.join(root,'blueprint-rs/target'))};
const built=spawnSync('cargo',['test','--manifest-path',path.join(root,'blueprint-rs/Cargo.toml'),'-p','forklaunch-billing-axum','--features','bound-checkout','--test','plan_read','--no-run','--message-format=json'],{cwd:root,env,encoding:'utf8',timeout:600000,maxBuffer:32*1024*1024});
assert.equal(built.status,0,built.stderr);
const executables=built.stdout.split('\n').filter(Boolean).map(JSON.parse).filter(message=>message.reason==='compiler-artifact'&&message.target.name==='plan_read'&&message.executable).map(message=>message.executable);
assert.equal(executables.length,1,'one bound Plan read test executable');
const tested=spawnSync(executables[0],['--ignored','--nocapture'],{cwd:root,env,stdio:'inherit',timeout:600000});
assert.equal(tested.status,0,tested.error?.message);
