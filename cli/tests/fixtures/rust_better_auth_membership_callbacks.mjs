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


for(const flow of ['add','accept'])for(const mode of ['zero','one','throw','reject','missing-org','missing-user','pending']){
 const calls=[];let release,entered;const reached=new Promise(r=>entered=r);
 const policy={membershipLimit:(user,org)=>{calls.push({user:user.id,org:org.id,keys:Object.keys(org).sort()});if(mode==='throw')throw Error('secret');if(mode==='reject')return Promise.reject(Error('secret'));if(mode==='pending'){entered();return new Promise(r=>release=()=>r(1));}return Promise.resolve(mode==='zero'?0:1);}};
 const auth=setup(policy,0);if(mode==='missing-org')ref.database.organization=[];if(mode==='missing-user')ref.database.user=ref.database.user.filter(x=>x.id!=='target-user');
 ref.database.invitation.push({id:'quota-invite',email:'org-policy@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:'target-user',expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});
 const pending=flow==='add'?auth.api.addMember({body:{userId:'target-user',role:'member',organizationId:'org-policy-existing'},asResponse:true}).then(async r=>({status:r.status,body:await r.text()})).catch(e=>({status:500,escaped:true,name:e.name})):call(auth,'/organization/accept-invitation',{invitationId:'quota-invite'},cookie);
 if(mode==='pending'){await reached;assert.equal(ref.database.member.length,0);assert.equal(ref.database.invitation[0].status,'pending');release();}
 const out=await pending;let expected=mode==='zero'?403:['throw','reject'].includes(mode)?500:mode==='missing-org'||(mode==='missing-user'&&flow==='add')?400:200;
 assert.equal(out.status,expected,flow+mode);assert.equal(calls.length,expected===400?0:1);if(calls.length)assert.equal(calls[0].user,flow==='add'?'target-user':uid);rows.push({flow,mode,status:out.status,escaped:out.escaped??false,calls,invitation:ref.database.invitation[0].status});
}

for(const flow of ['add','accept'])for(const mutation of ['count','duplicate','delete-user','delete-org','revoke','cancel-invite','change-invite']){
 let release,entered;const ready=new Promise(r=>entered=r);const calls=[];
 const auth=setup({membershipLimit:async(user,org)=>{calls.push({user:user.id,org:org.id});entered();return new Promise(r=>release=()=>r(1));}},0);
 ref.database.invitation.push({id:'quota-invite',email:'org-policy@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:'target-user',expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});
 const pending=flow==='add'?auth.api.addMember({body:{userId:'target-user',role:'member',organizationId:'org-policy-existing'},asResponse:true}).then(async r=>({status:r.status,body:await r.json()})).catch(e=>({status:500,escaped:true,name:e.name})):call(auth,'/organization/accept-invitation',{invitationId:'quota-invite'},cookie);
 await ready;const target=flow==='add'?'target-user':uid;
 if(mutation==='count'||mutation==='duplicate')ref.database.member.push({id:'concurrent-member',userId:mutation==='count'?'someone':target,organizationId:'org-policy-existing',role:'member',createdAt:new Date()});
 if(mutation==='delete-user')ref.database.user=ref.database.user.filter(x=>x.id!==target);
 if(mutation==='delete-org')ref.database.organization=[];
 if(mutation==='revoke')ref.database.session=[];
 if(mutation==='cancel-invite')ref.database.invitation[0].status='canceled';
 if(mutation==='change-invite'){ref.database.invitation[0].role='admin';ref.database.invitation[0].email='someone@example.test';}
 release();const out=await pending;assert.equal(out.status,flow==='accept'&&mutation==='cancel-invite'?400:200,flow+mutation);
 if(flow==='accept'&&mutation==='change-invite')assert.equal(out.body.member.role,'admin');rows.push({flow,mutation,status:out.status,role:flow==='accept'?out.body.member?.role:out.body.role,invitation:ref.database.invitation[0].status,members:ref.database.member.map(x=>({user:x.userId,role:x.role}))});
}

for(const flow of ['add','accept']) for(const count of [0,1])for(const [label,limit]of [['zero',0],['negative-zero',-0],['fraction',0.5],['one',1],['negative',-0.5],['nan',NaN],['positive-infinity',Infinity],['negative-infinity',-Infinity],['above-u32',4294967296],['max-number',Number.MAX_VALUE]]){
 const auth=setup({membershipLimit:async()=>limit},count);if(count)ref.database.member[0].userId='someone';
 ref.database.invitation.push({id:'quota-invite',email:'org-policy@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:'target-user',expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});
 const out=flow==='add'?await auth.api.addMember({body:{userId:'target-user',role:'member',organizationId:'org-policy-existing'},asResponse:true}):await call(auth,'/organization/accept-invitation',{invitationId:'quota-invite'},cookie);
 assert.equal(out.status,count>=limit?403:200,flow+count+label);rows.push({flow,count,label,status:out.status});
}

for(const mutation of ['revoke','token','session-user','team-delete','team-org','invite-org']){
 let release,entered;const ready=new Promise(r=>entered=r);
 const auth=setup({membershipLimit:async()=>{entered();return new Promise(r=>release=()=>r(Infinity));}},0);
 ref.database.team.push({id:'test-team',name:'Team',organizationId:'org-policy-existing',createdAt:new Date()});
 ref.database.invitation.push({id:'quota-invite',teamId:'test-team',email:'org-policy@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:'target-user',expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});
 const pending=call(auth,'/organization/accept-invitation',{invitationId:'quota-invite'},cookie);await ready;
 if(mutation==='revoke')ref.database.session=[];if(mutation==='token')ref.database.session[0].token='changed';if(mutation==='session-user')ref.database.session[0].userId='target-user';if(mutation==='team-delete')ref.database.team=[];if(mutation==='team-org')ref.database.team[0].organizationId='elsewhere';if(mutation==='invite-org'){ref.database.invitation[0].organizationId='elsewhere';ref.database.team[0].organizationId='elsewhere';}
 release();const out=await pending;assert.equal(out.status,['revoke','token'].includes(mutation)?500:['team-delete','team-org'].includes(mutation)?400:200);assert.equal(ref.database.invitation[0].status,out.status===200?'accepted':'pending');assert.equal(ref.database.member.length,out.status===200?1:0);assert.equal(ref.database.teamMember.length,out.status===200?1:0);rows.push({mutation,status:out.status,body:out.body,invitation:ref.database.invitation[0].status,members:ref.database.member.length,teamMembers:ref.database.teamMember.length,cookie:Boolean(out.cookie)});
}

for(const maximum of [undefined,2])for(const mutation of ['delete','move']){
 let release,entered;const ready=new Promise(r=>entered=r);const auth=setup({teams:{enabled:true,...(maximum===undefined?{}:{maximumMembersPerTeam:maximum})},membershipLimit:async()=>{entered();return new Promise(r=>release=()=>r(Infinity));}},0);
 ref.database.team.push({id:'test-team',memberCount:0,name:'Team',organizationId:'org-policy-existing',createdAt:new Date()});
 const pending=auth.api.addMember({body:{userId:'target-user',role:'member',organizationId:'org-policy-existing',teamId:'test-team'},asResponse:true}).then(async r=>({status:r.status,body:await r.json()})).catch(e=>({status:500,error:e.name}));
 await ready;if(mutation==='delete')ref.database.team=[];else ref.database.team[0].organizationId='elsewhere';release();const out=await pending;assert.equal(out.status,maximum!==undefined&&mutation==='delete'?403:200);assert.equal(ref.database.member.length,out.status===200?1:0);assert.equal(ref.database.teamMember.length,out.status===200?1:0);rows.push({maximum:maximum??null,mutation,...out,members:ref.database.member.length,teamMembers:ref.database.teamMember.length});
}


for(const mode of ['scalar','callback','explicit']){
 let calls=0;const auth=setup({membershipLimit:mode==='scalar'?1:async()=>{calls++;return 0;}},2);ref.database.user.push({...structuredClone(ref.database.user[0]),id:'other-1',email:'other@example.test'});
 const out=await call(auth,'/organization/get-full-organization?organizationId=org-policy-existing'+(mode==='explicit'?'&membersLimit=1':''),null,cookie);assert.equal(out.status,mode==='scalar'?500:200);assert.equal(calls,0);rows.push({mode:'hydration-'+mode,status:out.status,calls,members:out.body?.members?.length});
}
console.log('PASS awaited membership policy '+rows.length,JSON.stringify(rows));
