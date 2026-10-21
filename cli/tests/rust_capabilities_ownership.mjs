import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {appendFileSync,mkdirSync,mkdtempSync,readdirSync,writeFileSync} from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const cli=process.env.FORKLAUNCH_CLI??path.join(root,'cli/target/debug/forklaunch');
const output=path.join(root,'cli/tests/output/rust-capabilities-ownership');mkdirSync(output,{recursive:true});
const app=path.join(mkdtempSync(path.join(output,'run.')),'app');
const run=(...args)=>execFileSync(cli,args,{cwd:root,stdio:'inherit'});
run('init','application','ownership-app','--path',app,'--language','rust','-D','Ownership','-A','ForkLaunch','-L','MIT');
for(const service of ['alpha','beta'])run('init','service',service,'--path',app,'--language','rust','-d','postgresql','-D',service);
// Capability edits are stateless: they leave .forklaunch/ as init wrote it.
const forklaunchState=()=>readdirSync(path.join(app,'.forklaunch'),{recursive:true}).sort();
const baseline=forklaunchState();
const manifestOnly=()=>assert.deepEqual(forklaunchState(),baseline);
const compose=()=>JSON.parse(execFileSync('docker',['compose','config','--no-interpolate','--format','json'],{cwd:app,encoding:'utf8'}));
const edit=(operation,service,feature)=>run('infra',operation,service,feature,'--path',app);
const target=()=>compose().services['gateway-mock'].environment.MOCK_EVENTS_URL_EMAIL;
// Use Compose's standard normalized long forms as real editor input.
function writeMock(mock) {
 const document=compose();
 if(mock)document.services['gateway-mock']=mock;
 else delete document.services['gateway-mock'];
 writeFileSync(path.join(app,'docker-compose.yaml'),JSON.stringify(document));
}
appendFileSync(path.join(app,'docker-compose.yaml'),'\n  gateway-mock:\n    image: human/mock:fixture\n    networks:\n      ownership-app-network:\n        aliases: [custom-mock]\n    ports:\n      - name: gateway-control\n        target: 8080\n        published: "18088"\n        host_ip: 127.0.0.1\n        protocol: tcp\n        app_protocol: http\n        mode: host\n    environment:\n      USER_SETTING: preserved\n      REMOVE_ME: obsolete\n      MOCK_EVENTS_URL_EMAIL: https://human.example/callback\n    x-user-settings:\n      keep: unchanged\n      change: original\n      remove: obsolete\n');
let original=compose().services['gateway-mock'];
// The developer's own event destination is never replaced or removed.
edit('add','alpha','email');edit('add','beta','email');
assert.equal(target(),'https://human.example/callback');
edit('remove','alpha','email');edit('remove','beta','email');
assert.deepEqual(compose().services['gateway-mock'],original);
manifestOnly();
// The newest holder receives generated destinations; removing the active
// owner promotes another service with that feature.
delete original.environment.MOCK_EVENTS_URL_EMAIL;writeMock(original);
edit('add','alpha','email');edit('add','beta','email');
assert.equal(target(),'http://beta:8001');
edit('remove','alpha','email');assert.equal(target(),'http://beta:8001');
edit('remove','beta','email');assert.deepEqual(compose().services['gateway-mock'],original);
edit('add','alpha','email');edit('add','beta','email');
edit('remove','beta','email');assert.equal(target(),'http://alpha:8000');
edit('remove','alpha','email');assert.deepEqual(compose().services['gateway-mock'],original);
// Changes made while managed features are present survive later owned edits.
edit('add','alpha','email');
let customized=compose();
customized.services['gateway-mock'].image='human/edited:fixture';
customized.services['gateway-mock'].environment.USER_SETTING='changed';
customized.services['gateway-mock'].environment.NEW_SETTING='preserved';
delete customized.services['gateway-mock'].environment.REMOVE_ME;
customized.services['gateway-mock']['x-user-settings'].change='updated';
delete customized.services['gateway-mock']['x-user-settings'].remove;
customized.services['gateway-mock']['x-user-settings'].added='new';
writeMock(customized.services['gateway-mock']);
edit('add','beta','email');edit('remove','alpha','email');edit('remove','beta','email');
const edited=compose().services['gateway-mock'];
assert.equal(edited.image,'human/edited:fixture');assert.equal(edited.environment.USER_SETTING,'changed');
assert.equal(edited.environment.REMOVE_ME,undefined);
assert.deepEqual(edited['x-user-settings'],{keep:'unchanged',change:'updated',added:'new'});
assert.equal(edited.environment.NEW_SETTING,'preserved');assert.equal(edited.environment.MOCK_EVENTS_URL_EMAIL,undefined);
// An adopted gateway dependency keeps its options; an explicitly deleted one stays absent.
edit('add','alpha','email');
let dependencyDocument=compose();
dependencyDocument.services.alpha.depends_on['gateway-mock']={condition:'service_started',required:false,restart:true};
writeFileSync(path.join(app,'docker-compose.yaml'),JSON.stringify(dependencyDocument));
edit('remove','alpha','email');
assert.deepEqual(compose().services.alpha.depends_on['gateway-mock'],{condition:'service_started',required:false,restart:true});
edit('add','alpha','email');dependencyDocument=compose();delete dependencyDocument.services.alpha.depends_on['gateway-mock'];
writeFileSync(path.join(app,'docker-compose.yaml'),JSON.stringify(dependencyDocument));
edit('remove','alpha','email');assert.equal(compose().services.alpha.depends_on?.['gateway-mock'],undefined);
// Deleting a service with a local SDK needs its compiled contract.
run('openapi','export','--path',app);
// Deletion follows the same ownership lifetime, even if the first owner leaves first.
edit('add','alpha','email');edit('add','beta','email');
run('delete','service','alpha','--path',app,'--continue');assert.equal(target(),'http://beta:8001');
run('delete','service','beta','--path',app,'--continue');assert.deepEqual(compose().services['gateway-mock'],edited);
manifestOnly();
// A customized generated mock is now developer-owned; retain it without stale callbacks.
let document=compose();delete document.services['gateway-mock'];writeMock(undefined);
run('init','service','gamma','--path',app,'--language','rust','-d','postgresql','-D','Gamma');
edit('add','gamma','email');document=compose();
document.services['gateway-mock'].image='human/adopted:fixture';
document.services['gateway-mock'].environment.ADOPTED_SETTING='preserved';
writeMock(document.services['gateway-mock']);
edit('add','gamma','sms');edit('remove','gamma','email');edit('remove','gamma','sms');
const adopted=compose().services['gateway-mock'];assert.equal(adopted.image,'human/adopted:fixture');
assert.equal(adopted.environment.ADOPTED_SETTING,'preserved');
assert.equal(adopted.environment.MOCK_EVENTS_URL_EMAIL,undefined);assert.equal(adopted.environment.MOCK_EVENTS_URL_SMS,undefined);
// Explicitly removing the mock changes the baseline even if another add recreates it.
edit('add','gamma','email');document=compose();delete document.services['gateway-mock'];
writeMock(document.services['gateway-mock']);
edit('add','gamma','sms');edit('remove','gamma','email');edit('remove','gamma','sms');
assert.equal(compose().services['gateway-mock'],undefined);
console.log('PASS native multi-service gateway ownership: removal orders, promotion, deletion, developer destinations, active edits, adoption and explicit removal');
