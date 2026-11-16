import assert from "node:assert/strict";
import {createCMPServer} from "../../cmp-server/dist/server.js";
import {CMPClient,UnsupportedCMPCapability} from "../packages/sdk/dist/index.js";

process.env.CMP_ENV="development";
const token="local-core-test-token-1234567890123456";
const server=createCMPServer({token,dbFile:":memory:"});
const address=await server.listen();
const client=new CMPClient({
 httpEndpoint:"http://127.0.0.1:"+address.port,accessToken:token
});
try{
 const capabilities=await client.getCapabilities();
 assert.equal(capabilities.capabilities.websocket,false);
 assert.equal(capabilities.capabilities.agentCoordination,false);
 assert.equal(capabilities.capabilities.sessionArchive,false);
 const created=await client.createRepository("core-test");
 assert.ok(created.head);
 const updated=await client.commit("core-test",{
  branch:"main",expectedHead:created.head,authorId:"agent",
  message:"Core record",changes:[{op:"put",record:{
   id:"record1",kind:"fact",content:"Persistent knowledge",
   sourceRefs:["manual:fixture"],status:"approved",authorId:"agent",
   updatedAt:"2026-11-16T12:00:00Z"
  }}]
 });
 assert.ok(updated.id);
 const records=await client.getSnapshot("core-test");
 assert.equal(records.records.length,1);
 const branch=await client.createBranch("core-test","feature");
 assert.equal(branch.head,updated.id);
 const diff=await client.diff("core-test",created.head,"main");
 assert.equal(diff.changes.length,1);
 await assert.rejects(client.joinAgent({
  organizationId:"company",repositoryId:"core-test",agentId:"example",
  sourceRepository:{provider:"git",repositoryId:"a"}
 }),e=>e instanceof UnsupportedCMPCapability&&e.capability==="agentCoordination");
 await assert.rejects(client.connect(),
  e=>e instanceof UnsupportedCMPCapability&&e.capability==="websocket");
 console.log("PASS CMP public SDK Core HTTP conformance and Cloud-capability rejection");
}finally{
 client.disconnect();
 await server.close();
}
