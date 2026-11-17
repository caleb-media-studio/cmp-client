/** Native fixture-only binary boundary smoke. No Cloud server or user tokens. */
import assert from "node:assert/strict";
import {execFileSync,spawnSync} from "node:child_process";
import {randomBytes} from "node:crypto";
import {existsSync,mkdtempSync,mkdirSync,readFileSync,writeFileSync,chmodSync,rmSync} from "node:fs";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
const binary=process.env.CMP_CLOUD_FIXTURE_CLI;
if(!binary || resolve(binary)!==binary || !existsSync(binary)){
 console.error("BLOCKED: compile native CMP with --features cloud-fixture-tests and set CMP_CLOUD_FIXTURE_CLI");
 process.exit(2);
}
const root=mkdtempSync(join(tmpdir(),"cmp-cli-fixture-boundary-"));
const home=join(root,"home"),cwd=join(root,"project");
mkdirSync(join(home,".cmp"),{recursive:true,mode:0o700});
mkdirSync(join(cwd,".cmp"),{recursive:true,mode:0o700});
const session=randomBytes(20).toString("hex");
const secret="cmpr_"+randomBytes(24).toString("hex");
const key="fixture-password-"+randomBytes(20).toString("hex");
const marker=join(home,".cmp/fixture-session");
writeFileSync(marker,session,{mode:0o600});chmodSync(marker,0o600);
const config=join(cwd,".cmp/config.toml");
const setup=(endpoint)=>writeFileSync(config,
 'version=1\nbackend="cloud"\nendpoint="'+endpoint+'"\nbranch="main"\nrepository="fixture/test"\n');
const env={...process.env,CMP_HOME:home,
 CMP_CLOUD_FIXTURE_MODE:"1",CMP_CLOUD_FIXTURE_SESSION:session,
 CMP_CLOUD_FIXTURE_REFRESH_TOKEN:secret,CMP_CREDENTIAL_PASSPHRASE:key};
for(const name of ["CMP_CONFIG","CMP_ENDPOINT","CMP_REPOSITORY","CMP_DEV_TOKEN"])
 delete env[name];
function cli(args,opts={}){
 const result=spawnSync(binary,args,{cwd,env:{...env,...opts.env},encoding:"utf8",timeout:12000});
 assert(!String(result.stdout).includes(secret));
 assert(!String(result.stderr).includes(secret));
 return result;
}
try{
 setup("http://127.0.0.1:9");
 const seeded=cli(["fixture","seed-refresh"]);
 assert.equal(seeded.status,0,String(seeded.stderr));
 const encrypted=readFileSync(join(home,".cmp/credentials.enc"),"utf8");
 assert(!encrypted.includes(secret),"Fixture grant must not be saved in plaintext");
 assert.equal(readFileSync(join(home,".cmp/credential-store"),"utf8").trim(),"encrypted");
 const denied=cli(["doctor"],{env:{CMP_CLOUD_FIXTURE_SESSION:"not-the-session"}});
 assert.notEqual(denied.status,0);
 assert.match(denied.stderr,/marker mismatch|session marker/i);
 const absent=cli(["doctor"],{env:{CMP_CLOUD_FIXTURE_MODE:"0"}});
 assert.notEqual(absent.status,0);
 assert.match(absent.stderr,/supports only cloud\.cmp\.chat/i);
 setup("https://cloud.cmp.chat");
 const forbidden=cli(["doctor"]);
 assert.notEqual(forbidden.status,0);
 assert.match(forbidden.stderr,/fixture mode forbids the production/i);
 // This is an enforcement/credential-encryption test, not an OAuth simulation.
 console.log("PASS fixture-only native CMP: encrypted disposable grant, mismatched-session denial, disabled-mode denial and production-origin prohibition");
}finally{rmSync(root,{recursive:true,force:true});}
