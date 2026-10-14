import {forklaunchExpress} from '@forklaunch/express';
import {SchemaValidator, string, boolean, array, optional, union, null_, undefined_} from '@forklaunch/validator/zod';
import {z} from 'zod/v3';
import {OpenTelemetryCollector} from '@forklaunch/core/http';

// Match the published sample IAM contracts and deterministic in-memory seed.
const users = [
  {id:'user_1',email:'ada@example.com',display_name:null,disabled:false,roles:[]},
  {id:'user_2',email:'grace@example.com',display_name:null,disabled:false,roles:[]}
];
const user = {id:string,email:string,display_name:optional(union([string,null_])),disabled:boolean,roles:array(string)};
const auth = {jwt:{signatureKey:'test-only-signing-key'}};
const app = forklaunchExpress(SchemaValidator(), new OpenTelemetryCollector('canonical-iam','silent'), {mcp:false,json:{limit:'64b',verify(req){if(req.headers['x-deny']==='yes')throw new Error('PRIVATE_VERIFIER_DETAIL');}}});
app.get('/users/:id', {
  name:'Get User',summary:'Get a user by id',access:'authenticated',auth,
  params:{id:string},responses:{200:user,404:undefined_}
}, (req,res)=>{
  const found=users.find(user=>user.id===req.params.id);
  if(found) res.status(200).json(found); else res.status(404).send();
});
let queryCalls=0;
app.get('/users', {
  name:'List Users',summary:'List users',access:'authenticated',auth,
  query:{limit:optional(z.coerce.number().int()),cursor:optional(string)},
  responses:{200:{items:array(user),next_cursor:optional(union([string,null_]))}}
}, (_req,res)=>{queryCalls++;res.status(200).json({items:users,next_cursor:null});});
app.get('/query/calls',{name:'Query Calls',summary:'Calls',options:{openapi:false},responses:{200:z.number()}},(_req,res)=>res.status(200).json(queryCalls));
app.get('/errors/throw', {
 name:'Failure',summary:'Failure',options:{openapi:false},responses:{500:string}
}, ()=>{throw new Error('secret fixture detail');});
app.get('/errors/validate', {
 name:'Validate',summary:'Validate',options:{openapi:false},
 query:{display_name:z.string().refine(value=>value.trim().length>0,{message:'A display name is required'})},
 responses:{200:string}
}, (_req,res)=>res.status(200).send('accepted'));
let parserCalls=0;
for(const path of ['/parser/axum','/parser/local'])app.post(path,{
 name:'Parser',summary:'Parser',options:{openapi:false},
 body:{json:{task:string}},responses:{200:{accepted:string}}
},(_req,res)=>{parserCalls++;res.status(200).json({accepted:'yes'});});
app.get('/parser/calls',{name:'Calls',summary:'Calls',options:{openapi:false},responses:{200:z.number()}},(_req,res)=>res.status(200).json(parserCalls));
const server=app.listen(0,'127.0.0.1');
server.on('listening',()=>console.log(`READY http://127.0.0.1:${server.address().port}`));
process.on('SIGTERM',()=>server.close(()=>process.exit(0)));
