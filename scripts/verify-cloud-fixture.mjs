/**
 * Strict real-Cloud CLI interoperability runner.
 * Requires Backend+Security's disposal-certified local Next.js server,
 * PostgreSQL and Firebase Auth emulator. NEVER falls back to a mock.
 *
 * Build: cargo build --locked --features cloud-fixture-tests --bin cmp
 * Run: CMP_CLOUD_FIXTURE_CLI=/abs/cmp CMP_CLOUD_FIXTURE_MANIFEST=/abs/fixture.json
 *      CMP_CLOUD_FIXTURE_CONTROL=/abs/backend-control pnpm verify:cloud:fixture
 */
import assert from "node:assert/strict";
import {execFile,spawnSync} from "node:child_process";
import {promisify} from "node:util";
import {randomBytes} from "node:crypto";
import {createServer,request as httpRequest} from "node:http";
import {existsSync,mkdtempSync,mkdirSync,readFileSync,writeFileSync,rmSync,statSync,chmodSync} from "node:fs";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {validateCloudFixtureManifest} from "./cloud-fixture-contract.mjs";

const execute=promisify(execFile);
const manifestPath=process.env.CMP_CLOUD_FIXTURE_MANIFEST;
const binary=process.env.CMP_CLOUD_FIXTURE_CLI;
const control=process.env.CMP_CLOUD_FIXTURE_CONTROL;
if(!manifestPath||!binary||!control) {
 console.error("BLOCKED: CMP_CLOUD_FIXTURE_MANIFEST, CMP_CLOUD_FIXTURE_CLI and CMP_CLOUD_FIXTURE_CONTROL required. Backend must start an actual isolated Firebase Auth emulator + PostgreSQL + CMP Cloud Next.js HTTP server. NO mock fallback.");
 process.exit(2);
}
assert(resolve(binary)===binary && existsSync(binary),"Fixture CMP CLI must be absolute and built with cloud-fixture-tests");
assert(resolve(control)===control && existsSync(control),"Fixture controller must be an absolute executable path");
const manifest=validateCloudFixtureManifest(JSON.parse(readFileSync(manifestPath,"utf8")));
const origin=new URL(manifest.origin);
const grantNames={
 owner:"CMP_FIXTURE_REFRESH_OWNER",
 reader:"CMP_FIXTURE_REFRESH_READER",
 outsider:"CMP_FIXTURE_REFRESH_OUTSIDER",
 revokedMember:"CMP_FIXTURE_REFRESH_REVOKED_MEMBER",
 revokedGrant:"CMP_FIXTURE_REFRESH_REVOKED_GRANT",
 expired:"CMP_FIXTURE_REFRESH_EXPIRED",
};
for(const name of Object.values(grantNames)) {
 const value=process.env[name];
 assert(value?.startsWith("cmpr_")&&value.length>=16,
  "BLOCKED: missing fixture-only OAuth refresh grant in "+name);
}
const secrets=Object.values(grantNames).map(key=>process.env[key]);
const redact=value=>String(value??"").replaceAll(
 /cmpr_[A-Za-z0-9_-]{5,}/g,"[REDACTED]");
const root=mkdtempSync(join(tmpdir(),"cmp-cloud-fixture-cli-"));
chmodSync(root,0o700);
const session=randomBytes(24).toString("hex");
const passphrase="fixture-"+randomBytes(20).toString("hex");
let proxy;
let requestCount=0;
function noLeak(value){
 for(const secret of secrets)assert(!String(value).includes(secret),"CLI output exposed fixture refresh grant");
 assert(!String(value).includes(passphrase),"CLI output exposed fixture encryption key");
 assert(!/\b(?:cmpr|cmpa)_[A-Za-z0-9_-]{10,}/.test(String(value)),
  "CLI output exposed a CMP OAuth access or refresh token");
}
const clients=new Map();
function workspace(name){
 if(clients.has(name))return clients.get(name);
 const folder=join(root,name),home=join(folder,"home"),cwd=join(folder,"project");
 mkdirSync(home,{recursive:true,mode:0o700});mkdirSync(cwd,{recursive:true,mode:0o700});
 const marker=join(home,".cmp/fixture-session");
 mkdirSync(join(home,".cmp"),{recursive:true,mode:0o700});
 writeFileSync(marker,session,{mode:0o600});
 chmodSync(marker,0o600);
 const actor={name,home,cwd};
 clients.set(name,actor);
 return actor;
}
async function cmd(actor,args,{fail=false,override={},refreshToken}={}){
 const env={...process.env,CMP_HOME:actor.home,
  CMP_CLOUD_FIXTURE_MODE:"1",CMP_CLOUD_FIXTURE_SESSION:session,
  CMP_CREDENTIAL_PASSPHRASE:passphrase,
  ...override};
 for(const key of ["CMP_CONFIG","CMP_ENDPOINT","CMP_REPOSITORY","CMP_DEV_TOKEN","CMP_CLOUD_FIXTURE_REFRESH_TOKEN"])
  delete env[key];
 Object.assign(env,override);
 if(refreshToken)env.CMP_CLOUD_FIXTURE_REFRESH_TOKEN=refreshToken;
 let out,code=0;
 try{out=await execute(binary,args,{cwd:actor.cwd,env,timeout:25000,maxBuffer:1024*1024});}
 catch(error){code=Number(error.code)||1;out={stdout:error.stdout??"",stderr:error.stderr??""};}
 noLeak(out.stdout);noLeak(out.stderr);
 if(fail)assert.notEqual(code,0,"Expected CLI denial but command passed: "+args.join(" "));
 else assert.equal(code,0,"CLI "+args.join(" ")+" failed: "+redact(out.stderr).slice(0,330));
 return {stdout:out.stdout,stderr:out.stderr,code};
}
function readJson(output){return JSON.parse(output.stdout.trim());}
async function controlAction(action,extra={}){
 const input=JSON.stringify({action,...extra});
 const child=spawnSync(control,[],{input,encoding:"utf8",env:{...process.env,
  CMP_CLOUD_FIXTURE_MANIFEST:manifestPath},timeout:25000,maxBuffer:1024*1024});
 noLeak(child.stdout);noLeak(child.stderr);
 assert.equal(child.status,0,"Backend control failed "+action+": "+redact(child.stderr).slice(0,150));
 const result=JSON.parse(child.stdout);
 assert.equal(result.ok,true,"Backend fixture control action failed: "+action);
 return result;
}
async function createActor(name,repo,grant){
 const actor=workspace(name);
 await cmd(actor,["setup","--scope","project","--backend","cloud",
  "--endpoint",proxy.origin,"--repo",repo,"--no-interactive"]);
 await cmd(actor,["fixture","seed-refresh"],{refreshToken:process.env[grant]});
 const file=join(actor.home,".cmp/credentials.enc");
 assert(existsSync(file),"Encrypted refresh grant was not stored");
 const ciphertext=readFileSync(file,"utf8");
 assert(!ciphertext.includes(process.env[grant]),"Refresh grant leaked as plaintext");
 if(process.platform!=="win32") assert.equal(statSync(file).mode&0o077,0);
 assert.equal(readFileSync(join(actor.home,".cmp/credential-store"),"utf8").trim(),"encrypted");
 return actor;
}
const record=(id)=>({
 id,kind:"fact",status:"approved",content:"synthetic Cloud fixture "+id,
 sourceRefs:["fixture-only:synthetic"],authorId:"cloud-fixture-cli",
 updatedAt:"2026-11-17T12:00:00Z",
});
function changesFile(actor,id){const path=join(actor.cwd,"changes-"+id+".json");
 writeFileSync(path,JSON.stringify({message:"fixture "+id,
  changes:[{op:"put",record:record(id)}]}),{mode:0o600});return path;
}
function hasRecord(snapshot,id){return snapshot.records?.some(x=>x.id===id);}
async function snapshot(actor,repository){
 return readJson(await cmd(actor,["snapshot"],{override:repository?{CMP_REPOSITORY:repository}:{}}));
}
function pending(text){
 const match=String(text).match(/Pending CMP stream operations:\s*(\d+)/);
 assert(match,"Invalid stream status output");
 return Number(match[1]);
}
function makeProxy(backend){
 const target=new URL(backend);
 const server=createServer((req,res)=>{
  requestCount++;
  // This is an HTTP byte-forwarding counter, never a synthetic API backend.
  const upstream=httpRequest({hostname:"127.0.0.1",port:Number(target.port),
   path:req.url,method:req.method,headers:{...req.headers,host:target.host}},
   response=>{
    if((response.statusCode??0)>=300&&(response.statusCode??0)<400){
     const location=response.headers.location;
     if(location && new URL(location,backend).origin!==target.origin){
      response.resume();res.writeHead(502);res.end("Fixture blocked off-loopback redirect");return;
     }
    }
    res.writeHead(response.statusCode??502,response.headers);response.pipe(res);
   });
  upstream.on("error",()=>{if(!res.headersSent)res.writeHead(502);res.end();});
  req.pipe(upstream);
 });
 return new Promise((resolveListen,reject)=>{
  server.on("error",reject);
  server.listen(0,"127.0.0.1",()=>resolveListen({
   origin:"http://127.0.0.1:"+server.address().port,
   close:()=>new Promise(done=>{server.closeAllConnections();server.close(done);})
  }));
 });
}
try{
 proxy=await makeProxy(manifest.origin);
 // This probe reaches the real fixture's public Cloud API, not a mocked
 // response. The Backend fixture contract must provide the actual Next.js
 // implementation and preserve real OAuth/PostgreSQL enforcement.
 const probe=await fetch(proxy.origin+"/v1/capabilities",{
  signal:AbortSignal.timeout(5000)});
 assert.equal(probe.status,200,"Backend disposable Cloud capabilities not ready");
 const advertised=await probe.json();
 assert.equal(advertised.implementation,"cmp-cloud",
  "Expected actual CMP Cloud capabilities handler");
 assert.equal(advertised.protocolVersion,1);
 const r=manifest.repositories;
 const owner=await createActor("owner",r.ownerWritable,grantNames.owner);
 const reader=await createActor("reader",r.readerReadable,grantNames.reader);
 const outsider=await createActor("outsider",r.outsiderPrivate,grantNames.outsider);
 const revokeMember=await createActor("revoked-member",r.revocableMember,grantNames.revokedMember);
 const revokeGrant=await createActor("revoked-grant",r.revocableGrant,grantNames.revokedGrant);
 const expired=await createActor("expired",r.ownerWritable,grantNames.expired);

 // These calls reach the real disposable Cloud server with OAuth refresh/rotate.
 for(const actor of [owner,reader,outsider,revokeMember,revokeGrant]){
  const checked=await cmd(actor,["doctor"]);
  assert.match(checked.stdout,/Authenticated repository API: connected/);
  assert.match(checked.stdout,/Selected repository:/);
 }
 const deniedExpired=await cmd(expired,["doctor"],{fail:true});
 assert(!deniedExpired.stdout.includes("Authenticated repository API: connected"));

 // A read-only repository must be usable, but its membership must not permit writes.
 const readerBefore=await snapshot(reader);
 await cmd(reader,["commit",changesFile(reader,"reader_illegal_write")],{fail:true});
 assert(!hasRecord(await snapshot(reader),"reader_illegal_write"));
 assert.deepEqual((await snapshot(reader)).records,readerBefore.records);

 // Cross-tenant: member of beta may not read alpha's private repo.
 await cmd(outsider,["doctor"],{fail:true,override:{CMP_REPOSITORY:r.ownerWritable}});
 await cmd(outsider,["api","/v1/repos/"+encodeURIComponent(r.ownerWritable)+"/head"],
  {fail:true});

 // Live membership revocation is a real server-side fixture mutation.
 await controlAction("revokeMembership",{principal:"revokedMember",
  repository:r.revocableMember});
 await cmd(revokeMember,["doctor"],{fail:true});
 await controlAction("revokeOAuth",{principal:"revokedGrant"});
 await cmd(revokeGrant,["doctor"],{fail:true});

 // Local/offline cannot talk to Cloud until the user explicitly opts in.
 const offline=workspace("offline-stream");
 await cmd(offline,["setup","--scope","project","--backend","offline",
  "--endpoint",proxy.origin,"--repo",r.streamWritable,"--no-interactive"]);
 await cmd(offline,["init","--repo",r.streamWritable,"--no-interactive"]);
 const initialRequestCount=requestCount;
 await cmd(offline,["commit",changesFile(offline,"local_disabled")]);
 assert.equal(requestCount,initialRequestCount,"Offline default sent network requests");
 assert.equal(pending((await cmd(offline,["stream","status"])).stdout),0);
 assert(!hasRecord(await snapshot(owner,r.streamWritable),"local_disabled"));
 await cmd(offline,["config","set","streaming","true"]);
 assert.equal(pending((await cmd(offline,["stream","status"])).stdout),0,
  "Enabling streaming must not silently backfill");
 await cmd(offline,["commit",changesFile(offline,"streamed_first")]);
 assert.equal(pending((await cmd(offline,["stream","status"])).stdout),0);
 assert(hasRecord(await snapshot(owner,r.streamWritable),"streamed_first"));

 // Transient 503 must remain queued until an explicit successful retry.
 await controlAction("failNextCommit",{repository:r.streamWritable,status:503});
 await cmd(offline,["commit",changesFile(offline,"retry_after_503")]);
 assert.equal(pending((await cmd(offline,["stream","status"])).stdout),1);
 assert(!hasRecord(await snapshot(owner,r.streamWritable),"retry_after_503"));
 await cmd(offline,["stream","push"]);
 assert.equal(pending((await cmd(offline,["stream","status"])).stdout),0);
 assert(hasRecord(await snapshot(owner,r.streamWritable),"retry_after_503"));

 // A third-party remote commit after the acknowledged HEAD must block pushes.
 await cmd(owner,["commit",changesFile(owner,"remote_concurrent")],
  {override:{CMP_REPOSITORY:r.streamWritable}});
 await cmd(offline,["commit",changesFile(offline,"local_conflicting")]);
 assert.equal(pending((await cmd(offline,["stream","status"])).stdout),1);
 await cmd(offline,["stream","push"],{fail:true});
 assert.equal(pending((await cmd(offline,["stream","status"])).stdout),1);
 assert(!hasRecord(await snapshot(owner,r.streamWritable),"local_conflicting"));
 await cmd(offline,["config","set","streaming","false"]);
 const count=requestCount;
 await cmd(offline,["commit",changesFile(offline,"local_after_disable")]);
 assert.equal(requestCount,count,"Disabled streaming sent a network request");
 assert.equal(pending((await cmd(offline,["stream","status"])).stdout),1,
  "Disabled streaming must not flush or enqueue new local edits");
 console.log("PASS REAL disposable CMP Cloud CLI: authorized doctor, refresh rotation, expired grant, revoked OAuth and membership, cross-tenant/read-only denial, encrypted refresh storage, opt-in zero-network and streaming 503 retry / HEAD conflict");
}finally{
 if(proxy)await proxy.close();
 rmSync(root,{recursive:true,force:true});
}
