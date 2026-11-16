import { createHash,randomBytes,createCipheriv,createDecipheriv,hkdfSync } from "node:crypto";
import { mkdir,readFile,open,rename,rm } from "node:fs/promises";
import { join } from "node:path";
import type { AgentSession } from "./cloud/coordination-types.js";

export interface StoredCompanionSession {
  sourceSessionId:string;
  host:string;
  session:AgentSession;
  sequence:number;
  archiveHead?:string;
  peerCursor?:string;
  archiveAuthorized?:boolean;
  rawAuthorized?:boolean;
  pendingClose?:boolean;
  closed?:boolean;
  excluded?:boolean;
}
export class NodeEncryptedSessionStore{
  private readonly master:Buffer;
  constructor(private readonly directory:string,keyBase64:string){
    this.master=Buffer.from(keyBase64,"base64");
    if(this.master.length!==32)throw new Error("CMP_COMPANION_KEY must be 32 random bytes in base64");
  }
  private key(hostSessionId:string){
    if(hostSessionId.length<3||hostSessionId.length>200)throw new Error("Invalid host session ID");
    return Buffer.from(hkdfSync("sha256",this.master,Buffer.from(hostSessionId),Buffer.from("cmp-companion-session-v1"),32));
  }
  private path(hostSessionId:string){
    return join(this.directory,createHash("sha256").update(hostSessionId).digest("hex")+".cmpstate");
  }
  async load(hostSessionId:string):Promise<StoredCompanionSession|null>{
    let content:string;
    try { content=await readFile(this.path(hostSessionId),"utf8"); }
    catch(e){
      if((e as NodeJS.ErrnoException).code==="ENOENT")return null;
      throw e;
    }
    if(content.length>131072)throw new Error("CMP session state exceeds limit");
    const object=JSON.parse(content) as {v:number;iv:string;tag:string;content:string};
    if(object.v!==1)throw new Error("Unsupported CMP session state");
    const iv=Buffer.from(object.iv,"base64url"),tag=Buffer.from(object.tag,"base64url");
    if(iv.length!==12||tag.length!==16)throw new Error("Invalid encrypted session state");
    const decipher=createDecipheriv("aes-256-gcm",this.key(hostSessionId),iv);
    decipher.setAuthTag(tag);
    const plaintext=Buffer.concat([decipher.update(Buffer.from(object.content,"base64url")),decipher.final()]);
    const state=JSON.parse(plaintext.toString("utf8")) as StoredCompanionSession;
    if(state.sourceSessionId!==hostSessionId||!state.session?.sessionToken)
      throw new Error("Invalid CMP session state binding");
    return state;
  }
  async save(hostSessionId:string,state:StoredCompanionSession):Promise<void>{
    if(state.sourceSessionId!==hostSessionId)throw new Error("CMP session state mismatch");
    await mkdir(this.directory,{recursive:true,mode:0o700});
    const iv=randomBytes(12),cipher=createCipheriv("aes-256-gcm",this.key(hostSessionId),iv);
    const encrypted=Buffer.concat([cipher.update(JSON.stringify(state)),cipher.final()]);
    const payload=JSON.stringify({v:1,iv:iv.toString("base64url"),tag:cipher.getAuthTag().toString("base64url"),
      content:encrypted.toString("base64url")});
    const path=this.path(hostSessionId),temp=path+"."+randomBytes(8).toString("hex")+".tmp";
    const file=await open(temp,"wx",0o600);
    try{await file.writeFile(payload);await file.sync();}finally{await file.close();}
    await rename(temp,path);
  }
  async delete(hostSessionId:string){await rm(this.path(hostSessionId),{force:true});}
}
