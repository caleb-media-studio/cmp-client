import assert from "node:assert/strict";
import {resolve} from "node:path";
import {pathToFileURL} from "node:url";
import {CMPClient,UnsupportedCMPCapability} from "../packages/sdk/dist/index.js";

// Separate CMP development chats use isolated worktrees. The caller may
// point to a previously built Core server without modifying that checkout.
// Preserve the documented sibling checkout default for older workflows.
const coreModule = process.env.CMP_CORE_SERVER_MODULE
  ? pathToFileURL(resolve(process.env.CMP_CORE_SERVER_MODULE)).href
  : new URL("../../cmp-server/dist/server.js",import.meta.url).href;
const {createCMPServer}=await import(coreModule);

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
 assert.equal(capabilities.repositoryIdentifier,"owner/name");
 assert.equal(capabilities.repositoryNamespace,"local");
 const created=await client.createRepository("local/core-test");
 assert.ok(created.head);
 assert.equal(created.id,"local/core-test");
 assert.deepEqual((await client.listRepositories()).repositories,["local/core-test"]);
 await assert.rejects(client.createRepository("other/forbidden"));
 const updated=await client.commit("local/core-test",{
  branch:"main",expectedHead:created.head,authorId:"agent",
  message:"Core record",changes:[{op:"put",record:{
   id:"record1",kind:"fact",content:"Persistent knowledge",
   sourceRefs:["manual:fixture"],status:"approved",authorId:"agent",
   updatedAt:"2026-11-16T12:00:00Z"
  }}]
 });
 assert.ok(updated.id);
 const records=await client.getSnapshot("local/core-test");
 assert.equal(records.records.length,1);
 const branch=await client.createBranch("local/core-test","feature");
 assert.equal(branch.head,updated.id);
 const manifest=await client.cmpManifest("local/core-test");
 assert.equal(manifest.repository,"local/core-test");
 assert.equal(manifest.head,updated.id);
 assert.equal((await client.cmpRefs("local/core-test")).branches.feature,updated.id);
 assert.equal((await client.cmpSnapshot("local/core-test")).records.length,1);
 const diff=await client.diff("local/core-test",created.head,"main");
 assert.equal(diff.changes.length,1);
 await assert.rejects(client.joinAgent({
  organizationId:"company",repositoryId:"local/core-test",agentId:"example",
  sourceRepository:{provider:"git",repositoryId:"a"}
 }),e=>e instanceof UnsupportedCMPCapability&&e.capability==="agentCoordination");
 await assert.rejects(client.connect(),
  e=>e instanceof UnsupportedCMPCapability&&e.capability==="websocket");
 console.log("PASS CMP public SDK Core HTTP conformance and Cloud-capability rejection");
}finally{
 client.disconnect();
 await server.close();
}
