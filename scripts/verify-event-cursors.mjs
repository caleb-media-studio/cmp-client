import assert from "node:assert/strict";
import { CMPClient } from "../packages/sdk/dist/index.js";

const client=new CMPClient({httpEndpoint:"http://127.0.0.1:8787",accessToken:"dev-fixture"});
const delivered=[];
client.onEvent(event=>delivered.push([event.method,event.params,event.cursor]));
const emit=(method,params,cursor)=>client.receive(JSON.stringify({
 v:1,id:"evt-"+cursor,type:"event",method,eventId:"evt-"+cursor,cursor,params
}));
emit("commit.created",{repo:"repo-one"},"10");
emit("commit.created",{repo:"repo-two"},"3");
emit("commit.created",{repo:"repo-one"},"10"); // duplicate cursor
emit("commit.created",{repo:"repo-one"},"7"); // out of order
emit("agent.peer_detected",{organizationId:"org-a",repositoryId:"repo-one"},"2");
emit("agent.peer_detected",{organizationId:"org-a",repositoryId:"repo-two"},"1");
emit("agent.peer_detected",{organizationId:"org-b",repositoryId:"repo-one"},"1");
emit("agent.peer_detected",{organizationId:"org-a",repositoryId:"repo-one"},"2"); // duplicate
emit("commit.created",{repo:"repo-one"},"11");
assert.equal(delivered.length,6,"Stream should deduplicate independently per repo and tenant");
assert.deepEqual(delivered.map(x=>x[2]),["10","3","2","1","1","11"]);
console.log("PASS per-repository and tenant event cursor replay and duplicate suppression");
