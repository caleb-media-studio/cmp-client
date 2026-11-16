#!/usr/bin/env node
import { CMPClient } from "./index.js";
import type { Change } from "@cmp-chat/protocol";
import {readFile,stat} from "node:fs/promises";

const help=`CMP context CLI

Usage:
  cmp-context repos
  cmp-context create <repository>
  cmp-context head <repository> [branch]
  cmp-context snapshot <repository> [ref]
  cmp-context branch <repository> <name> [from]
  cmp-context diff <repository> <from> <to>
  cmp-context merge <repository> <source> <target>
  cmp-context commit <repository> <branch> <expected-head> <json-file>
  cmp-context help

Environment:
  CMP_ENDPOINT      API URL (default: http://127.0.0.1:8787)
  CMP_ACCESS_TOKEN  Bearer token for hosted CMP Cloud (refresh externally before expiry)
  CMP_DEV_TOKEN     Development-server token fallback; never pass tokens as CLI arguments
  CMP_AUTHOR_ID     Commit author in self-hosted environments (Cloud overrides with verified UID)

JSON commit file must include message and changes (CMP Change[]).
The CLI calls the same CMP protocol endpoints as the official SDK.
`;

async function main() {
  const [command,repo,ref,extra,filename]=process.argv.slice(2);
  if(!command||command==="help"||command==="--help"){console.log(help);return;}
  const token=process.env.CMP_ACCESS_TOKEN||process.env.CMP_DEV_TOKEN;
  if(!token){console.error("CMP_ACCESS_TOKEN or CMP_DEV_TOKEN is required");process.exitCode=2;return;}
  const client=new CMPClient({
    httpEndpoint:process.env.CMP_ENDPOINT||"http://127.0.0.1:8787",
    accessToken:token
  });
  const cmd=command.toLowerCase();
  let result:unknown;
  if(cmd==="repos")result=await client.listRepositories();
  else if(cmd==="head"&&repo)result=await client.getHead(repo,ref||"main");
  else if(cmd==="snapshot"&&repo)result=await client.getSnapshot(repo,ref||"main");
  else if(cmd==="create"&&repo)result=await client.createRepository(repo);
  else if(cmd==="branch"&&repo&&ref)result=await client.createBranch(repo,ref,extra||"main");
  else if(cmd==="diff"&&repo&&ref&&extra)result=await client.diff(repo,ref,extra);
  else if(cmd==="merge"&&repo&&ref&&extra)result=await client.merge(repo,ref,extra,process.env.CMP_AUTHOR_ID||"cmp-cli");
  else if(cmd==="commit"&&repo&&ref&&extra&&filename){
    const metadata=await stat(filename);
    if(!metadata.isFile()||metadata.size>5_000_000)throw Error("Commit file must be a regular JSON file under 5 MB");
    const body=JSON.parse(await readFile(filename,"utf8")) as {message?:unknown;changes?:unknown};
    if(typeof body.message!=="string"||!Array.isArray(body.changes))
     throw Error("Commit file must contain message and changes array");
    result=await client.commit(repo,{branch:ref,expectedHead:extra,
     authorId:process.env.CMP_AUTHOR_ID||"cmp-cli",message:body.message,changes:body.changes as Change[]});
  }
  else {console.error("Unknown or incomplete command.\n"+help);process.exitCode=2;return;}
  process.stdout.write(JSON.stringify(result,null,2)+"\n");
}
main().catch(e=>{console.error(e instanceof Error?e.message:String(e));process.exitCode=1;});
