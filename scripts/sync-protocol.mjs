import {createHash} from "node:crypto";
import {readFileSync,writeFileSync,mkdirSync,existsSync,cpSync,readdirSync} from "node:fs";
import {join,dirname,resolve} from "node:path";
import {execFileSync} from "node:child_process";
import {fileURLToPath} from "node:url";
const client=resolve(dirname(fileURLToPath(import.meta.url)),"..");
const standard=resolve(process.env.CMP_PROTOCOL_SOURCE||join(client,"..","cmp-protocol"));
const source=join(standard,"packages","protocol");
const mirror=join(client,"packages","protocol");
const manifest=join(client,"protocol-snapshot.json");
const tracked=["package.json","tsconfig.json"];
function files(folder){
 const acc=[...tracked];
 function traverse(parent){
  for(const item of readdirSync(join(folder,parent),{withFileTypes:true})){
   const path=join(parent,item.name);
   if(item.isDirectory())traverse(path);
   else if(item.isFile()&&/\.(ts|json)$/.test(item.name))acc.push(path);
  }
 }
 traverse("src");
 return acc.sort();
}
function sha(b){return createHash("sha256").update(b).digest("hex");}
const standardAvailable=existsSync(source);
if(!standardAvailable&&(process.argv.includes("--write")||process.argv.includes("--strict")))
 throw new Error("CMP standard checkout missing: "+source);
const list=files(standardAvailable?source:mirror);
const sourceFiles=Object.fromEntries(list.map(path=>
 [path,sha(readFileSync(join(standardAvailable?source:mirror,path)))]));
if(process.argv.includes("--write")){
 for(const path of list){
  mkdirSync(dirname(join(mirror,path)),{recursive:true});
  cpSync(join(source,path),join(mirror,path));
 }
 const commit=execFileSync("git",["rev-parse","HEAD"],{cwd:standard,encoding:"utf8"}).trim();
 writeFileSync(manifest,JSON.stringify({
  standardRepo:"caleb-media-studio/cmp-protocol",
  standardCommit:commit,
  package:"@cmp-chat/protocol",
  files:sourceFiles
 },null,2)+"\n");
 console.log("Pinned CMP wire types to cmp-protocol "+commit.slice(0,10));
}else{
 if(!existsSync(manifest))throw new Error("No protocol snapshot manifest. Run pnpm protocol:sync.");
 const pin=JSON.parse(readFileSync(manifest,"utf8"));
 if(pin.standardRepo!=="caleb-media-studio/cmp-protocol")throw new Error("Unexpected standard source");
 for(const path of list){
  const mirrorHash=sha(readFileSync(join(mirror,path)));
  if(mirrorHash!==sourceFiles[path]||pin.files[path]!==sourceFiles[path])
   throw new Error("Protocol type mirror drift: "+path+"; sync from cmp-protocol and review.");
 }
 if(Object.keys(pin.files).length!==list.length)throw new Error("Protocol source file list changed");
 console.log("PASS protocol type mirror matches pinned manifest; "+(standardAvailable
  ?"authoritative cmp-protocol checkout verified"
  :"source checkout unavailable, remote drift not checked")
  +"; pinned commit "+pin.standardCommit.slice(0,10));
}
