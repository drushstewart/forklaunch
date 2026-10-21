import {createForkLaunchSdk} from './fields.js';
let calls=0;
const fetcher:typeof fetch=async()=>{calls++;return new Response('ok',{headers:{'content-type':'text/plain'}});};
async function rejects(f:()=>Promise<unknown>){const before=calls;try{await f();}catch{if(calls!==before)throw Error('invalid request was sent');return;}throw Error('invalid request accepted');}
const sdk=createForkLaunchSdk({baseUrl:'http://localhost',fetch:fetcher});
await sdk.fields.range.range({query:{start:'a',end:'b'},headers:{'X-Start':'a','X-End':'b'}});
await rejects(()=>sdk.fields.range.range({query:{start:'a'},headers:{}}));
await rejects(()=>sdk.fields.range.range({query:{},headers:{'X-Start':'a'}}));
await rejects(()=>sdk.fields.range.range({query:JSON.parse('{"__proto__":"bad"}'),headers:{}}));
const hooked=createForkLaunchSdk({baseUrl:'http://localhost',fetch:fetcher,beforeRequest:meta=>({...meta,requestObjects:{},input:{query:{start:'a'},headers:{}}})});
await rejects(()=>hooked.fields.range.range({query:{start:'a',end:'b'},headers:{}}));
const injected=createForkLaunchSdk({baseUrl:'http://localhost',fetch:fetcher,beforeRequest:meta=>({...meta,path:'/range?start=a'})});
await rejects(()=>injected.fields.range.range({query:{},headers:{}}));
if(calls!==1)throw Error('unexpected request count');
console.log('PASS actual TS query/header groups, case-insensitive transport, unknown keys and post-hook validation');
const {createForkLaunchSdk:createVersions}=await import('./versions.js');
let response='second';
const versioned=createVersions({baseUrl:'http://localhost',fetch:async()=>new Response(response,{headers:{'content-type':'text/plain'}})});
await versioned.fields.ranges.rangesVersionFirst({query:{start:'a'}});
response='first';
let rejected=false;
try{await versioned.fields.ranges.rangesVersionFirst({query:{start:'a'}});}catch{rejected=true;}
if(!rejected)throw Error('response from nonmatching group accepted');
console.log('PASS actual TS branch-specific request groups select valid response contracts');
