import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
const root=new URL('../../..',import.meta.url).pathname.replace(/\/$/,'');
const deps=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(deps,'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES');
const {betterAuthReference}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const {betterAuth}=await import(pathToFileURL(deps+'/node_modules/better-auth/dist/index.mjs'));
const {jwt}=await import(pathToFileURL(deps+'/node_modules/better-auth/dist/plugins/jwt/index.mjs'));
const {organization}=await import(pathToFileURL(deps+'/node_modules/better-auth/dist/plugins/organization/index.mjs'));
const ref=await betterAuthReference(root);
const call=async(auth,path,body,cookie='',method)=>{const r=await auth.handler(new Request('http://localhost:3000/api/auth'+path,{method:method??(body?'POST':'GET'),headers:{origin:'http://localhost:3000','content-type':'application/json',cookie},...(body?{body:JSON.stringify(body)}:{})}));const raw=await r.text();let data;try{data=JSON.parse(raw);}catch{}return {status:r.status,data,raw,cookie:r.headers.getSetCookie().map(x=>x.split(';')[0]).join('; ')};};
const signup=await call(ref.auth,'/sign-up/email',{name:'Owned',firstName:'Owned',lastName:'User',email:'org-feature@example.test',password:'owned-password'});assert.equal(signup.status,200);
const saved=structuredClone(ref.database),uid=signup.data.user.id,cookie=signup.cookie,secret=(await ref.auth.$context).secret;
for(const teams of [true,false])for(const dynamic of [true,false]){
 for(const key of Object.keys(saved))ref.database[key]=structuredClone(saved[key]);ref.database.user[0].emailVerified=true;
 const original=ref.config.plugins.find(p=>p.id==='organization');
 let delivery;let quotaCalls=0;let jwtSession;
 const plugin=organization({...original.options,sendInvitationEmail:async(data)=>{delivery={teamPresent:Object.hasOwn(data.invitation,'teamId'),teamId:data.invitation.teamId};},roles:{...original.options.roles,owner:original.options.ac.newRole({invitation:['create'],member:['create','update','delete'],team:['create']})},teams:{...original.options.teams,enabled:teams,maximumMembersPerTeam:()=>{quotaCalls++;return 99;}},dynamicAccessControl:{...original.options.dynamicAccessControl,enabled:dynamic}});
 const auth=betterAuth({...ref.config,secret,logger:{disabled:true},rateLimit:{enabled:false},plugins:ref.config.plugins.map(p=>p.id==='organization'?plugin:p.id==='jwt'?jwt({...p.options,jwt:{...p.options?.jwt,definePayload:({session})=>{jwtSession=session;return {};}}}):p)});
 const enabled=Object.values(plugin.endpoints).map(e=>e.path).sort();
 const created=await call(auth,'/organization/create',{name:'Feature',slug:'feature'},cookie);assert.equal(created.status,200);
 const org=created.data.id;
 const createdTeams=ref.database.team.length;
 const full=await call(auth,'/organization/get-full-organization?organizationId='+org,null,cookie);
 const routes=[];
 const teamRoutes=['create-team','update-team','remove-team','list-teams','list-user-teams','list-team-members','set-active-team','add-team-member','remove-team-member'].map(p=>'/organization/'+p);
 const roleRoutes=['create-role','update-role','delete-role','list-roles','get-role'].map(p=>'/organization/'+p);
 for(const path of [...teamRoutes,...roleRoutes]){
  for(const method of ['GET','POST','HEAD']){const out=await call(auth,path,method==='POST'?{}:null,'',method);routes.push({path,method,status:out.status,code:out.data?.code??null});}
 }
 for(const route of routes){const allowed=teamRoutes.includes(route.path)?teams:dynamic;if(!allowed||route.method==='HEAD')assert.equal(route.status,404,JSON.stringify(route));}
 assert.equal(enabled.length,23+(teams?9:0)+(dynamic?5:0));
 const schema=await call(auth,'/open-api/generate-schema');
 assert.equal(schema.status,200);
 assert.equal(Object.hasOwn(schema.data.components.schemas,'Team'),teams);
 assert.equal(Object.hasOwn(schema.data.components.schemas,'TeamMember'),teams);
 assert.equal(Object.hasOwn(schema.data.components.schemas,'OrganizationRole'),dynamic);
 assert.equal(Object.hasOwn(schema.data.components.schemas.Session.properties,'activeTeamId'),teams);
 assert.equal(Object.hasOwn(schema.data.components.schemas.Invitation.properties,'teamId'),teams);
 for(const path of [...teamRoutes,...roleRoutes])assert.equal(Object.hasOwn(schema.data.paths,path),teamRoutes.includes(path)?teams:dynamic,path);
 assert.equal(createdTeams,teams?1:0);assert.equal(full.status,200);assert.equal(Object.hasOwn(full.data,'teams'),teams);
 ref.database.organizationRole.push({id:'stored-owner-grant',organizationId:org,role:'owner',permission:JSON.stringify({team:['create'],member:['create'],invitation:['create'],platform:['custom']}),createdAt:new Date()});
 const permission=await call(auth,'/organization/has-permission',{organizationId:org,permissions:{platform:['custom']}},cookie);
 assert.equal(permission.status,200);assert.equal(permission.data.success,dynamic);
 ref.database.session[0].activeTeamId='legacy-team';
 const sessionOut=await call(auth,'/get-session',null,cookie);
 const invite=await call(auth,'/organization/invite-member',{organizationId:org,email:'recipient@example.test',role:'member',teamId:'nonexistent-team'},cookie);
 assert.ok(jwtSession);assert.equal(Object.hasOwn(jwtSession,'activeTeamId'),teams);
 assert.equal(sessionOut.status,200);assert.equal(Object.hasOwn(sessionOut.data.session,'activeTeamId'),teams);
 assert.equal(invite.status,teams?400:200);if(teams)assert.equal(invite.data.code,'TEAM_NOT_FOUND');else {assert.equal(Object.hasOwn(invite.data,'teamId'),false);assert.equal(Object.hasOwn(ref.database.invitation.find(r=>r.id===invite.data.id),'teamId'),false);assert.deepEqual(delivery,{teamPresent:false,teamId:undefined});}
 let add;try{const r=await auth.api.addMember({body:{organizationId:org,userId:uid,role:'member',teamId:'nonexistent-team'},asResponse:true});const data=await r.json();add={status:r.status,code:data.code,message:data.message};}catch(e){add={thrown:true,message:e.message};}
 assert.equal(add.status,400);if(teams)assert.equal(add.code,'USER_IS_ALREADY_A_MEMBER_OF_THIS_ORGANIZATION');else assert.equal(add.message,'Teams are not enabled');
 ref.database.team.push({id:'legacy-team',organizationId:org,name:'Legacy',memberCount:0,createdAt:new Date()});
 ref.database.invitation.push({id:'legacy-invite',organizationId:org,email:ref.database.user[0].email,role:'member',teamId:'legacy-team',inviterId:uid,status:'pending',expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});
 const getInvite=await call(auth,'/organization/get-invitation?id=legacy-invite',null,cookie);
 const accept=await call(auth,'/organization/accept-invitation',{invitationId:'legacy-invite'},cookie);
 assert.equal(getInvite.status,200);assert.equal(Object.hasOwn(getInvite.data,'teamId'),teams);
 assert.equal(accept.status,200);assert.equal(ref.database.teamMember.filter(r=>r.teamId==='legacy-team').length,teams?1:0);assert.equal(quotaCalls,teams?1:0);
 assert.equal(ref.database.invitation.find(r=>r.id==='legacy-invite').teamId,'legacy-team');
 ref.database.organizationRole.push({id:'stored-custom-role',organizationId:org,role:'custom',permission:'{}',createdAt:new Date()});
 const customRole=await call(auth,'/organization/invite-member',{organizationId:org,email:'custom@example.test',role:'custom'},cookie);
 assert.equal(customRole.status,dynamic?200:400);if(!dynamic)assert.equal(customRole.data.message,'ROLE_NOT_FOUND: custom');
 ref.database.user.push({id:'removed-user',name:'Removed',email:'removed@example.test',emailVerified:true,createdAt:new Date(),updatedAt:new Date()});
 ref.database.member.push({id:'removed-member',organizationId:org,userId:'removed-user',role:'member',createdAt:new Date()});
 ref.database.teamMember.push({id:'removed-pair',teamId:'legacy-team',userId:'removed-user',createdAt:new Date()});
 const removed=await call(auth,'/organization/remove-member',{organizationId:org,memberIdOrEmail:'removed-member'},cookie);
 assert.equal(removed.status,200);assert.equal(ref.database.teamMember.some(r=>r.id==='removed-pair'),!teams);
 assert.equal(ref.database.member.some(r=>r.id==='removed-member'),false);
 if(!teams){
  const restored=betterAuth({...ref.config,secret,logger:{disabled:true},rateLimit:{enabled:false}});
  const restoredSession=await call(restored,'/get-session',null,cookie);
  assert.equal(restoredSession.status,200);assert.equal(restoredSession.data.session.activeTeamId,'legacy-team');
 }

}
console.log('PASS organization feature topology: 4 configurations, 168 route/method probes, route/schema/default-team/dynamic-grant/session/invitation/server-add/removal/reenable assertions');
