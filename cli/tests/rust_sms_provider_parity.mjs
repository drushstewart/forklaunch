import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFile, realpath } from 'node:fs/promises';
import http from 'node:http';
import { spawn } from 'node:child_process';
import vm from 'node:vm';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHmac } from 'node:crypto';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const require = createRequire(import.meta.url);
const tsc = process.env.TSC_BIN ?? path.join(root, 'framework/node_modules/.bin/tsc');
const ts = require(require.resolve('typescript', { paths: [path.dirname(await realpath(tsc))] }));
const smsEnum = await load('blueprint/implementations/messaging/base/domain/enum/smsStatus.enum.ts');
const twilioEnum = await load('blueprint/implementations/messaging/twilio/domain/enum/twilioMessageStatus.enum.ts');
class EntityManager {}
let fixtureBase;
let sentRequests = [];
async function load(file) {
  const source = await readFile(path.join(root,file),'utf8');
  const compiled = ts.transpileModule(source,{ compilerOptions:{ module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022 } }).outputText;
  const exports = {};
  const localRequire = name => {
    if (name === '@mikro-orm/core') return { EntityManager };
    if (name === '@forklaunch/core/http') return {};
    if (name === '@forklaunch/implementation-messaging-base/enum' || name === '../domain/enum') return smsEnum;
    if (name === '../domain/enum/twilioMessageStatus.enum') return twilioEnum;
    throw new Error(`Unexpected runtime dependency ${name}`);
  };
  const redirectedFetch = (url,options) => fetch(String(url).replace('https://api.twilio.com/2010-04-01',fixtureBase),options);
  vm.runInNewContext(compiled,{ exports,require:localRequire,Buffer,URLSearchParams,fetch:redirectedFetch },{filename:file});
  return exports;
}
const { BaseSmsService } = await load('blueprint/implementations/messaging/base/services/sms.service.ts');
const { TwilioSmsService, mapTwilioMessageStatus } = await load('blueprint/implementations/messaging/twilio/services/sms.service.ts');
function service(Type) {
  const records = [];
  const em = new EntityManager();
  em.transactional = callback => callback(em);
  em.persist = record => { records.push(record); };
  em.flush = () => {};
  em.findOneOrFail = (_,query) => { const found = records.find(r=>Object.entries(query).every(([key,value])=>r[key] === value));if(!found)throw new Error('not found');return found; };
  const mappers = {
    SendSmsMapper:{toEntity:async dto=>({...structuredClone(dto),id:`id-${records.length}`,status:'queued'})},
    SmsRecordMapper:{entity:{},toDto:record=>structuredClone(record)}
  };
  const otel = { info(){},error(){} };
  return Type === BaseSmsService ? new Type(em,otel,{},mappers) : new Type('ACfixture','fixture-secret-only','+15550000000',em,otel,{},mappers);
}
const statuses = [...Object.values(twilioEnum.TwilioMessageStatusEnum),'unknown','','SENT',' delivered'];
const requests = [
  {to:'+15550000001',body:'queued',metadata:{nested:[1,null,true],unicode:'💬'}},
  {to:'',body:'sent'},
  {to:'+15550000002',body:'read'},
  {to:'+15550000003',body:'unknown'},
  {to:'+15550000004',body:'provider-error'},
  {to:'+15550000005',body:'provider-malformed'}
];
const signatures = [
  {token:'12345',url:'https://example.com/myapp.php?foo=1&bar=2',params:[['CallSid','CA1234567890ABCDE'],['Caller','+14158675310'],['Digits','1234'],['From','+14158675310'],['To','+18005551212']]},
  {token:'token',url:'https://example.com/hook?x=a%2Bb',params:[['x','b'],['x','a'],['x','b'],['\ue000','z'],['💬','v'],['Extra','']]}
];
// Twilio SDK sorts unique values for duplicate parameters with JS UTF-16 order.
function expectedSignature({token,url,params}) {
  const fields = new Map();for(const [key,value] of params) fields.set(key,[...(fields.get(key)??[]),value]);
  const data = [...fields.keys()].sort().reduce((s,k)=>s+[...new Set(fields.get(k))].sort().map(v=>k+v).join(''),url);
  return createHmac('sha1',token).update(data,'utf8').digest('base64');
}
function normalize(record) {
  const result = structuredClone(record);for(const key of ['id','createdAt','updatedAt'])delete result[key];
  for(const key of ['error','providerMessageId','metadata'])if(result[key]==null)delete result[key];return result;
}
const server = http.createServer(async (req,res)=>{
  try {
    assert.equal(req.method,'POST');assert.equal(req.url,'/Accounts/ACfixture/Messages.json');
    assert.equal(req.headers.authorization,`Basic ${Buffer.from('ACfixture:fixture-secret-only').toString('base64')}`);
    assert.match(req.headers['content-type'],/^application\/x-www-form-urlencoded/);
    let body='';for await(const chunk of req)body+=chunk;
    const form = Object.fromEntries(new URLSearchParams(body));assert.equal(form.From,'+15550000000');
    assert.deepEqual(Object.keys(form).sort(),['Body','From','To']);sentRequests.push(form);
    if(form.Body === 'provider-error') {res.writeHead(400);res.end('{"message":"fixture failure"}');}
    else if(form.Body === 'provider-malformed') {res.end('invalid-json');}
    else {res.setHeader('Content-Type','application/json');res.end(JSON.stringify({sid:`SM-${form.Body}`,status:form.Body,error_message:form.Body==='unknown'?'initial error':null}));}
  } catch(error) {res.writeHead(500);res.end(String(error));}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
fixtureBase = `http://127.0.0.1:${server.address().port}`;
try {
  const base = service(BaseSmsService),twilio = service(TwilioSmsService);
  const expected = {base:[],sends:[],callbacks:[],errors:[],mapped:statuses.map(mapTwilioMessageStatus),signatures:signatures.map(expectedSignature)};
  for(const request of requests) {
    expected.base.push(normalize(await base.sendSms(request)));
    let record;try {record=await twilio.sendSms(request);}catch {expected.errors.push('twilio_provider_error');continue;}
    expected.sends.push(normalize(record));assert.deepEqual(await twilio.getSmsRecord({id:record.id}),record);
    if(record.providerMessageId)for(const [index,status] of statuses.entries()) {
      const updated = await twilio.processStatusCallback({MessageSid:record.providerMessageId,MessageStatus:status,...(index===0?{ErrorMessage:'delivery error'}:{})});
      expected.callbacks.push(normalize(updated));
    }
  }
  await assert.rejects(()=>twilio.getSmsRecord({id:'missing'}));
  await assert.rejects(()=>twilio.processStatusCallback({MessageSid:'missing',MessageStatus:'sent'}));
  const tsRequests = structuredClone(sentRequests);sentRequests=[];
  const child = spawn('cargo',['run','--quiet','--manifest-path','blueprint-rs/Cargo.toml','-p','forklaunch-messaging-twilio','--example','sms_provider_parity'],{cwd:root,env:{...process.env,TWILIO_FIXTURE_URL:fixtureBase},stdio:['pipe','pipe','inherit']});
  let stdout='';child.stdout.on('data',chunk=>stdout+=chunk);
  child.stdin.end(JSON.stringify({statuses,requests,signatures}));
  const code=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('exit',resolve);});assert.equal(code,0);
  assert.deepEqual(JSON.parse(stdout),expected);
  assert.deepEqual(sentRequests,tsRequests);
  console.log(`PASS actual TypeScript/Rust base and Twilio SMS DTOs, REST forms/auth, ${statuses.length} status mappings, callbacks, error clearing, missing records, provider failures and signature vectors`);
} finally {await new Promise(resolve=>server.close(resolve));}
