import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {actualValidator} from './rust_actual_validator_fixture.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const {load} = actualValidator(root);
const {prettyPrintParseErrors} = load(path.join(root, 'framework/validator/src/shared/utils/prettyPrintParseErrors.ts'));
const cases = [];
const prefixes = [undefined, null, '', 'Request', 'Session', 'Header', 'Response', ' 用户\n', '😀'];
const messages = ['', 'Required', 'Invalid input: expected string, received undefined', 'a\nb\r\nc', 'quotes: " \\ < >', 'é/用户/😀', '\0'];
const paths = [[], [''], ['root'], ['user', 'email'], ['items', '0', 'name'], [' > ', '\n'], ['é', '用户', '😀']];
for (const prefix of prefixes) {
  cases.push({prefix}, {prefix, errors: null}, {prefix, errors: []});
  for (const message of messages) for (const path of paths) {
    cases.push({prefix, errors: [{path, message}]});
    cases.push({prefix, errors: [{path, message}, {path: [], message: 'second'}, {path: ['x', '1'], message: ''}]});
  }
  cases.push({prefix, errors: Array.from({length: 105}, (_, i) => ({path: ['rows', String(i)], message: String(i)}))});
}
const native = spawnSync('cargo', ['run', '--quiet', '--manifest-path', path.join(root, 'framework-rs/Cargo.toml'), '-p', 'forklaunch-validation', '--example', 'parse_errors_parity'], {
  input: cases.map(value => JSON.stringify(value)).join('\n') + '\n', encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, timeout: 300000,
  env: {...process.env, CARGO_INCREMENTAL: '0'},
});
assert.equal(native.status, 0, native.stderr);
const results = native.stdout.trim().split('\n').map(value => JSON.parse(value));
assert.equal(results.length, cases.length);
for (const [index, value] of cases.entries())
  assert.equal(results[index], prettyPrintParseErrors(value.errors, value.prefix), JSON.stringify(value));
console.log(`PASS ${cases.length} actual TypeScript parse-error formatting comparisons`);
