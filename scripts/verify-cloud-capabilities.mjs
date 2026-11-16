import assert from "node:assert/strict";
import {createServer} from "node:http";
import {CMPClient,UnsupportedCMPCapability} from "../packages/sdk/dist/index.js";

const requests=[];
const capabilities={websocket:false,agentCoordination:false,sessionArchive:false,connect:false};
const server=createServer(async(req,res)=>{
 let body="";
 for await(const chunk of req)body+=String(chunk);
 requests.push({path:req.url,body});
 res.setHeader("content-type","application/json");
 if(req.url==="/v1/capabilities")
  return res.end(JSON.stringify({protocolVersion:1,implementation:"test-mock",capabilities}));
 if(req.url==="/v1/rpc")
  return res.end(JSON.stringify({result:{acknowledged:true}}));
 res.statusCode=404;res.end(JSON.stringify({error:"not supported"}));
});
await new Promise(ok=>server.listen(0,"127.0.0.1",ok));
const base="http://127.0.0.1:"+server.address().port;
const cmp=new CMPClient({httpEndpoint:base,accessToken:"fixture-token"});
try{
 for(const [method,feature] of [
  ["agent.join","agentCoordination"],
  ["coordination.message","agentCoordination"],
  ["session.open","sessionArchive"],["sources.list","connect"]]){
  await assert.rejects(cmp.call(method,{}),
   e=>e instanceof UnsupportedCMPCapability&&e.capability===feature);
 }
 assert.ok(requests.every(r=>r.path==="/v1/capabilities"));
 const enabled=await cmp.getCapabilities();
 assert.equal(enabled.capabilities.connect,false);
 capabilities.agentCoordination=true;
 const result=await cmp.call("agent.list",{sessionId:"fixture"});
 assert.equal(result.acknowledged,true);
 assert.equal(requests.at(-1).path,"/v1/rpc");
 console.log("PASS optional Cloud client methods are capability-gated without Cloud backend code");
}finally{cmp.disconnect();await new Promise(ok=>server.close(ok));}
