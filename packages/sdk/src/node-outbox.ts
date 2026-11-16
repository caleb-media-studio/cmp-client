import { mkdir, open, readFile, rename, rm, stat } from "node:fs/promises";
import { dirname, join } from "node:path";
import { randomBytes, createHash, hkdfSync, createCipheriv, createDecipheriv } from "node:crypto";
import type { SessionArchiveEvent } from "./cloud/archive-types.js";
import type { CompanionOutbox } from "./companion.js";

/** Local Node.js encrypted outbox, for a single companion process per directory.
 * The key must be supplied by the host application, not committed to Git.
 * Per-session keys are derived from the host's 32-byte master key.
 */
export class NodeEncryptedOutbox implements CompanionOutbox {
 private readonly key:Buffer;
 constructor(private readonly directory:string,base64Key:string){
  const key=Buffer.from(base64Key,"base64");
  if(key.length!==32)throw new Error("CMP companion outbox needs a 32-byte base64 master key");
  this.key=key;
 }
 private filename(id:string){
  if(!/^[a-zA-Z0-9_-]{8,100}$/.test(id))throw new Error("Invalid CMP session identifier");
  return join(this.directory,createHash("sha256").update(id).digest("hex")+".cmpq");
 }
 private derive(id:string){
  return Buffer.from(hkdfSync("sha256",this.key,Buffer.from(id),Buffer.from("cmp-local-outbox-v1"),32));
 }
 private async read(id:string):Promise<SessionArchiveEvent[]>{
  const path=this.filename(id);
  let raw:Buffer;
  try{raw=await readFile(path);}catch(err){
   if((err as NodeJS.ErrnoException).code==="ENOENT")return [];
   throw err;
  }
  if(raw.length>8_388_608)throw new Error("CMP outbox too large");
  const envelope=JSON.parse(raw.toString("utf8")) as {version:number;iv:string;tag:string;ciphertext:string};
  if(envelope.version!==1)throw new Error("Unsupported CMP outbox encryption version");
  const iv=Buffer.from(envelope.iv,"base64url"),tag=Buffer.from(envelope.tag,"base64url");
  if(iv.length!==12||tag.length!==16)throw new Error("Invalid CMP outbox envelope");
  const decipher=createDecipheriv("aes-256-gcm",this.derive(id),iv);
  decipher.setAuthTag(tag);
  const plain=Buffer.concat([decipher.update(Buffer.from(envelope.ciphertext,"base64url")),decipher.final()]);
  const entries=JSON.parse(plain.toString("utf8")) as SessionArchiveEvent[];
  if(!Array.isArray(entries)||entries.length>2000)throw new Error("Corrupted CMP outbox");
  return entries;
 }
 private async save(id:string,entries:SessionArchiveEvent[]){
  await mkdir(this.directory,{recursive:true,mode:0o700});
  const path=this.filename(id);
  const iv=randomBytes(12),cipher=createCipheriv("aes-256-gcm",this.derive(id),iv);
  const plain=Buffer.from(JSON.stringify(entries));
  if(plain.length>4_194_304)throw new Error("CMP outbox event limit exceeded");
  const ciphertext=Buffer.concat([cipher.update(plain),cipher.final()]);
  const content=JSON.stringify({version:1,iv:iv.toString("base64url"),
   tag:cipher.getAuthTag().toString("base64url"),ciphertext:ciphertext.toString("base64url")});
  const tmp=path+"."+randomBytes(8).toString("hex")+".tmp";
  const handle=await open(tmp,"wx",0o600);
  try{
   await handle.writeFile(content);await handle.sync();
  }finally{await handle.close();}
  await rename(tmp,path);
 }
 async enqueue(sessionId:string,event:SessionArchiveEvent){
  const entries=await this.read(sessionId);
  if(entries.some(v=>v.eventId===event.eventId))return;
  if(entries.length>=2000)throw new Error("CMP offline archive outbox reached its capacity");
  entries.push(event);
  await this.save(sessionId,entries);
 }
 async list(sessionId:string){return this.read(sessionId);}
 /** Acknowledge just the first durably queued event after server commit. */
 async ack(sessionId:string,eventId:string){
  const entries=await this.read(sessionId);
  if(!entries.length||entries[0]!.eventId!==eventId)
   throw new Error("CMP outbox acknowledgment must match the next queued event");
  if(entries.length===1)await this.clear(sessionId);
  else await this.save(sessionId,entries.slice(1));
 }
 async clear(sessionId:string){
  await rm(this.filename(sessionId),{force:true});
 }
}
