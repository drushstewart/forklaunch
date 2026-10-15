import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
const root=new URL('../../..',import.meta.url).pathname.replace(/\/$/,'');
const deps=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
const {betterAuthReference}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const {betterAuth}=await import(pathToFileURL(deps+'/node_modules/better-auth/dist/index.mjs'));
const {organization}=await import(pathToFileURL(deps+'/node_modules/better-auth/dist/plugins/organization/index.mjs'));
const ref=await betterAuthReference(root);
const call=async(auth,path,body,cookie='')=>{const r=await auth.handler(new Request('http://localhost:3000/api/auth'+path,{method:body?'POST':'GET',headers:{origin:'http://localhost:3000','content-type':'application/json',cookie},...(body?{body:JSON.stringify(body)}:{})}));const text=await r.text();return {status:r.status,body:text?JSON.parse(text):null,cookie:r.headers.getSetCookie().map(x=>x.split(';')[0]).join('; ')};};
const signup=await call(ref.auth,'/sign-up/email',{name:'Owned',firstName:'Owned',lastName:'User',email:'org-policy@example.test',password:'owned-password'});assert.equal(signup.status,200);
const saved=structuredClone(ref.database),uid=signup.body.user.id,cookie=signup.cookie;
const secret=(await ref.auth.$context).secret;
const rows=[];
function setup(policy={},members=0){for(const key of Object.keys(ref.database))ref.database[key]=structuredClone(saved[key]);ref.database.user[0].emailVerified=true;
ref.database.user.push({...structuredClone(ref.database.user[0]),id:'target-user',email:'target-policy@example.test'});
ref.database.organization.push({id:'org-policy-existing',name:'Existing',slug:'existing',createdAt:new Date(),subscription:'free',status:'active'});
for(let i=0;i<members;i++)ref.database.member.push({id:'member-'+i,organizationId:'org-policy-existing',userId:i?('other-'+i):uid,role:i?'member':'viewer',createdAt:new Date()});
const auth=betterAuth({...ref.config,secret,logger:{disabled:true},rateLimit:{enabled:false},plugins:ref.config.plugins.map(p=>p.id==='organization'?organization({...p.options,...policy}):p)});return auth;}

for(const mode of ['allow','deny','allow-throw','allow-reject','limit','limit-throw','limit-reject','duplicate','no-session','pending']){
 const calls=[]; let release;
 const auth=setup({allowUserToCreateOrganization:(user)=>{calls.push(['allow',Object.keys(user).sort(),user.id]);if(mode==='allow-throw')throw Error('secret');if(mode==='allow-reject')return Promise.reject(Error('secret'));if(mode==='pending')return new Promise(r=>release=()=>r(true));return Promise.resolve(mode!=='deny');},organizationLimit:(user)=>{calls.push(['limit',user.id]);if(mode==='limit-throw')throw Error('secret');if(mode==='limit-reject')return Promise.reject(Error('secret'));return Promise.resolve(mode==='limit');}},0);
 const pending=call(auth,'/organization/create',{name:'Next',slug:mode==='duplicate'?'existing':'next'},mode==='no-session'?'':cookie);
 if(mode==='pending'){await new Promise(r=>setTimeout(r,20));assert.equal(ref.database.organization.length,1);assert.equal(calls.length,1);release();}
 const out=await pending; const expected=['allow','pending'].includes(mode)?200:mode==='no-session'?401:mode==='duplicate'?400:mode.includes('throw')||mode.includes('reject')?500:403;
 assert.equal(out.status,expected,mode);assert.deepEqual(calls.map(x=>x[0]),mode==='no-session'?[]:['deny','allow-throw','allow-reject'].includes(mode)?['allow']:['allow','limit']);
 rows.push({mode,status:out.status,calls,organizations:ref.database.organization.length});
}

for(const stage of ['allow','limit']) for(const mutation of ['revoke','delete-user','change-user','slug','quota']){
 let release,entered;const reached=new Promise(r=>entered=r);let calls=[];
 const hook=(name,user)=>{calls.push([name,user.email]);if(name===stage){entered();return new Promise(r=>release=()=>r(name==='allow'));}return Promise.resolve(name==='allow');};
 const auth=setup({allowUserToCreateOrganization:u=>hook('allow',u),organizationLimit:u=>hook('limit',u)},0);
 const pending=call(auth,'/organization/create',{name:'Next',slug:'next'},cookie);await reached;
 if(mutation==='revoke')ref.database.session=[];
 if(mutation==='delete-user')ref.database.user=ref.database.user.filter(u=>u.id!==uid);
 if(mutation==='change-user')ref.database.user.find(u=>u.id===uid).email='changed@example.test';
 if(mutation==='slug')ref.database.organization.push({id:'concurrent-org',name:'Concurrent',slug:'next',createdAt:new Date()});
 if(mutation==='quota')ref.database.member.push({id:'concurrent-member',organizationId:'org-policy-existing',userId:uid,role:'member',createdAt:new Date()});
 release();const out=await pending;assert.deepEqual(calls,[['allow','org-policy@example.test'],['limit','org-policy@example.test']]);assert.equal(out.status,mutation==='slug'?400:200,stage+mutation);
 rows.push({stage,mutation,status:out.status,calls,memberExists:ref.database.member.some(m=>m.userId===uid&&m.organizationId!== 'org-policy-existing')});
}

for(const stage of ['allow','limit']) for(const mutation of ['token','session-id','session-user','replace-token']){
 let release,entered;const reached=new Promise(r=>entered=r);let calls=[];
 const hook=(name,user)=>{calls.push([name,user.email]);if(name===stage){entered();return new Promise(r=>release=()=>r(name==='allow'));}return Promise.resolve(name==='allow');};
 const auth=setup({allowUserToCreateOrganization:u=>hook('allow',u),organizationLimit:u=>hook('limit',u)},0);
 const pending=call(auth,'/organization/create',{name:'Next',slug:'next'},cookie);await reached;
 const current=ref.database.session[0],originalToken=current.token;
 if(mutation==='token')current.token='changed-token';
 if(mutation==='session-id')current.id='changed-session-id';
 if(mutation==='session-user')current.userId='target-user';
 if(mutation==='replace-token'){current.token='changed-token';ref.database.session.push({...structuredClone(current),id:'replacement-session',token:originalToken,userId:'target-user'});}
 release();const out=await pending;assert.deepEqual(calls,[['allow','org-policy@example.test'],['limit','org-policy@example.test']]);assert.equal(out.status,mutation==='slug'?400:200,stage+mutation);
 for(const session of ref.database.session){const shouldUpdate=mutation!=='token'&&(mutation!=='replace-token'||session.id==='replacement-session');assert.equal(Boolean(session.activeOrganizationId),shouldUpdate);}
 rows.push({stage,mutation,status:out.status,calls,sessions:ref.database.session.map(x=>({id:x.id,userId:x.userId,active:x.activeOrganizationId??null}))});
}
for(const allowed of [false,true]){
 const original=ref.config.database;const calls=[];
 ref.config.database=(config)=>{const adapter=original(config);return {...adapter,findMany:async(args)=>{if(args.model==='member'){calls.push('read');throw Error('owned list failure');}return adapter.findMany(args);}};}; 
 const auth=setup({allowUserToCreateOrganization:async()=>{calls.push('allow');return allowed;},organizationLimit:async()=>{calls.push('limit');return false;}},0);
 const out=await call(auth,'/organization/create',{name:'Next',slug:'next'},cookie);ref.config.database=original;
 assert.equal(out.status,allowed?500:403);assert.deepEqual(calls,allowed?['allow','read']:['allow']);assert.equal(ref.database.organization.length,1);rows.push({mode:'membership-read-failure',allowed,status:out.status,calls});
}
{
 const RealDate=Date;let clock=RealDate.now();const start=clock;
 globalThis.Date=class extends RealDate{constructor(...args){super(...(args.length?args:[clock]))}static now(){return clock}};
 try {
  const auth=setup({allowUserToCreateOrganization:async()=>{clock+=2000;return true;},organizationLimit:async()=>{clock+=3000;return false;}},0);
  const out=await call(auth,'/organization/create',{name:'Delayed',slug:'delayed'},cookie);assert.equal(out.status,200);assert.equal(RealDate.parse(out.body.createdAt),start+5000);assert.equal(RealDate.parse(out.body.members[0].createdAt),start+5000);rows.push({mode:'delayed-created-at',status:out.status,elapsed:RealDate.parse(out.body.createdAt)-start});
 }finally{globalThis.Date=RealDate;}
}
console.log('PASS async organization creation '+rows.length,JSON.stringify(rows));
