// Test-only lifecycle controls travel inside the production encrypted message.
// Never add fail/sleep test fields or fixture keys to generated app contracts.
import {execFile as nativeExecFile, spawn as nativeSpawn} from 'node:child_process';
import {createHash} from 'node:crypto';
import {promisify} from 'node:util';
const nativeExec = promisify(nativeExecFile);
const names = new Map();
const uuid = value => /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
export function secureJobId(label) {
  if (uuid(label)) return label;
  const hex = createHash('sha256').update(`forklaunch-runtime-fixture:${label}`).digest('hex');
  const id = `${hex.slice(0,8)}-${hex.slice(8,12)}-4${hex.slice(13,16)}-8${hex.slice(17,20)}-${hex.slice(20,32)}`;
  names.set(id, label);
  return id;
}
export function secureEnvironment(environment) {
  return {...environment, TENANT_ID: secureJobId(environment.TENANT_ID ?? 'bullmq-fixture-tenant'),
    ENCRYPTION_KEY_HEX:'7b'.repeat(32), ENCRYPTION_KEY_ID:'fixture', ENCRYPTION_LEGACY_KEY_HEX:'disabled'};
}
export function secureArguments(args) {
  if (args[0] === '--enqueue') return [args[0], secureJobId(args[1]), JSON.stringify({
    resourceId:secureJobId(`resource:${args[1]}`), revision:1, message:args[2],
  })];
  if (['--inspect','--retry'].includes(args[0])) return [args[0], secureJobId(args[1])];
  return args;
}
function decodedOutput(stdout) {
  // Keep readable scenario labels in assertions while the real backend stores
  // UUID operation IDs. Only metadata IDs are mapped, never ciphertext/payloads.
  try {
    const value=JSON.parse(stdout);
    const decode = item => {
      if(Array.isArray(item)) return item.map(decode);
      if(item && typeof item==='object') return Object.fromEntries(Object.entries(item).map(([key,val])=>[key,key==='id'?(names.get(val)??val):decode(val)]));
      return item;
    };
    return JSON.stringify(decode(value));
  } catch { return stdout; }
}
export function execFile(...args) { return nativeExecFile(...args); }
execFile[promisify.custom] = async (binary,args,options) => {
  const result = await nativeExec(binary, secureArguments(args), {...options,env:secureEnvironment(options.env)});
  return {...result,stdout:decodedOutput(result.stdout)};
};
export function spawn(binary,args,options) {
  return nativeSpawn(binary, secureArguments(args), {...options,env:secureEnvironment(options.env)});
}
