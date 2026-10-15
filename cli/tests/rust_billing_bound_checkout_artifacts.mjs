// Compile the actual generated billing library template and export both profiles.
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,readFileSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const temporary=mkdtempSync(path.join(tmpdir(),'fl-billing-bound-checkout-artifacts-'));
const target=process.env.CARGO_TARGET_DIR??path.join(root,'blueprint-rs/target');
const template=readFileSync(path.join(root,'cli/src/templates/rust/billing/lib.rs'),'utf8');
function run(command,args,env={}) {
 const result=spawnSync(command,args,{cwd:temporary,env:{...Object.fromEntries(Object.entries(process.env).filter(([key])=>!['BILLING_BOUND_CHECKOUT_CONFIG','BILLING_BOUND_BILLING_CONFIG','BILLING_PUBLISHER_DATABASE_URL'].includes(key))),CARGO_TARGET_DIR:target,CARGO_INCREMENTAL:'0',...env},encoding:'utf8',timeout:300000,maxBuffer:16*1024*1024});
 assert.equal(result.status,0,result.stderr);return result.stdout;
}
try {
 mkdirSync(path.join(temporary,'src'));
 const crates={
  'forklaunch-contract':'framework-rs/crates/forklaunch-contract',
  'forklaunch-core':'framework-rs/crates/forklaunch-core',
  'forklaunch-openapi':'framework-rs/crates/forklaunch-openapi',
  'forklaunch-blueprint-core':'blueprint-rs/crates/forklaunch-blueprint-core',
  'forklaunch-billing-interface':'blueprint-rs/crates/interfaces/forklaunch-billing-interface',
  'forklaunch-billing-axum':'blueprint-rs/crates/implementations/billing/forklaunch-billing-axum'
 };
 const dependencies=Object.entries(crates).map(([name,relative])=>`${name} = { path = ${JSON.stringify(path.join(root,relative))}${name==='forklaunch-billing-axum'?', features = ["bound-checkout"]':''} }`).join('\n');
 const patches=readFileSync(path.join(root,'blueprint-rs/Cargo.toml'),'utf8').split('[patch.crates-io]')[1].replaceAll('path = "../framework-rs/',`path = "${root}/framework-rs/`);
 writeFileSync(path.join(temporary,'Cargo.toml'),`[package]\nname="fl-billing-bound-checkout-artifacts"\nversion="0.1.0"\nedition="2024"\n[dependencies]\nserde_json="1"\n${dependencies}\n[patch.crates-io]${patches}`);
 writeFileSync(path.join(temporary,'src/routes.rs'),'pub fn contracts() -> Vec<forklaunch_contract::RouteContract> { vec![] }');
 writeFileSync(path.join(temporary,'src/main.rs'),`fn main() {
  let contract=std::path::PathBuf::from(std::env::var("ARTIFACT_DIR").unwrap()).join("contract.json");
  let openapi=contract.with_file_name("openapi.json");
  let result=fl_billing_bound_checkout_artifacts::write_contract_artifacts(&contract,&openapi);
  if std::env::var("EXPECT_FAILURE").as_deref()==Ok("1") {
    assert!(result.is_err()); assert!(!contract.exists()); assert!(!openapi.exists());
  } else { result.unwrap(); }
 }`);
 for(const stripe of [false,true]) {
  const rendered=template.replace(/{{#stripe}}([\s\S]*?){{\/stripe}}/g,(_,body)=>stripe?body:'').replace(/{{\^stripe}}([\s\S]*?){{\/stripe}}/g,(_,body)=>stripe?'':body).replaceAll('{{{name}}}','billing-date-artifacts');
  assert.ok(!rendered.includes('{{'));writeFileSync(path.join(temporary,'src/lib.rs'),rendered);
  run('cargo',['build','--quiet','--manifest-path',path.join(temporary,'Cargo.toml')]);
  const configured={deploymentLabel:'owned',accountId:'acct_private',livemode:false,plans:[{localPlanId:'11111111-1111-4111-8111-111111111111',creationOperationId:'22222222-2222-4222-8222-222222222222'}],successUrl:'https://private.example/success',cancelUrl:'https://private.example/cancel'};
  const aggregate={deploymentLabel:'owned',accountId:'acct_private',livemode:false,portal:{configurationId:'bpc_private',returnUrl:'https://private.example/return'}};
  const subscriptionRead={plans:configured.plans,collectionPolicy:'automatic_default_incomplete_v1'};
  const readAggregate={deploymentLabel:'owned',accountId:'acct_private',livemode:false,subscriptionRead};
  const paymentLinkRead={trustedPrices:["price_owned"]};
  const linkReadAggregate={deploymentLabel:"owned",accountId:"acct_private",livemode:false,paymentLinkRead};
  const cases=[
   {name:'payment-link-read-only',profile:'bound-billing-v1',aggregate:linkReadAggregate,fail:!stripe,count:1},
   {name:'all-read-only',profile:'bound-billing-v1',aggregate:{...linkReadAggregate,subscriptionRead,planRead:{trustedProducts:['custom-product']}},fail:!stripe,count:3},
   {name:'payment-link-read-stray-publisher',profile:'bound-billing-v1',aggregate:linkReadAggregate,publisher:'postgres://publisher@localhost/db',fail:true},
   {name:'payment-link-read-mixed',profile:'bound-billing-v1',aggregate:{...aggregate,paymentLinkRead},publisher:'postgres://publisher@localhost/db',fail:!stripe,count:4},
   {name:'payment-link-read-missing-publisher',profile:'bound-billing-v1',aggregate:{...aggregate,paymentLinkRead},fail:true},
   {name:'payment-link-read-null',profile:'bound-billing-v1',aggregate:{...linkReadAggregate,paymentLinkRead:null},fail:true},
   {name:'payment-link-read-unknown',profile:'bound-billing-v1',aggregate:{...linkReadAggregate,paymentLinkRead:{...paymentLinkRead,create:true}},fail:true},
   {name:'payment-link-read-empty',profile:'bound-billing-v1',aggregate:{...linkReadAggregate,paymentLinkRead:{trustedPrices:[]}},fail:true},

   {name:'subscription-read-only',profile:'bound-billing-v1',aggregate:readAggregate,fail:!stripe,count:1},
   {name:'subscription-plan-read',profile:'bound-billing-v1',aggregate:{...readAggregate,planRead:{trustedProducts:['custom-product']}},fail:!stripe,count:2},
   {name:'subscription-read-stray-publisher',profile:'bound-billing-v1',aggregate:readAggregate,publisher:'postgres://publisher@localhost/db',fail:true},
   {name:'subscription-read-mixed',profile:'bound-billing-v1',aggregate:{...aggregate,subscriptionRead},publisher:'postgres://publisher@localhost/db',fail:!stripe,count:4},
   {name:'subscription-read-missing-publisher',profile:'bound-billing-v1',aggregate:{...aggregate,subscriptionRead},fail:true},
   {name:'subscription-read-null',profile:'bound-billing-v1',aggregate:{...readAggregate,subscriptionRead:null},fail:true},
   {name:'subscription-read-unknown',profile:'bound-billing-v1',aggregate:{...readAggregate,subscriptionRead:{...subscriptionRead,create:true}},fail:true},
   {name:'subscription-read-policy',profile:'bound-billing-v1',aggregate:{...readAggregate,subscriptionRead:{...subscriptionRead,collectionPolicy:'send_invoice'}},fail:true},
   {name:'plan-read-only',profile:'bound-billing-v1',aggregate:{deploymentLabel:'owned',accountId:'acct_private',livemode:false,planRead:{trustedProducts:['custom-product']}},fail:!stripe,count:1},
   {name:'plan-read-stray-publisher',profile:'bound-billing-v1',aggregate:{deploymentLabel:'owned',accountId:'acct_private',livemode:false,planRead:{trustedProducts:['custom-product']}},publisher:'postgres://publisher@localhost/db',fail:true},
   {name:'plan-read-mixed',profile:'bound-billing-v1',aggregate:{...aggregate,planRead:{trustedProducts:['custom-product']}},publisher:'postgres://publisher@localhost/db',fail:!stripe,count:4},
   {name:'plan-read-mixed-missing-publisher',profile:'bound-billing-v1',aggregate:{...aggregate,planRead:{trustedProducts:['custom-product']}},fail:true},
   {name:'plan-read-empty',profile:'bound-billing-v1',aggregate:{deploymentLabel:'owned',accountId:'acct_private',livemode:false,planRead:{trustedProducts:[]}},fail:true},
   {name:'plan-read-null',profile:'bound-billing-v1',aggregate:{deploymentLabel:'owned',accountId:'acct_private',livemode:false,planRead:null},fail:true},
   {name:'plan-read-unknown',profile:'bound-billing-v1',aggregate:{deploymentLabel:'owned',accountId:'acct_private',livemode:false,planRead:{trustedProducts:['custom-product'],create:true}},fail:true},
   {name:'aggregate-portal',profile:'bound-billing-v1',aggregate,publisher:'postgres://publisher@localhost/db',fail:!stripe,count:3},
   {name:'aggregate-both',profile:'bound-billing-v1',aggregate:{...aggregate,checkout:{plans:configured.plans,successUrl:configured.successUrl,cancelUrl:configured.cancelUrl}},publisher:'postgres://publisher@localhost/db',fail:!stripe,count:6},
   {name:'aggregate-checkout',profile:'bound-billing-v1',aggregate:{deploymentLabel:aggregate.deploymentLabel,accountId:aggregate.accountId,livemode:false,checkout:{plans:configured.plans,successUrl:configured.successUrl,cancelUrl:configured.cancelUrl}},publisher:'postgres://publisher@localhost/db',fail:!stripe,count:3},
   {name:'aggregate-mixed',profile:'bound-billing-v1',aggregate,config:configured,publisher:'postgres://publisher@localhost/db',fail:true},
   {name:'aggregate-missing-publisher',profile:'bound-billing-v1',aggregate,fail:true},
   {name:'aggregate-missing-config',profile:'bound-billing-v1',publisher:'postgres://publisher@localhost/db',fail:true},
   {name:'aggregate-null',profile:'bound-billing-v1',aggregate:{...aggregate,portal:null},publisher:'postgres://publisher@localhost/db',fail:true},
   {name:'old-mixed',profile:'bound-checkout-v1',aggregate,config:configured,publisher:'postgres://publisher@localhost/db',fail:true},
   {name:'legacy-mixed',profile:'legacy',aggregate,fail:true},
   {name:'legacy',profile:'legacy',config:null,publisher:null,fail:false},
   {name:'bound',profile:'bound-checkout-v1',config:configured,publisher:'postgres://publisher:private-password@localhost/db',fail:!stripe},
   {name:'missing-publisher',profile:'bound-checkout-v1',config:configured,publisher:null,fail:true},
   {name:'missing-config',profile:'bound-checkout-v1',config:null,publisher:'postgres://publisher@localhost/db',fail:true},
   {name:'stray',profile:'legacy',config:configured,publisher:null,fail:true},
   {name:'unknown',profile:'unknown',config:null,publisher:null,fail:true},
   {name:'unknown-config-field',profile:'bound-checkout-v1',config:{...configured,stripeFields:{}},publisher:'postgres://publisher@localhost/db',fail:true}
  ];
  cases.sort((a,b)=>(a.name==='legacy'?-1:b.name==='legacy'?1:0));
  let legacy;
  for(const test of cases){
   const directory=path.join(temporary,`${stripe}-${test.name}`);
   const env={ARTIFACT_DIR:directory,BILLING_REQUEST_DATE_PROFILE:'utc-iso-and-epoch',BILLING_IDENTITY_PROFILE:test.profile,EXPECT_FAILURE:test.fail?'1':'0'};
   if(test.config)env.BILLING_BOUND_CHECKOUT_CONFIG=JSON.stringify(test.config);
   if(test.aggregate)env.BILLING_BOUND_BILLING_CONFIG=JSON.stringify(test.aggregate);
   if(test.publisher)env.BILLING_PUBLISHER_DATABASE_URL=test.publisher;
   run(path.join(target,'debug/fl-billing-bound-checkout-artifacts'),[],env);
   if(test.fail)continue;
   const contract=JSON.parse(readFileSync(path.join(directory,'contract.json'))),openapi=JSON.parse(readFileSync(path.join(directory,'openapi.json')));
   const text=JSON.stringify({contract,openapi});assert(text.includes('x-forklaunch-date-profile'));
   for(const secret of ['acct_private','private-password','private.example'])assert(!text.includes(secret));
   const bound=contract.routes.filter(r=>(r.path.startsWith('/bound-checkout/')||r.path.startsWith('/bound-portal/')||r.path.startsWith('/bound-plans/')||r.path.startsWith('/bound-subscriptions/')||r.path.startsWith('/bound-payment-links/')));
   if(test.profile==='legacy'){assert.equal(bound.length,0);legacy=contract.routes;}
   else {assert.equal(bound.length,test.count??3);assert.deepEqual(contract.routes.filter(r=>!(r.path.startsWith('/bound-checkout/')||r.path.startsWith('/bound-portal/')||r.path.startsWith('/bound-plans/')||r.path.startsWith('/bound-subscriptions/')||r.path.startsWith('/bound-payment-links/'))),legacy);if(['plan-read-only','subscription-read-only','subscription-plan-read','payment-link-read-only','all-read-only'].includes(test.name)){assert(bound.every(r=>['getBoundCreatedPlan','getBoundCreatedSubscription','getBoundCreatedPaymentLink'].includes(r.operationName)));assert(bound.every(r=>!r.responses.some(v=>v.statusCode===202)));if(test.name==='subscription-read-only')assert.equal(bound[0].operationName,'getBoundCreatedSubscription');if(test.name==='payment-link-read-only')assert.equal(bound[0].operationName,'getBoundCreatedPaymentLink');}else assert(bound.some(r=>r.responses.some(v=>v.statusCode===202)));}
  }

 }
 // Compile the real binary template too: library-only metadata cannot prove startup wiring.
 const mainTemplate=readFileSync(path.join(root,'cli/src/templates/rust/billing/main.rs'),'utf8');
 const manifestTemplate=readFileSync(path.join(root,'cli/src/templates/rust/billing/Cargo.toml.template'),'utf8');
 const manifest=manifestTemplate.replaceAll('{{{name}}}','fl-billing-bound-checkout-artifacts').replaceAll('{{{description}}}','Owned template compilation proof')
  .replace('version.workspace = true','version = "0.1.0"').replace('edition.workspace = true','edition = "2024"')
  .replace('license.workspace = true','license = "MIT"').replace('authors.workspace = true','authors = []')
  .replaceAll('../../.forklaunch/',`${root}/`);
 writeFileSync(path.join(temporary,'Cargo.toml'),`${manifest}\n[patch.crates-io]${patches}`);
 writeFileSync(path.join(temporary,'src/routes.rs'),'pub fn contracts() -> Vec<forklaunch_contract::RouteContract> { vec![] } pub fn register_router(router: forklaunch_axum::router::ForkLaunchRouter) -> forklaunch_axum::router::ForkLaunchRouter { router }');
 for(const stripe of [false,true]) {
  const render=text=>text.replace(/{{#stripe}}([\s\S]*?){{\/stripe}}/g,(_,body)=>stripe?body:'').replace(/{{\^stripe}}([\s\S]*?){{\/stripe}}/g,(_,body)=>stripe?'':body).replaceAll('{{{name}}}','billing-checkout-artifacts').replaceAll('{{crate_module}}','fl_billing_bound_checkout_artifacts');
  writeFileSync(path.join(temporary,'src/lib.rs'),render(template));
  writeFileSync(path.join(temporary,'src/main.rs'),render(mainTemplate));
  run('cargo',['check','--quiet','--manifest-path',path.join(temporary,'Cargo.toml')]);
 }
 console.log('PASS actual rendered generated library: 78 base/Stripe identity startup/export cases, shared selected-family registry, unchanged default routes, Date composition, no private config in metadata; actual base/Stripe binary templates compile');
} finally {rmSync(temporary,{recursive:true,force:true});}
