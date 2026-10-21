import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdirSync,mkdtempSync,readFileSync,writeFileSync,appendFileSync,existsSync,renameSync,statSync,rmSync,readdirSync} from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const cli=process.env.FORKLAUNCH_CLI??path.join(root,'cli/target/debug/forklaunch');
const output=path.join(root,'cli/tests/output/rust-infrastructure-runtime');mkdirSync(output,{recursive:true});
const app=process.argv[2]??path.join(mkdtempSync(path.join(output,'run.')),'app');
const stateHas=key=>{const file=path.join(app,'.forklaunch/local-state.json');return existsSync(file)&&Object.hasOwn(JSON.parse(readFileSync(file,'utf8')),key);};
const run=(...args)=>execFileSync(cli,args,{cwd:root,stdio:'inherit'});
const edit=(operation,service,feature,...extra)=>run('infra',operation,service,feature,'--path',app,...extra);
const read=relative=>readFileSync(path.join(app,relative),'utf8');
const write=(relative,text)=>writeFileSync(path.join(app,relative),text);
const compose=()=>JSON.parse(execFileSync('docker',['compose','config','--no-interpolate','--format','json'],{cwd:app,encoding:'utf8'}));
run('init','application','infra-app','--path',app,'--database','postgresql','--validator','zod','--formatter','prettier','--linter','eslint','--http-framework','express','--runtime','node','--test-framework','vitest','-D','Infrastructure','-A','ForkLaunch','-L','MIT');
for(const service of ['alpha','beta'])run('init','service',service,'--path',app,'--language','rust','-D',service);
const originalBackends = {redis: compose().services.redis, minio: compose().services.minio};
// Init and change flags use the same native wiring rather than silently ignoring them.
run('init','service','flagged','--path',app,'--language','rust','-D','Flagged','--infrastructure','redis','s3');
for(const id of ['redis','s3'])assert.ok(existsSync(path.join(app,`src/flagged/infrastructure/${id}.rs`)));
appendFileSync(path.join(app,'src/flagged/server.rs'),'\n// Human flag customization\n');
run('change','service','--path',path.join(app,'src/flagged'),'--infrastructure','--confirm');
assert.ok(read('src/flagged/server.rs').includes('Human flag customization'));
for(const id of ['redis','s3'])assert.equal(existsSync(path.join(app,`src/flagged/infrastructure/${id}.rs`)),false);
run('change','service','--path',path.join(app,'src/flagged'),'--infrastructure','redis','--confirm');
assert.ok(read('src/flagged/server.rs').includes('Human flag customization'));
run('delete','service','flagged','--path',app,'--continue');
const pristine=Object.fromEntries(['Cargo.toml','lib.rs','server.rs'].map(file=>[file,read(`src/alpha/${file}`)]));
for(const id of ['redis','s3']){
 edit('add','alpha',id,'--dryrun');
 assert.equal(existsSync(path.join(app,`src/alpha/infrastructure/${id}.rs`)),false);
 assert.equal(stateHas(`alpha/infra-${id}`),false);
}
// Stacked local resources and managed capabilities can leave in any order.
edit('add','alpha','redis');edit('add','alpha','email');edit('add','alpha','s3');
edit('remove','alpha','redis');edit('remove','alpha','email');edit('remove','alpha','s3');
for(const [file,text] of Object.entries(pristine))assert.equal(read(`src/alpha/${file}`),text,file);
assert.equal(existsSync(path.join(app,'src/alpha/.env.local')),false);
assert.deepEqual(compose().services.redis,originalBackends.redis);assert.deepEqual(compose().services.minio,originalBackends.minio);
// Shared backend remains until the final owner leaves, including deletion.
for(const id of ['redis','s3']){edit('add','alpha',id);edit('add','beta',id);edit('remove','alpha',id);}
assert.ok(compose().services.redis);assert.ok(compose().services.minio);
run('delete','service','beta','--path',app,'--continue');
assert.deepEqual(compose().services.redis,originalBackends.redis);assert.deepEqual(compose().services.minio,originalBackends.minio);
for(const id of ['redis','s3'])assert.equal(stateHas(`_app/infra-${id}`),false);
// Existing backends and edited deployment values remain application-owned.
let document=compose();
document.services.redis={image:'human/redis:fixture',environment:{USER_SETTING:'original'},networks:{'infra-app-network':{aliases:['custom-cache']}}};
document.services.alpha.environment.REDIS_URL='redis://human-cache:6379/4';
write('docker-compose.yaml',JSON.stringify(document));
write('src/alpha/.env.local','# Custom local configuration\nexport REDIS_URL = "redis://127.0.0.1:32770/4"\nUNRELATED=preserved\n');
const originalEnv=read('src/alpha/.env.local');
edit('add','alpha','redis');
assert.equal(read('src/alpha/.env.local'),originalEnv);
assert.equal(compose().services.alpha.environment.REDIS_URL,'redis://human-cache:6379/4');
document=compose();document.services.redis.image='human/redis:edited';document.services.redis.environment.USER_SETTING='updated';
write('docker-compose.yaml',JSON.stringify(document));
edit('remove','alpha','redis');
assert.equal(read('src/alpha/.env.local'),originalEnv);
assert.equal(compose().services.redis.image,'human/redis:edited');
assert.deepEqual(compose().services.redis.networks['infra-app-network'].aliases,['custom-cache']);
assert.equal(compose().services.redis.environment.USER_SETTING,'updated');
// Move to a nested registered crate path, then preserve factories through sync/rename.
mkdirSync(path.join(app,'services/team'),{recursive:true});renameSync(path.join(app,'src/alpha'),path.join(app,'services/team/alpha'));
write('Cargo.toml',read('Cargo.toml').replaceAll('src/alpha','services/team/alpha'));
write('.forklaunch/manifest.toml',read('.forklaunch/manifest.toml').replaceAll('src/alpha','services/team/alpha'));
// Relative framework paths follow the human relocation.
for (const file of ['Cargo.toml', '.forklaunch/contracts/Cargo.toml', 'sdk/rust/Cargo.toml']) {
 const manifest = `services/team/alpha/${file}`;
 // Only dependencies outside the relocated project gain a parent component.
 write(manifest, read(manifest).replace(/path\s*=\s*"([^"]+)"/g, (declaration, dependency) => {
  const previous = path.resolve(app, 'src/alpha', path.dirname(file), dependency);
  if (previous.startsWith(path.resolve(app, 'src/alpha') + path.sep)) return declaration;
  return `path = "${path.relative(path.join(app, 'services/team/alpha', path.dirname(file)), previous)}"`;
 }));
}
for(const id of ['redis','s3'])edit('add','alpha',id);
appendFileSync(path.join(app,'services/team/alpha/infrastructure/redis.rs'),'\npub const HUMAN_FACTORY: bool = true;\n');
appendFileSync(path.join(app,'services/team/alpha/server.rs'),'\n// Human main customization\n');
appendFileSync(path.join(app,'services/team/alpha/config.rs'),'\npub const HUMAN_CONFIG: bool = true;\n');
run('sync','service','alpha','--path',app);
run('change','service','--path',path.join(app,'services/team/alpha'),'-N','storage','--confirm');
assert.ok(read('services/team/storage/infrastructure/redis.rs').includes('HUMAN_FACTORY'));
assert.ok(read('services/team/storage/server.rs').includes('Human main customization'));
assert.ok(read('services/team/storage/config.rs').includes('HUMAN_CONFIG'));
for(const id of ['redis','s3']){
 const state=path.join(app,'.forklaunch/local-state.json');
 assert.ok(stateHas(`storage/infra-${id}`));assert.equal(stateHas(`alpha/infra-${id}`),false);
 if(process.platform!=='win32')assert.equal(statSync(state).mode&0o777,0o600);
}
edit('remove','storage','redis');
assert.ok(read('services/team/storage/infrastructure/redis.rs').includes('HUMAN_FACTORY'));
assert.ok(read('services/team/storage/Cargo.toml').includes('forklaunch-infrastructure-redis'));
// Leave both adapters enabled for the runtime fixture.
edit('add','storage','redis');
// Clone recovery removes known generated hooks but keeps unknown settings/dependencies.
rmSync(path.join(app,'.forklaunch/local-state.json'),{force:true});
const clonedEnv=read('services/team/storage/.env.local');
edit('remove','storage','s3');
assert.equal(existsSync(path.join(app,'services/team/storage/infrastructure/s3.rs')),false);
assert.ok(read('services/team/storage/Cargo.toml').includes('forklaunch-infrastructure-s3'));
assert.equal(read('services/team/storage/.env.local'),clonedEnv);
edit('add','storage','s3');
console.log(`PASS native Redis/S3 CLI ownership, dry run, stacking, shared backends, custom settings, nested crate paths, sync and rename: ${app}`);
