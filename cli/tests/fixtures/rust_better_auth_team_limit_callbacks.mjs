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
function setup(teams={},members=1){for(const key of Object.keys(ref.database))ref.database[key]=structuredClone(saved[key]);ref.database.user[0].emailVerified=true;
ref.database.user.push({...structuredClone(ref.database.user[0]),id:'target-user',email:'target-policy@example.test'});
ref.database.organization.push({id:'org-policy-existing',name:'Existing',slug:'existing',createdAt:new Date(),subscription:'free',status:'active'});
for(let i=0;i<members;i++)ref.database.member.push({id:'member-'+i,organizationId:'org-policy-existing',userId:i?('other-'+i):uid,role:i?'member':'owner',createdAt:new Date()});
ref.database.organizationRole.push({id:'owner-grant',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({team:['create','delete','update'],member:['create','update'],invitation:['create']}),createdAt:new Date()});
const auth=betterAuth({...ref.config,secret,logger:{disabled:true},rateLimit:{enabled:false},plugins:ref.config.plugins.map(p=>p.id==='organization'?organization({...p.options,teams:{...p.options.teams,...teams}}):p)});return auth;}

function team(id='team-existing',count=0){ref.database.team.push({id,organizationId:'org-policy-existing',name:id,memberCount:count,createdAt:new Date()});}

const results=[];
const org='org-policy-existing',tid='team-existing';
const addPair=()=>ref.database.teamMember.push({id:'new-pair',teamId:tid,userId:'other-user',createdAt:new Date()});
async function operation(auth,kind){
 if(kind==='team')return call(auth,'/organization/add-team-member',{organizationId:org,teamId:tid,userId:'target-user'},cookie);
 if(kind==='invite')return call(auth,'/organization/invite-member',{organizationId:org,email:'new-team@example.test',role:'member',teamId:tid},cookie);
 if(kind==='accept')return call(auth,'/organization/accept-invitation',{invitationId:'accept-team'},cookie);
 try { const r=await auth.api.addMember({body:{organizationId:org,userId:'target-user',role:'member',teamId:tid},headers:new Headers({cookie}),asResponse:true});
 const text=await r.text();return {status:r.status,body:text?JSON.parse(text):null}; } catch(e) { if(e.message!=='OWNED')throw e; return {status:500,thrown:true}; }
}
function prepare(kind){team();
 if(kind==='team')ref.database.member.push({id:'target-member',organizationId:org,userId:'target-user',role:'member',createdAt:new Date()});
 if(kind==='accept'){ref.database.member[0].userId='target-user';ref.database.invitation.push({id:'accept-team',organizationId:org,email:'org-policy@example.test',role:'member',inviterId:'target-user',teamId:tid,status:'pending',expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});}
}
for(const kind of ['team','invite','accept','member'])for(const [label,limit]of [['zero',0],['negative',-1],['fraction',0.5],['nan',NaN],['positiveInfinity',Infinity],['negativeInfinity',-Infinity],['large',2**32]]){
 let calls=0;const auth=setup({maximumMembersPerTeam:async(data)=>{calls++;assert.deepEqual(Object.keys(data),['teamId','session','organizationId']);assert.equal(data.session.user.id,uid);assert.equal(data.session.session.token,ref.database.session[0].token);return limit;}});prepare(kind);const out=await operation(auth,kind);assert.equal(calls,1);results.push({kind,label,status:out.status,code:out.body?.code});
}
for(const [label,limit]of [['zero',0],['negative',-1],['fraction',0.5],['nan',NaN],['positiveInfinity',Infinity],['negativeInfinity',-Infinity],['large',2**32]]){
 let calls=0;const auth=setup({maximumTeams:async(data,ctx)=>{calls++;assert.equal(data.organizationId,org);assert.equal(data.session.user.id,uid);assert.ok(ctx.context.adapter);return limit;}});team();const out=await call(auth,'/organization/create-team',{organizationId:org,name:'next'},cookie);assert.equal(calls,1);results.push({kind:'create',label,status:out.status,code:out.body?.code});
}
for(const kind of ['team','invite','accept','member'])for(const mode of ['add-seat','delete-team','revoke-session','delete-member','sync-error','async-error']){
 let calls=0;const auth=setup({maximumMembersPerTeam:(data)=>{calls++;if(mode==='sync-error')throw Error('OWNED');return (async()=>{if(mode==='async-error')throw Error('OWNED');if(mode==='add-seat')addPair();if(mode==='delete-team')ref.database.team.length=0;if(mode==='revoke-session')ref.database.session.length=0;if(mode==='delete-member')ref.database.member.length=0;return 1;})();}});prepare(kind);const out=await operation(auth,kind);assert.equal(calls,1);results.push({kind,mode,status:out.status,code:out.body?.code,members:ref.database.member.length,teamMembers:ref.database.teamMember.length,invitation:ref.database.invitation[0]?.status});
}
for(const mode of ['add-team','delete-team','delete-organization','revoke-session','sync-error','async-error']){
 const auth=setup({maximumTeams:()=>{if(mode==='sync-error')throw Error('OWNED');return (async()=>{if(mode==='async-error')throw Error('OWNED');if(mode==='add-team')team('intervening');if(mode==='delete-team')ref.database.team.length=0;if(mode==='delete-organization')ref.database.organization.length=0;if(mode==='revoke-session')ref.database.session.length=0;return 1;})();}});if(mode==='delete-team')team();const out=await call(auth,'/organization/create-team',{organizationId:org,name:'next'},cookie);results.push({kind:'create',mode,status:out.status,code:out.body?.code});
}
const {getCurrentAdapter}=await import(pathToFileURL(deps+'/node_modules/@better-auth/core/dist/context/transaction.mjs'));
for(const mode of ['add-seat','delete-team','revoke-session','delete-member','change-invitation']){
 let auth;auth=setup({maximumMembersPerTeam:async()=>{const adapter=await getCurrentAdapter((await auth.$context).adapter);
 if(mode==='add-seat')await adapter.create({model:'teamMember',data:{id:'current-pair',teamId:tid,userId:'other-user',createdAt:new Date()},forceAllowId:true});
 if(mode==='delete-team')await adapter.deleteMany({model:'team',where:[{field:'id',value:tid}]});
 if(mode==='revoke-session')await adapter.deleteMany({model:'session',where:[]});
 if(mode==='delete-member')await adapter.deleteMany({model:'member',where:[]});
 if(mode==='change-invitation')await adapter.update({model:'invitation',where:[{field:'id',value:'accept-team'}],update:{status:'canceled'}});
 return 1;}});prepare('accept');const out=await operation(auth,'accept');results.push({kind:'accept-current-adapter',mode,status:out.status,code:out.body?.code,members:ref.database.member.length,teamMembers:ref.database.teamMember.length,invitation:ref.database.invitation[0]?.status});
}

for(const kind of ['team','member','accept']){
 let seen;const auth=setup({maximumMembersPerTeam:async()=>{const adapter=await getCurrentAdapter((await auth.$context).adapter);seen={members:(await adapter.findMany({model:'member',where:[{field:'organizationId',value:org}]})).length,invitation:kind==='accept'?(await adapter.findOne({model:'invitation',where:[{field:'id',value:'accept-team'}]})).status:null};return 10;}});prepare(kind);const out=await operation(auth,kind);assert.equal(out.status,200);assert.equal(seen.members,kind==='member'||kind==='team'?2:1);if(kind==='accept')assert.equal(seen.invitation,'accepted');results.push({kind,visibility:seen,status:out.status});
}
{
 let calls=0;const auth=setup({maximumMembersPerTeam:()=>{calls++;return 1;}});prepare('member');const r=await auth.api.addMember({body:{organizationId:org,userId:'target-user',role:'member',teamId:tid},asResponse:true});assert.equal(r.status,401);assert.equal(calls,0);assert.equal(ref.database.member.length,1);results.push({kind:'member-no-session',status:r.status,calls});
}
for(const kind of ['team','accept']){
 let calls=0;const auth=setup({maximumMembersPerTeam:()=>{calls++;return NaN;}});prepare(kind);ref.database.teamMember.push({id:'existing',teamId:tid,userId:kind==='team'?'target-user':uid,createdAt:new Date()});const out=await operation(auth,kind);assert.equal(out.status,200);assert.equal(calls,1);results.push({kind,duplicate:true,status:out.status,calls});
}
for(const kind of ['invite','accept']){
 const seen=[];const auth=setup({maximumMembersPerTeam:({teamId})=>{seen.push(teamId);return 10;}});prepare(kind);team('second');let out;if(kind==='invite')out=await call(auth,'/organization/invite-member',{organizationId:org,email:'new-team@example.test',role:'member',teamId:[tid,'second']},cookie);else{ref.database.invitation[0].teamId=tid+',second';out=await operation(auth,kind);}assert.equal(out.status,200);assert.deepEqual(seen,[tid,'second']);results.push({kind,multiple:seen,status:out.status});
}

{
 let calls=0;const auth=setup({maximumTeams:()=>{calls++;return -1;},maximumMembersPerTeam:()=>{calls++;return 0;}});const out=await call(auth,'/organization/create',{name:'Implicit',slug:'implicit'},cookie);assert.equal(out.status,200);assert.equal(calls,0);results.push({kind:'implicit-default-team',status:200,calls});
}
{
 let calls=0;const auth=setup({maximumTeams:({session,organizationId})=>{calls++;assert.equal(session,null);assert.equal(organizationId,org);return 0;}});const r=await auth.api.createTeam({body:{organizationId:org,name:'Server'},asResponse:true});assert.equal(r.status,200);assert.equal(calls,1);results.push({kind:'server-create-team',status:200,calls});
}
{
 let calls=0;const auth=setup({maximumTeams:()=>{calls++;return 0;}});const r=await auth.api.createTeam({body:{organizationId:'',name:'Empty scope'},asResponse:true});const out=await r.json();assert.equal(r.status,400);assert.equal(out.code,'NO_ACTIVE_ORGANIZATION');assert.equal(calls,0);results.push({kind:'server-empty-organization',status:r.status,calls});
}
for(const row of results){
 if(row.label){const expected=row.kind==='create'?(['negative','fraction','negativeInfinity'].includes(row.label)?400:200):(['zero','negative','negativeInfinity'].includes(row.label)||(row.label==='nan'&&row.kind!=='invite')?403:200);assert.equal(row.status,expected,JSON.stringify(row));}
 if(row.mode){let expected;
 if(row.mode.endsWith('error'))expected=500;
 else if(row.kind==='create')expected=['delete-team','delete-organization'].includes(row.mode)?400:200;
 else if(row.kind==='accept-current-adapter')expected=['add-seat','delete-team'].includes(row.mode)?403:row.mode==='revoke-session'?500:200;
 else expected=['team','member'].includes(row.kind)&&['add-seat','delete-team'].includes(row.mode)?403:200;
 assert.equal(row.status,expected,JSON.stringify(row));
 if(row.kind==='accept-current-adapter'&&row.status!==200){assert.equal(row.invitation,'pending');assert.equal(row.teamMembers,0);assert.equal(row.members,1);}
 }
}
console.log('PASS team callback actual '+results.length,JSON.stringify(results));
