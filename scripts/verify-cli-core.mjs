/**
 * M1 true interoperability: compiled native CMP CLI child processes talk to
 * the actual dev-only CMP Core HTTP server with persisted SQLite on disk.
 * This is not a mocked response harness and never uses production credentials.
 *
 * CMP_CLI_BINARY=/absolute/path/to/cmp \
 * CMP_CORE_SERVER_MODULE=/absolute/path/to/cmp-server/dist/server.js \
 * node scripts/verify-cli-core.mjs
 */
import assert from "node:assert/strict";
import {execFile} from "node:child_process";
import {promisify} from "node:util";
import {existsSync,mkdtempSync,mkdirSync,writeFileSync,rmSync} from "node:fs";
import {tmpdir} from "node:os";
import {join,resolve,dirname} from "node:path";
import {pathToFileURL} from "node:url";

const executable=process.env.CMP_CLI_BINARY&&resolve(process.env.CMP_CLI_BINARY);
const modulePath=process.env.CMP_CORE_SERVER_MODULE&&resolve(process.env.CMP_CORE_SERVER_MODULE);
if(!executable||!modulePath||!existsSync(executable)||!existsSync(modulePath)){
 console.error("Provide CMP_CLI_BINARY and CMP_CORE_SERVER_MODULE pointing to a built native CMP CLI and real Core server.");
 process.exit(2);
}
const {createCMPServer}=await import(pathToFileURL(modulePath).href);
const {Store}=await import(pathToFileURL(join(dirname(modulePath),"storage.js")).href);
const execute=promisify(execFile);
const token="core-m1-disposable-development-token-123456789";
const dir=mkdtempSync(join(tmpdir(),"cmp-m1-real-cli-"));
const home=join(dir,"home"),legacy=join(dir,"legacy"),fresh=join(dir,"fresh"),initial=join(dir,"initial");
for(const p of [home,legacy,fresh,initial])mkdirSync(p,{recursive:true,mode:0o700});
const db=join(dir,"core.sqlite");
const contextRecord=(id,content)=>({
 id,kind:"fact",status:"approved",content,sourceRefs:["m1:fixture"],
 authorId:"m1-cli",updatedAt:"2026-11-16T19:00:00Z"
});
const parseJson=value=>JSON.parse(value.trim());
let app;
let port;
const env={...process.env,CMP_HOME:home,CMP_DEV_TOKEN:token};
for(const key of ["CMP_CONFIG","CMP_ENDPOINT","CMP_REPOSITORY","CMP_CREDENTIAL_PASSPHRASE"]){
 delete env[key];
}
async function cmp(cwd,args,{shouldFail=false,tokenOverride}={}){
 const processEnv={...env,CMP_DEV_TOKEN:tokenOverride??token};
 let output;
 try{output=await execute(executable,args,{cwd,env:processEnv,timeout:20000,maxBuffer:1024*1024});}
 catch(error){
  assert(shouldFail,"cmp "+args.join(" ")+" failed: "+error.stderr);
  assert(!String(error.stderr).includes(token),"CLI leaked bearer token");
  return {failed:true,stdout:error.stdout??"",stderr:error.stderr??""};
 }
 assert(!shouldFail,"Expected CMP command to fail: "+args.join(" "));
 assert(!output.stdout.includes(token),"CLI printed bearer token");
 return output.stdout;
}
const api=(path,method="GET",body,auth=token)=>
 fetch("http://127.0.0.1:"+port+path,{
  method,headers:{authorization:"Bearer "+auth,"content-type":"application/json"},
  body:body===undefined?undefined:JSON.stringify(body)
 });
const setup=async(cwd,repo)=>cmp(cwd,[
 "setup","--scope","project","--backend","local","--endpoint","http://127.0.0.1:"+port,
 "--repo",repo,"--no-interactive"
]);
const firstStore=new Store(db);
const seeded=firstStore.create("legacy_repo");
const original=seeded.head();
// A legacy pre-canonical event must stay flat in SQLite yet be canonical
// through the public CLI/API response, including after server restarts.
firstStore.mutate("legacy_repo",r=>r.branch("historical"),"branch.created");
const historicalCursor=firstStore.eventsAfter("legacy_repo")[0].cursor;
assert.equal(firstStore.eventsAfter("legacy_repo")[0].params.repo,"legacy_repo");
firstStore.close();
process.env.CMP_ENV="development";
try{
 app=createCMPServer({token,dbFile:db,host:"127.0.0.1",port:0});
 port=(await app.listen()).port;
 // A pre-existing flat SQLite row can be selected directly by canonical name.
 await setup(legacy,"local/legacy_repo");
 await cmp(legacy,["init","--repo","local/legacy_repo","--no-interactive"]);
 assert.equal(parseJson(await cmp(legacy,["head"])).head,original);
 assert.deepEqual(parseJson(await cmp(legacy,["snapshot"])).records,[]);
 const doctor=await cmp(legacy,["doctor"]);
 assert.match(doctor,/Authenticated repository API: connected/);
 assert.match(doctor,/Selected repository: local\/legacy_repo/);
 assert.deepEqual(parseJson(await cmp(legacy,["repo","list"])).repositories,["local/legacy_repo"]);
const historicalEvents=parseJson(await cmp(legacy,[
 "api","/v1/repos/local%2Flegacy_repo/events"
])).events;
assert.equal(historicalEvents.length,1);
assert.equal(historicalEvents[0].params.repo,"local/legacy_repo");
assert.equal(historicalEvents[0].cursor,historicalCursor);
assert.deepEqual(parseJson(await cmp(legacy,[
 "api","/v1/repos/legacy_repo/events"
])).events,historicalEvents);
 assert.match(await cmp(legacy,["repo","view"]),/local\/legacy_repo.cmp/);
 assert.equal((await cmp(legacy,["config","get","streaming"])).trim(),"false");
 const invalid=await cmp(legacy,["repo","create","other/forbidden"],{shouldFail:true});
 assert(invalid.failed);
 const created=parseJson(await cmp(legacy,["repo","create","local/new_repo"]));
 assert.equal(created.id,"local/new_repo");
 assert.deepEqual(parseJson(await cmp(legacy,["repo","list"])).repositories.slice().sort(),
  ["local/legacy_repo","local/new_repo"].sort());
 // Exercise create-via-init, not just repo create from another project.
 await setup(initial,"local/created_by_init");
 await cmp(initial,["init","--repo","local/created_by_init","--create","--no-interactive"]);
 assert.match(await cmp(initial,["doctor"]),/Selected repository: local\/created_by_init/);
 await setup(fresh,"local/new_repo");
 await cmp(fresh,["init","--repo","local/new_repo","--no-interactive"]);
 assert.match(await cmp(fresh,["doctor"]),/Selected repository: local\/new_repo/);
 const before=parseJson(await cmp(fresh,["head"])).head;
 const file=join(fresh,"first.json");
 writeFileSync(file,JSON.stringify({message:"first real native CLI commit",
  changes:[{op:"put",record:contextRecord("first","First real Core CLI fact")}] }));
 const committed=parseJson(await cmp(fresh,["commit",file]));
 assert.equal(committed.parents[0],before);
 assert.equal(parseJson(await cmp(fresh,["snapshot"])).records[0].id,"first");
 assert.equal(parseJson(await cmp(fresh,["snapshot",before])).records.length,0);
 assert.equal(parseJson(await cmp(fresh,["diff",before,"main"])).changes.length,1);
 const stale=join(fresh,"stale.json");
 writeFileSync(stale,JSON.stringify({message:"stale CAS should reject",expectedHead:before,
  changes:[{op:"put",record:contextRecord("wrong","Must never be stored")}] }));
 await cmp(fresh,["commit",stale],{shouldFail:true});
 assert.equal(parseJson(await cmp(fresh,["snapshot"])).records.length,1);
 const feature=parseJson(await cmp(fresh,["branch","create","feature"]));
 assert.equal(feature.head,committed.id);
 assert.equal(parseJson(await cmp(fresh,["branch","list"])).branches.feature,committed.id);
 const featureFile=join(fresh,"feature.json");
 writeFileSync(featureFile,JSON.stringify({message:"feature commit",branch:"feature",
  expectedHead:feature.head,changes:[
   {op:"put",record:contextRecord("second","Feature branch fact")}] }));
 const featureCommit=parseJson(await cmp(fresh,["commit",featureFile]));
 assert.equal(featureCommit.branch,"feature");
 assert.equal(parseJson(await cmp(fresh,["snapshot"])).records.length,1);
 const merged=parseJson(await cmp(fresh,["merge","feature","main"]));
 assert.equal(merged.parents.length,2);
 assert.equal(parseJson(await cmp(fresh,["snapshot"])).records.length,2);
 assert.equal(parseJson(await cmp(fresh,["diff",before,"main"])).changes.length,2);
const newlyCreatedEvents=parseJson(await cmp(fresh,[
 "api","/v1/repos/local%2Fnew_repo/events"
])).events;
assert(newlyCreatedEvents.length>=3);
assert(newlyCreatedEvents.every(event=>event.params.repo==="local/new_repo"));
assert.deepEqual(parseJson(await cmp(fresh,[
 "api","/v1/repos/new_repo/events"
])).events,newlyCreatedEvents);
 // Exact .cmp JSON transport identity and snapshot fetched from real Core.
 let resp=await api("/local/new_repo.cmp");
 assert.equal(resp.status,200);
 const manifest=await resp.json();
 assert.equal(manifest.repository,"local/new_repo");
 assert.equal(manifest.protocol,"cmp");
 assert.equal(manifest.transportVersion,1);
 assert.equal(manifest.head,merged.id);
 resp=await api(manifest.links.snapshot);
 const remoteSnapshot=await resp.json();
 assert.equal(remoteSnapshot.repository,manifest.repository);
 assert.equal(remoteSnapshot.records.length,2);
 const cloneDir=join(fresh,"clone");
 await cmp(fresh,["repo","clone","local/new_repo",cloneDir]);
 assert(existsSync(join(cloneDir,".cmp/config.toml")));
 assert(!existsSync(join(cloneDir,".git")));
 assert.equal(parseJson(await cmp(cloneDir,["snapshot"])).records.length,2);
 assert.equal((await cmp(cloneDir,["config","get","streaming"])).trim(),"false");
 await cmp(fresh,["doctor"],{shouldFail:true,tokenOverride:"wrong-"+token});
 assert.equal((await api("/v1/repos","GET",undefined,"wrong-"+token)).status,401);
 // Exercise a complete shutdown/reopen of exactly the same SQLite database.
 await app.close();app=undefined;
 const flat=new Store(db);
 assert.deepEqual(flat.list().slice().sort(),["created_by_init","legacy_repo","new_repo"].sort());
assert.equal(flat.eventsAfter("legacy_repo")[0].params.repo,"legacy_repo");
assert(flat.eventsAfter("new_repo").every(event=>event.params.repo==="new_repo"));
flat.close();
 app=createCMPServer({token,dbFile:db,host:"127.0.0.1",port});
 port=(await app.listen()).port;
 assert.match(await cmp(fresh,["doctor"]),/Authenticated repository API: connected/);
 assert.equal(parseJson(await cmp(fresh,["head"])).head,merged.id);
 assert.equal(parseJson(await cmp(fresh,["snapshot"])).records.length,2);
assert.equal(parseJson(await cmp(legacy,[
 "api","/v1/repos/local%2Flegacy_repo/events?cursor=0"
])).events[0].params.repo,"local/legacy_repo");
assert.deepEqual(parseJson(await cmp(fresh,[
 "api","/v1/repos/local%2Fnew_repo/events"
])).events,newlyCreatedEvents);
assert.equal((await (await api("/local/legacy_repo.cmp")).json()).repository,"local/legacy_repo");
 console.log("PASS native CMP CLI against real Core server and persisted SQLite: init, create/list/doctor, branches/commit/snapshot/diff/merge/CAS, historical/new canonical event payloads, legacy flat SQLite keys, .cmp, clone, restart and authentication denial");
} finally {
 if(app)await app.close();
 rmSync(dir,{recursive:true,force:true});
}
