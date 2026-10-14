// Actual configured Better Auth error boundaries with transaction-aware owned
// adapter failures. Native fault_boundaries tests exercise matching HTTP results
// and native transaction rollback; this fixture does not emulate SQL COMMIT.
import assert from 'node:assert/strict';
import path from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {betterAuthReference} from './fixtures/rust_better_auth_reference.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const dependencies=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(dependencies,'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES');
const {betterAuth}=await import(pathToFileURL(path.join(dependencies,'node_modules/better-auth/dist/index.mjs')));
const reference=await betterAuthReference(root);
let failure='',cookie=''; const calls=[],observations=[];
const wrap=adapter=>{
 const wrapped={...adapter,transaction:async callback=>{
  if(failure==='transaction:begin')throw new Error('Owned transaction begin failure');
  return adapter.transaction(async tx=>{
   const result=await callback(wrap(tx));
   if(failure==='transaction:commit')throw new Error('Owned transaction completion failure');
   return result;
  });
 }};
 for(const method of ['create','findOne','delete','deleteMany'])wrapped[method]=async args=>{
  calls.push([method,args.model]);
  if(failure===`${method}:${args.model}`)throw new Error(`Owned ${method} failure`);
  return adapter[method](args);
 };
 return wrapped;
};
const auth=betterAuth({...reference.config,secret:'better-auth-protocol-fixture-secret-32-characters',database:options=>wrap(reference.config.database(options)),onAPIError:{onError:error=>observations.push(error)}});
async function request(route,body){const response=await auth.handler(new Request(`http://localhost:3000/api/auth${route}`,{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json',cookie},body:JSON.stringify(body)}));return {status:response.status,body:await response.text().then(text=>{try{return JSON.parse(text)}catch{return text}}),cookies:response.headers.getSetCookie()};}
const credentials={email:'fault-oracle@example.test',password:'owned-fixture-password',name:'Fault Oracle',firstName:'Fault',lastName:'Oracle'};
failure='create:user';const failedSignup=await request('/sign-up/email',credentials);console.log(JSON.stringify({case:'signup-create-user-failure',...failedSignup,observations:observations.length}));assert.equal(failedSignup.status,422);assert.deepEqual(failedSignup.body,{message:"Failed to create user",code:"FAILED_TO_CREATE_USER"});assert.deepEqual(failedSignup.cookies,[]);assert.equal(observations.length,0);
for(const [index,mode] of ['create:account','create:session','transaction:begin','transaction:commit'].entries()) {
 failure=mode;const before=observations.length;
 const result=await request('/sign-up/email',{...credentials,email:`fault-${index}@example.test`});
 console.log(JSON.stringify({case:`signup-${mode}`,...result,observations:observations.length-before}));
 assert.equal(result.status,500);assert.equal(result.body,"");assert.deepEqual(result.cookies,[]);assert.equal(observations.length-before,1);
}
observations.length=0;
failure='';const signup=await request('/sign-up/email',credentials);assert.equal(signup.status,200);cookie=signup.cookies.find(value=>value.startsWith('better-auth.session_token=')).split(';')[0];
failure='findOne:session';const failedRead=await request('/sign-out',{});console.log(JSON.stringify({case:'signout-read-failure',...failedRead,observations:observations.length}));assert.equal(failedRead.status,200);assert.equal(observations.length,0);
failure='';const signin=await request('/sign-in/email',credentials);assert.equal(signin.status,200);cookie=signin.cookies.find(value=>value.startsWith('better-auth.session_token=')).split(';')[0];
failure='delete:session';const failedDelete=await request('/sign-out',{});console.log(JSON.stringify({case:'signout-delete-failure',...failedDelete,observations:observations.length}));assert.equal(failedDelete.status,200);assert.equal(observations.length,0);
const expectedCookies = ['session_token','session_data','dont_remember'].map(name => `better-auth.${name}=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax`);
for (const result of [failedRead, failedDelete]) {
 assert.deepEqual(result.body,{success:true});
 assert.deepEqual(result.cookies,expectedCookies);
}
assert.ok(calls.some(([method,model])=>method==='create' && model==='account'));
assert.ok(calls.some(([method,model])=>method==='create' && model==='session'));
console.log('Better Auth fault boundaries: user422, four unexpected empty500 branches and best-effort signout200/cookie clearing verified.');
