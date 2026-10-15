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


function admit(){ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'owner-grant',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({invitation:['create']}),createdAt:new Date()});}
function pending(id='old',email='new-invite@example.test'){return {id,email,organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:uid,expiresAt:new Date(Date.now()+3600000),createdAt:new Date()};}
const body={email:'new-invite@example.test',role:'member',organizationId:'org-policy-existing'};
for(const [label,limit]of [['zero',0],['negative',-1],['fraction',0.5],['nan',NaN],['positiveInfinity',Infinity],['negativeInfinity',-Infinity],['large',2**32]]){
 let calls=0;const auth=setup({invitationLimit:async(data,ctx)=>{calls++;assert.deepEqual(Object.keys(data),['user','organization','member']);assert.equal(data.user.id,uid);assert.equal(data.organization.id,'org-policy-existing');assert.equal(data.member.userId,uid);assert.ok(ctx.adapter);return limit;}},1);admit();
 const out=await call(auth,'/organization/invite-member',body,cookie);assert.equal(out.status,0>=limit?403:200,JSON.stringify(out));assert.equal(calls,1);rows.push({label,status:out.status,code:out.body?.code});
}
for(const mode of ['resend','duplicate','cancel-quota','cancel-sync-error','cancel-async-error','missing-team']){
 let calls=0;const auth=setup({cancelPendingInvitationsOnReInvite:mode.startsWith('cancel'),invitationLimit:()=>{calls++;if(mode==='cancel-sync-error')throw Error('OWNED');if(mode==='cancel-async-error')return Promise.reject(Error('OWNED'));return Promise.resolve(mode==='missing-team'?100:0);}},1);admit();
 if(mode!=='missing-team')ref.database.invitation.push(pending());
 const out=await call(auth,'/organization/invite-member',{...body,...(mode==='resend'?{resend:true}:{}),...(mode==='missing-team'?{teamId:'missing'}:{})},cookie);
 const expected=mode==='resend'?200:mode==='duplicate'||mode==='missing-team'?400:mode==='cancel-quota'?403:500;
 assert.equal(out.status,expected,JSON.stringify({mode,out}));assert.equal(calls,['resend','duplicate'].includes(mode)?0:1);
 if(mode.startsWith('cancel'))assert.equal(ref.database.invitation[0].status,'canceled');rows.push({mode,status:out.status,code:out.body?.code,calls});
}
for(const mode of ['add-pending','delete-pending','revoke-session','delete-user','delete-member','demote-member','delete-organization','add-target-member','add-same-email','mutate-target-email']){
 let calls=0;const auth=setup({invitationLimit:async()=>{calls++;
 if(mode==='add-pending')ref.database.invitation.push(pending('new-pending','other@example.test'));
 if(mode==='delete-pending')ref.database.invitation.length=0;
 if(mode==='revoke-session')ref.database.session.length=0;
 if(mode==='delete-user')ref.database.user=ref.database.user.filter(x=>x.id!==uid);
 if(mode==='delete-member')ref.database.member.length=0;
 if(mode==='demote-member')ref.database.member[0].role='member';
 if(mode==='delete-organization')ref.database.organization.length=0;
 if(mode==='add-target-member'){ref.database.user.find(x=>x.id==='target-user').email=body.email;ref.database.member.push({id:'target-member',organizationId:'org-policy-existing',userId:'target-user',role:'member',createdAt:new Date()});}
 if(mode==='add-same-email')ref.database.invitation.push(pending('new-duplicate'));
 if(mode==='mutate-target-email')ref.database.user.find(x=>x.id===uid).email='changed@example.test';
 return mode==='add-pending'||mode==='delete-pending'?1:100;
 }},1);admit();if(mode==='delete-pending')ref.database.invitation.push(pending('old-pending','other@example.test'));
 const out=await call(auth,'/organization/invite-member',body,cookie);assert.equal(out.status,mode==='add-pending'?403:200,JSON.stringify({mode,out}));assert.equal(calls,1);rows.push({mode,status:out.status,code:out.body?.code});
}

for(const [count,expired,limit,expected]of [[101,0,101,200],[101,100,1,200],[1,0,1,403]]){
 const auth=setup({invitationLimit:async()=>limit},1);admit();for(let i=0;i<count;i++)ref.database.invitation.push({...pending('window-'+i,'window-'+i+'@example.test'),expiresAt:new Date(Date.now()+(i<expired?-3600000:3600000))});
 const out=await call(auth,'/organization/invite-member',body,cookie);assert.equal(out.status,expected);rows.push({window:{count,expired,limit},status:out.status});
}
{
 const RealDate=Date;const initial=RealDate.now(),later=initial+2000;
 const auth=setup({invitationExpiresIn:10,invitationLimit:async()=>{globalThis.Date=class extends RealDate{constructor(...args){super(...(args.length?args:[later]));}static now(){return later;}};return 1;}},1);admit();ref.database.invitation.push({...pending('expires','other@example.test'),expiresAt:new RealDate(initial+1000)});
 try{const out=await call(auth,'/organization/invite-member',body,cookie);assert.equal(out.status,200);assert.equal(RealDate.parse(out.body.createdAt),later);assert.equal(RealDate.parse(out.body.expiresAt),later+10000);rows.push({clock:'post-callback-expiry-filter-and-insert',status:200});}finally{globalThis.Date=RealDate;}
}
for(const failure of ['none','quota','delivery-preparation','delivery-future']){
 const events=[];const auth=setup({cancelPendingInvitationsOnReInvite:true,invitationLimit:async(data)=>{events.push('quota');assert.equal(data.member.role,'owner');assert.equal(ref.database.invitation[0].status,'canceled');return failure==='quota'?0:100;},sendInvitationEmail:()=>{events.push('delivery');assert.equal(ref.database.invitation.length,2);if(failure==='delivery-preparation')throw Error('OWNED');return failure==='delivery-future'?Promise.reject(Error('OWNED')):Promise.resolve();}},1);admit();ref.database.invitation.push(pending());
 const out=await call(auth,'/organization/invite-member',body,cookie);assert.equal(out.status,failure==='quota'?403:failure==='delivery-preparation'?500:200);assert.deepEqual(events,failure==='quota'?['quota']:['quota','delivery']);rows.push({failure,status:out.status,events});
}
console.log('PASS invitation callback actual '+rows.length,JSON.stringify(rows));
