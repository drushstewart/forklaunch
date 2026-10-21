import assert from 'node:assert/strict';
import {randomBytes} from 'node:crypto';
const recorder=`http://localhost:${process.env.FIELDNOTES_TELEMETRY_PORT}`;
const origin=process.env.PUBLIC_ORIGIN;
const marker=`private-telemetry-canary-${randomBytes(16).toString('hex')}`;
const traceId=randomBytes(16).toString('hex');
const sign=await fetch('http://localhost:3001/api/auth/sign-in/email',{method:'POST',headers:{origin,'content-type':'application/json'},body:JSON.stringify({email:'alice@example.test',password:process.env.DEMO_PASSWORD})});
assert.equal(sign.status,200,'telemetry fixture sign-in');
const cookie=sign.headers.getSetCookie().map(value=>value.split(';')[0]).join('; ');
await fetch(recorder+'/expect',{method:'POST',body:JSON.stringify({traceId,secrets:[marker,cookie,...cookie.split('; ').map(value=>value.slice(value.indexOf('=')+1)),process.env.DEMO_PASSWORD,process.env.INTERNAL_AUTH_SECRET,process.env.DATABASE_URL]})});
const prefix='http://notes:8000/v1/workspaces/11111111-1111-4111-8111-111111111111/notes';
const headers={cookie,origin,'content-type':'application/json'};
let id;
try {
  const response=await fetch(prefix+'?private='+marker,{method:'POST',headers:{...headers,traceparent:`00-${traceId}-0123456789abcdef-01`,'x-private-test':marker},body:JSON.stringify({title:marker,body:marker})});
  assert.equal(response.status,201,'telemetry fixture note create');
  id=(await response.json()).id;
  // Ensure errors are measured without exporting raw URL/header/body values.
  await fetch('http://notes:8000/unknown-'+marker,{headers:{cookie,origin}});
  let completed=false;
  for(let i=0;i<120;i++) {
    const response=await fetch(prefix+'/'+id,{headers});assert.equal(response.status,200);
    const note=await response.json();
    if(note.insightExcerpt!=null){completed=true;break;}
    await new Promise(resolve=>setTimeout(resolve,500));
  }
  assert.ok(completed,'real encrypted worker result must arrive');
  for(let i=0;i<60;i++) {
    const receipt=await(await fetch(recorder+'/receipt')).json();
    assert.equal(receipt.payloadLeak,false,'sensitive data exported');
    assert.equal(receipt.overflow,false,'bounded recorder overflow');
    assert.ok(Object.values(receipt.services).every(s=>!s.unsafeMetricDimensions),'unbounded identifier metric dimension');
    const services=Object.values(receipt.services);
    const http=services.some(s=>s.metrics.includes('http.server.request.count')&&s.signals.resourceSpans&&s.signals.resourceLogs);
    const worker=services.some(s=>s.metrics.includes('forklaunch.worker.attempts')&&s.signals.resourceSpans);
    if(receipt.correlated&&http&&worker){console.log(JSON.stringify(receipt,null,2));process.exitCode=0;break;}
    assert.ok(i<59,'HTTP-to-worker trace correlation and OTLP signals missing');
    await new Promise(resolve=>setTimeout(resolve,500));
  }
} finally {
  if(id)assert.equal((await fetch(prefix+'/'+id,{method:'DELETE',headers})).status,204,'telemetry fixture cleanup');
}
