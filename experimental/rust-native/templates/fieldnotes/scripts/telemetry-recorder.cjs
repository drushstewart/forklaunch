// Disposable in-memory OTLP/HTTP JSON receiver for the local acceptance proof.
const http = require('node:http');
const fs = require('node:fs');
const receipt = {protocol:'http/json', endpoints:{}, services:{}, payloadLeak:false, overflow:false, correlated:false};
let samples=[], total=0, secrets=[], traceId='';
const spans=[];
function inspect(text) { receipt.payloadLeak ||= secrets.some(value=>text.includes(value)); }
const server=http.createServer((req,res)=>{
  const respond=(value,status=200)=>{res.statusCode=status;res.setHeader('content-type','application/json');res.end(JSON.stringify(value));};
  if(req.url==='/receipt') {
    const parents=spans.filter(s=>s.traceId===traceId && s.name.startsWith('POST ')).map(s=>s.spanId);
    receipt.correlated=spans.some(s=>s.traceId===traceId && s.name==='worker.process' && parents.includes(s.parentSpanId));
    respond(receipt);return;
  }
  if(req.url==='/shutdown'){respond({});server.close();return;}
  let chunks=[],size=0;
  req.on('data',chunk=>{size+=chunk.length;if(size>4*1024*1024){receipt.overflow=true;req.destroy();return;}chunks.push(chunk);});
  req.on('end',()=>{
    try {
      const text=Buffer.concat(chunks).toString('utf8');const data=JSON.parse(text);
      if(req.url==='/expect') {
        secrets=data.secrets.filter(s=>typeof s==='string'&&s.length>7);traceId=data.traceId;
        samples.forEach(inspect);samples=[];respond({});return;
      }
      if(!['/v1/traces','/v1/metrics','/v1/logs'].includes(req.url)){respond({},404);return;}
      total+=size;if(total>32*1024*1024){receipt.overflow=true;respond({},413);return;}
      inspect(text);if(!secrets.length)samples.push(text);
      receipt.endpoints[req.url]=(receipt.endpoints[req.url]??0)+1;
      for(const signal of ['resourceSpans','resourceMetrics','resourceLogs'])for(const resource of data[signal]??[]) {
        const name=resource.resource?.attributes?.find(a=>a.key==='service.name')?.value?.stringValue??'unknown';
        const service=receipt.services[name]??={signals:{},metrics:[],unsafeMetricDimensions:false};
        service.signals[signal]=(service.signals[signal]??0)+1;
        for(const scope of resource.scopeSpans??[])for(const s of scope.spans??[])spans.push({name:s.name,traceId:s.traceId,spanId:s.spanId,parentSpanId:s.parentSpanId});
        for(const scope of resource.scopeMetrics??[])for(const metric of scope.metrics??[]) {
          if(!service.metrics.includes(metric.name))service.metrics.push(metric.name);
          for(const point of (metric.sum??metric.histogram??metric.gauge)?.dataPoints??[]) {
            service.unsafeMetricDimensions ||= (point.attributes??[]).some(a=>/tenant|workspace|resource|operation|correlation|user|url|target/i.test(a.key));
          }
        }
      }
      respond({});
    } catch {respond({},400);}
  });
});
server.listen(0,'0.0.0.0',()=>fs.writeFileSync(process.argv[2],String(server.address().port)));
