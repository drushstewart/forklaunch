// Run actual companion payment modules with owned bootstrap seams, never cloud APIs.
import assert from 'node:assert/strict';
import {mkdtemp,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {execFileSync} from 'node:child_process';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const platform=process.env.FORKLAUNCH_PLATFORM_ROOT,deps=process.env.FORKLAUNCH_CONTEXT_TEST_DEPENDENCIES;
assert(platform&&deps,'Set FORKLAUNCH_PLATFORM_ROOT and FORKLAUNCH_CONTEXT_TEST_DEPENDENCIES (TypeScript5.9.3, Vitest3.2.4, Stripe22.6.2, MikroORM7.2.3, @types/node22)');
const work=await mkdtemp(path.join(tmpdir(),'fl-managed-platform-tests-'));
const p=(name)=>path.join(platform,'src/modules/managed-apps',name),d=(name)=>path.join(deps,'node_modules',name);
try {
  await writeFile(path.join(work,'http.ts'),`export * from ${JSON.stringify(path.join(root,'framework/core/src/http/createHmacToken'))};\nexport * from ${JSON.stringify(path.join(root,'framework/core/src/http/generateHmacAuthHeaders'))};\n`);
  await writeFile(path.join(work,'platform.ts'),'export {};\n');
  await writeFile(path.join(work,'setup.ts'),`import {vi} from 'vitest';\nvi.mock(${JSON.stringify(p('persistence/entities/instance-payment-account.entity.ts'))},()=>({InstancePaymentAccountEntity:{}}));\n`);
  const aliases={
    '@forklaunch-platform/core':path.join(work,'platform.ts'),
    '@forklaunch/core/http':path.join(work,'http.ts'),
    '@forklaunch/common':path.join(root,'framework/common/src/safeStringify.ts'),
    '@mikro-orm/postgresql':d('@mikro-orm/postgresql/index.js'),
    '@mikro-orm/migrations':d('@mikro-orm/migrations/index.js'),
    stripe:d('stripe/esm/stripe.esm.node.js'),vitest:d('vitest/dist/index.js')
  };
  await writeFile(path.join(work,'vitest.config.mjs'),'export default '+JSON.stringify({resolve:{alias:aliases},test:{setupFiles:[path.join(work,'setup.ts')],include:['payments-context','payments-gateway','payments-context-controller','payments-binding-generation'].map(name=>p(`__test__/${name}.test.ts`))}}));
  execFileSync(d('.bin/vitest'),['run','--config',path.join(work,'vitest.config.mjs')],{cwd:platform,stdio:'inherit'});
  await writeFile(path.join(work,'tsconfig.json'),JSON.stringify({compilerOptions:{target:'ES2022',module:'ESNext',moduleResolution:'Bundler',strict:true,skipLibCheck:true,noEmit:true,typeRoots:[d('@types')],types:['node'],paths:{stripe:[d('stripe/esm/stripe.esm.node.d.ts')],vitest:[d('vitest/dist/index.d.ts')]}},files:['domain/services/payments-context.ts','domain/services/stripe-connect.adapter.ts','__test__/payments-context.test.ts'].map(p)}));
  execFileSync(d('.bin/tsc'),['-p',path.join(work,'tsconfig.json')],{cwd:platform,stdio:'inherit'});
  console.log('PASS focused strict real-dependency protocol/adapter types; actual controller/service tests use explicit bootstrap/entity seams, not full platform bootstrap');
}finally{await rm(work,{recursive:true,force:true});}
