// Execute the actual generated relay service over an owned real ORM schema.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { betterAuthOrm } from './fixtures/rust_better_auth_orm.mjs';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const source = await betterAuthOrm(root, process.env.FORKLAUNCH_TEST_ORM_DEPENDENCIES,
  (specifier, file, load) => {
    if(file.endsWith('/relay/domain/services/relaySession.service.ts') && specifier === '../../persistence/entities/session.entity')
      return load(path.join(root,'blueprint/iam-better-auth/persistence/entities/session.entity.ts'));
  }, true);
const { RelaySessionHandoff } = source.load('cli/src/templates/project/relay/persistence/entities/relaySessionHandoff.entity.ts');
const { RelaySessionService } = source.load('cli/src/templates/project/relay/domain/services/relaySession.service.ts');
const schema = `fl_relay_${crypto.randomBytes(8).toString('hex')}`;
const orm = await source.MikroORM.init({entities:[...Object.values(source.entities),RelaySessionHandoff],clientUrl:process.env.FORKLAUNCH_TEST_POSTGRES_URL,schema});
const logger = {info(){},error(){}};
const cookies = async()=>({secret:'relay-fixture-secret',sessionTokenName:'better-auth.session_token',sessionTokenAttributes:{httpOnly:true,path:'/',sameSite:'lax'}});
const service = em => new RelaySessionService(em,cookies,logger);
try {
  await orm.schema.create();
  const { sanitizeRedirectPath } = source.load('cli/src/templates/project/relay/domain/utils/redirect-path.util.ts');
  for(const input of [null,undefined,'','https://foreign.test','//foreign.test','/\\foreign.test','/\t/foreign.test','/\n/foreign.test','/\r/foreign.test']) {
    const safe = sanitizeRedirectPath(input);
    assert.equal(safe,'/',JSON.stringify(input));
    assert.equal(new URL(safe,'https://fixture.test').origin,'https://fixture.test');
  }
  for(const input of ['/','/dashboard?q=1#part','/path with spaces','/%2F%2Fhost']) assert.equal(sanitizeRedirectPath(input),input);
  const result = await service(orm.em.fork()).ingest({nonce:'relay-once',tokens:{access_token:'owned-token'}});
  const ticket = new URL(result.redirectPath,'https://fixture.test').searchParams.get('ticket');
  assert.ok(ticket);
  await assert.rejects(()=>service(orm.em.fork()).ingest({nonce:'relay-once',tokens:{}}), /relay nonce already consumed/);
  // Synchronize the reads in two independent request managers. A subsequent
  // atomic claim must still allow exactly one browser to redeem the ticket.
  let reads = 0, release;
  const ready = new Promise(resolve=>{release=resolve;});
  function racingManager() {
    const em = orm.em.fork();
    return new Proxy(em,{get(target,key){
      if(key==='findOne') return async(...args)=>{
        const row = await target.findOne(...args);
        if(args[0]===RelaySessionHandoff) { if(++reads===2) release(); await ready; }
        return row;
      };
      const value = Reflect.get(target,key,target);
      return typeof value==='function' ? value.bind(target) : value;
    }});
  }
  const redeemed = await Promise.all([service(racingManager()).redeemTicket(ticket),service(racingManager()).redeemTicket(ticket)]);
  assert.equal(redeemed.filter(Boolean).length,1,'handoff ticket must be redeemed exactly once across concurrent requests');
  assert.deepEqual(redeemed.find(Boolean),{cookie:null,redirectTo:'/'});
  assert.equal(await service(orm.em.fork()).redeemTicket(ticket),null);
  assert.equal(await service(orm.em.fork()).redeemTicket(crypto.randomUUID()),null);
  const em = orm.em.fork();
  const expired = em.create(RelaySessionHandoff,{nonce:'expired',ownerUserId:null,activeOrganizationId:null,redirectTo:'/safe',expiresAt:new Date(0),consumedAt:null});
  em.persist(expired);await em.flush();
  assert.equal(await service(orm.em.fork()).redeemTicket(expired.id),null);
  console.log('PASS actual relay ORM: nonce uniqueness, synchronized concurrent one-time redemption, replay, unknown and expired tickets');
} finally {
  try { await orm.schema.drop({dropMigrationsTable:true,dropDb:false}); }
  finally { await orm.close(true); }
}
