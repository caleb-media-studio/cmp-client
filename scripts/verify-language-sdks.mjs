import {spawnSync} from "node:child_process";
import {existsSync} from "node:fs";
import {join} from "node:path";
import {fileURLToPath} from "node:url";

const root=fileURLToPath(new URL("../",import.meta.url));
const tasks=[
 {name:"Python",cmd:"python3",args:["-m","unittest","discover","-s","sdk/python/tests","-p","test_*.py"],cwd:root,
  env:{PYTHONPATH:join(root,"sdk/python/src")}},
 {name:"Go",cmd:"go",args:["test","./..."],cwd:join(root,"sdk/go")},
 {name:"Rust",cmd:"cargo",args:["test","--offline","--quiet"],cwd:join(root,"sdk/rust")}
];
let failed=false;
for(const task of tasks){
 const which=spawnSync("sh",["-c","command -v "+task.cmd],{encoding:"utf8"});
 if(which.status!==0){
  console.log("SKIP "+task.name+" runtime unavailable");
  continue;
 }
 const result=spawnSync(task.cmd,task.args,{
  cwd:task.cwd,env:{...process.env,...task.env},encoding:"utf8",timeout:120000
 });
 process.stdout.write(result.stdout||"");
 process.stderr.write(result.stderr||"");
 if(result.status!==0){failed=true;console.error("FAIL "+task.name+" Core SDK unit suite");}
 else console.log("PASS "+task.name+" SDK unit suite");
}
if(failed)process.exitCode=1;
