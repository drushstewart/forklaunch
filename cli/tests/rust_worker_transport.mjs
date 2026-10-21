import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
const exec=promisify(execFile),[binary,backend]=process.argv.slice(2);
const privateMarker='private-broker-credential-must-not-be-logged';
const environment={...process.env, WORKER_ALLOW_PLAINTEXT_DEVELOPMENT:'false',
  TENANT_ID:'11111111-1111-4111-8111-111111111111',WORKER_QUEUE:'jobs',WORKER_NAMESPACE:'transport-test',
  REDIS_KEY_PREFIX:'transport-test',KAFKA_TRANSACTIONAL_ID:'transport-test',
  DATABASE_URL:`postgres://user:${privateMarker}@remote.invalid/db`,
  REDIS_URL:`redis://user:${privateMarker}@remote.invalid/`,KAFKA_BROKERS:'remote.invalid:9092',
  KAFKA_CLIENT_PROPERTIES:JSON.stringify({'security.protocol':'PLAINTEXT','sasl.password':privateMarker}),
};
const args=backend==='kafka'?['--peek','1']:['--inspect','22222222-2222-4222-8222-222222222222'];
try {
  await exec(binary,args,{env:environment,timeout:3000});
  assert.fail('Remote plaintext broker was accepted');
} catch(error) {
  assert.equal(error.killed,false,'Broker policy should reject before connecting');
  assert.ok(error.code!==0);
  const diagnostic=`${error.stdout??''}${error.stderr??''}`;
  assert.match(diagnostic,/Configuration|worker configuration unavailable/);
  assert.ok(!diagnostic.includes(privateMarker),'Broker credentials leaked');
  assert.ok(!diagnostic.includes('remote.invalid'),'Broker address leaked');
}
console.log(`PASS generated ${backend} remote plaintext broker admission and credential redaction`);
