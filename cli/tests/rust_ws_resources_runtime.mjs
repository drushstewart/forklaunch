// Resource-isolated unions through portable generation and both live clients.
import assert from 'node:assert/strict';
import {execFileSync, spawn} from 'node:child_process';
import {once} from 'node:events';
import {mkdtemp, mkdir, writeFile, symlink, rm} from 'node:fs/promises';
import {createRequire} from 'node:module';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath, pathToFileURL} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const framework=path.join(root,'framework-rs');
const deps=createRequire(path.join(process.env.FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES,'package.json'));
const {WebSocketServer}=deps('ws');
const temporary=await mkdtemp(path.join(tmpdir(),'fl-ws-resources-'));
const env={...process.env,CARGO_INCREMENTAL:'0',CARGO_TARGET_DIR:path.resolve(root,process.env.CARGO_TARGET_DIR??'framework-rs/target')};
const modes=['plain','legacy','dynamic','recursive'];
const sockets=new Set();
let server,child;
try {
 await mkdir(path.join(temporary,'src'));
 await symlink(path.join(path.dirname(deps.resolve('ws/package.json')),'..'),path.join(temporary,'node_modules'),'dir');
 await writeFile(path.join(temporary,'package.json'),JSON.stringify({type:'module'}));
 const artifact={formatVersion:1,title:'Resource sockets',version:'1',sockets:{}};
 for(const mode of modes) {
  const entries={};
  for(const [name,type] of [['text','string'],['count','integer']]) {
   const schema={$schema:mode==='legacy'?'http://json-schema.org/draft-04/schema#':mode==='recursive'?'https://json-schema.org/draft/2019-09/schema':mode==='dynamic'?'https://json-schema.org/draft/2020-12/schema':'http://json-schema.org/draft-07/schema#',
    [mode==='legacy'?'id':'$id']:'https://schemas.fixture.test/reused',type:'object',additionalProperties:false,required:['value'],properties:{value:{type,...(type==='string'?{minLength:1}:{minimum:1})},next:{$ref:'#'}}};
   if(mode==='dynamic') {schema.$dynamicAnchor='node';schema.properties.next={$dynamicRef:'#node'};}
   if(mode==='recursive') {schema.$recursiveAnchor=true;schema.properties.next={$recursiveRef:'#'};}
   entries[name]={shape:schema,jsonEncoding:true,channel:`/${mode}`};
  }
  artifact.sockets[mode]={path:`/${mode}`,clientMessages:entries,serverMessages:entries};
 }
 const input=path.join(temporary,'contract.json');
 await writeFile(input,JSON.stringify(artifact));
 const generated=JSON.parse(execFileSync('cargo',['run','--quiet','--manifest-path',path.join(framework,'Cargo.toml'),'-p','forklaunch-sdk-typescript','--example','socket_artifacts','--',input,'all'],{cwd:root,env,encoding:'utf8',maxBuffer:16*1024*1024,timeout:300000}));
 const Ajv=deps('ajv').default;
 const specValidator=new Ajv({strict:false,allErrors:true,ownProperties:true});
 deps('ajv-formats')(specValidator);
 const validateDocument=specValidator.compile(deps('@asyncapi/specs/schemas/3.0.0-without-$id.json'));
 assert.ok(validateDocument(generated.asyncapi),JSON.stringify(validateDocument.errors));
 let rustModules='',rustChecks='',tsTypes='';
 for(const mode of modes) {
  await writeFile(path.join(temporary,`src/${mode}.ts`),generated.typescript[`socket_${mode}`]);
  await writeFile(path.join(temporary,`src/${mode}.rs`),generated.rust[`socket_${mode}`]);
  rustModules+=`mod ${mode};\n`;
  rustChecks+=`{
let (mut client,_)= ${mode}::SocketClient::connect(format!("{}/${mode}",url),SocketOptions::default()).await.unwrap();
for value in [serde_json::json!({"value":"a","next":{"value":"b"}}),serde_json::json!({"value":1,"next":{"value":2}})] {
let outgoing: ${mode}::SocketOutgoing=serde_json::from_value(value.clone()).unwrap();
client.send(&outgoing).await.unwrap();
match client.recv().await.unwrap().unwrap() {SocketEvent::Message(received)=>assert_eq!(serde_json::to_value(received).unwrap(),value),_=>panic!("expected message")}
}
let invalid: ${mode}::SocketOutgoing=serde_json::from_value(serde_json::json!({"value":""})).unwrap();
assert!(client.send(&invalid).await.is_err());
client.close(1000,None).await.unwrap();
}\n`;
  tsTypes+=`import type {SocketOutgoing as ${mode}} from './${mode}.js';\nexport const ${mode}Valid: ${mode}={value:'a',next:{value:'b'}};\n// @ts-expect-error Every recursive variant constrains its value type.\nexport const ${mode}Invalid: ${mode}={value:false};\n`;
 }
 await writeFile(path.join(temporary,'src/types.ts'),tsTypes);
 await writeFile(path.join(temporary,'tsconfig.json'),JSON.stringify({compilerOptions:{strict:true,target:'ES2022',module:'NodeNext',moduleResolution:'NodeNext',esModuleInterop:true,types:['node'],outDir:'dist',rootDir:'src',skipLibCheck:false},include:['src/*.ts']}));
 execFileSync(process.env.TSC_BIN??deps.resolve('typescript/bin/tsc'),['--project',path.join(temporary,'tsconfig.json')],{stdio:'inherit',timeout:60000});
 await writeFile(path.join(temporary,'Cargo.toml'),`[package]\nname="forklaunch-resource-socket-fixture"\nversion="0.0.0"\nedition="2021"\n[workspace]\n[dependencies]\nforklaunch-ws={path=${JSON.stringify(path.join(framework,'crates/forklaunch-ws'))},features=["transport","json-schema"]}\nserde={version="1",features=["derive"]}\nserde_json="1"\ntokio={version="1",features=["net","macros","rt-multi-thread"]}\ntokio-tungstenite="0.30"\n`);
 await writeFile(path.join(temporary,'src/main.rs'),`#![allow(dead_code)]\n${rustModules}use forklaunch_ws::transport::{SocketEvent,SocketOptions};\n#[tokio::main] async fn main(){let url=std::env::args().nth(1).unwrap();${rustChecks}println!("PASS Rust scoped socket clients");}`);
 execFileSync('cargo',['build','--quiet','--manifest-path',path.join(temporary,'Cargo.toml')],{cwd:root,env,stdio:'inherit',timeout:300000});
 server=new WebSocketServer({host:'127.0.0.1',port:0});await once(server,'listening');
 let messages=0;
 server.on('connection',socket=>{sockets.add(socket);socket.on('close',()=>sockets.delete(socket));socket.on('message',(bytes,binary)=>{messages++;socket.send(bytes,{binary});});});
 const url=`ws://127.0.0.1:${server.address().port}`;
 for(const mode of modes) {
  const {SocketClient}=await import(pathToFileURL(path.join(temporary,`dist/${mode}.js`)));
  const client=await SocketClient.connect(`${url}/${mode}`);
  for(const value of [{value:'a',next:{value:'b'}},{value:1,next:{value:2}}]) {
   await client.send(value);const event=await client.recv({timeoutMs:5000});assert.equal(event.type,'message');assert.deepEqual(event.value,value);
  }
  for(const value of [{value:''},{value:'a',next:{value:2}},{value:1,next:{value:'a'}}]) await assert.rejects(client.send(value));
  await client.close(1000);
 }
 child=spawn(path.join(env.CARGO_TARGET_DIR,'debug/forklaunch-resource-socket-fixture'),[url],{stdio:'inherit'});
 const timer=setTimeout(()=>child.kill('SIGKILL'),20000);
 try {const [code]=await once(child,'exit');assert.equal(code,0);} finally {clearTimeout(timer);}
 assert.equal(messages,16);
 console.log('PASS duplicate resource IDs, static/legacy/dynamic/recursive unions, both typed SDKs and 16 live socket roundtrips');
} finally {
 if(child?.exitCode===null) child.kill('SIGKILL');
 for(const socket of sockets) socket.terminate();
 if(server) await new Promise(resolve=>server.close(resolve));
 await rm(temporary,{recursive:true,force:true});
}
