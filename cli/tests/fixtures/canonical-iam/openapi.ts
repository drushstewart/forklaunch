import assert from 'node:assert/strict';
import {generateOpenApiSpecs,OPENAPI_DEFAULT_VERSION} from '@fixture/openapi';
import {SchemaValidator,string} from '@forklaunch/validator/zod';

// Exercise the actual emitter with default/explicit/custom JWT headers and Basic.
for(const auth of [
 {jwt:{signatureKey:'test-only-signing-key'}},
 {jwt:{signatureKey:'test-only-signing-key'},headerName:'Authorization'},
 {jwt:{signatureKey:'test-only-signing-key'},headerName:'X-Api-Key'},
 {basic:{login:()=>true}}
]) {
 const spec=generateOpenApiSpecs(SchemaValidator(),['http://localhost'],['Fixture'],{
  basePath:'/',routes:[{basePath:'/',path:'/:id',method:'get',contractDetails:{
   name:'Get Item',summary:'Get an item',access:'protected',params:{id:string},
   auth:{...auth,allowedPermissions:new Set(['items:read'])},responses:{200:string}
  }}],routers:[],_fetchMap:{},sdk:{},sdkPaths:{}
 })[OPENAPI_DEFAULT_VERSION];
 const operation=Object.values(spec.paths)[0].get;
 assert.equal(operation.parameters[0].required,true);
 const expected='basic' in auth?'basic':auth.headerName??'Authorization';
 assert.deepEqual(operation.security,[{[expected]:[]}]);
 assert.ok(spec.components.securitySchemes[expected]);
 if(expected==='X-Api-Key')assert.deepEqual(spec.components.securitySchemes[expected],{type:'apiKey',in:'header',name:expected});
 else assert.equal(spec.components.securitySchemes[expected].scheme,expected==='basic'?'basic':'bearer');
}
console.log('PASS actual TypeScript OpenAPI required paths and resolvable default/explicit/custom JWT/Basic security schemes');
