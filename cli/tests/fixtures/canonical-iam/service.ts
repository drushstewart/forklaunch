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
const app = forklaunchExpress(SchemaValidator(), new OpenTelemetryCollector('canonical-iam','silent'), {mcp:false});
app.get('/users/:id', {
  name:'Get User',summary:'Get a user by id',access:'authenticated',auth,
  params:{id:string},responses:{200:user,404:undefined_}
}, (req,res)=>{
  const found=users.find(user=>user.id===req.params.id);
  if(found) res.status(200).json(found); else res.status(404).send();
});
app.get('/users', {
  name:'List Users',summary:'List users',access:'authenticated',auth,
  query:{limit:optional(z.coerce.number().int()),cursor:optional(string)},
  responses:{200:{items:array(user),next_cursor:optional(union([string,null_]))}}
}, (_req,res)=>res.status(200).json({items:users,next_cursor:null}));
const server=app.listen(0,'127.0.0.1');
server.on('listening',()=>console.log(`READY http://127.0.0.1:${server.address().port}`));
process.on('SIGTERM',()=>server.close(()=>process.exit(0)));
