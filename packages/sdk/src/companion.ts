import type {AgentJoinRequest,AgentSession} from "./cloud/coordination-types.js";
import type {ArchiveLocalConfig,SessionArchiveEvent,SessionEventKind,ArchiveRevision}
 from "./cloud/archive-types.js";
import { isRetryableCMPError, type CMPClient } from "./index.js";

/**
 * The CMP Companion attaches to a supported host adapter. It never scans home
 * directories or guesses at transcripts. The host explicitly delivers events.
 */
export type HostVisibleEvent={
 eventId:string;
 kind:SessionEventKind;
 sequence:number;
 occurredAt:string;
 sourceOffset?:string;
 sourceHash?:string;
 branch?:string;
 parentIds?:string[];
 expectedHead?:string;
 contentType?:string;
 /** Only visible host content. Never hidden reasoning. */
 payload?:string;
 payloadRef?:string;
};
export type CompanionOptions={
 agent:AgentJoinRequest;
 source:string;
 archive:ArchiveLocalConfig;
};
export interface CompanionOutbox{
 enqueue(sessionId:string,event:SessionArchiveEvent):Promise<void>;
 list(sessionId:string):Promise<SessionArchiveEvent[]>;
 clear(sessionId:string):Promise<void>;
 ack(sessionId:string,eventId:string):Promise<void>;
}
export class MemoryCompanionOutbox implements CompanionOutbox{
 private readonly queued=new Map<string,SessionArchiveEvent[]>();
 async enqueue(sessionId:string,event:SessionArchiveEvent){
  const events=this.queued.get(sessionId)||[];
  if(!events.some(item=>item.eventId===event.eventId))events.push(event);
  this.queued.set(sessionId,events);
 }
 async list(sessionId:string){return [...(this.queued.get(sessionId)||[])];}
 async clear(sessionId:string){this.queued.delete(sessionId);}
 async ack(sessionId:string,eventId:string){
  const current=this.queued.get(sessionId)||[];
  if(!current.length||current[0]!.eventId!==eventId)throw new Error("Unexpected outbox event acknowledgment");
  this.queued.set(sessionId,current.slice(1));
 }
}
/** Prevent common credentials from reaching any configured outbox or backend.
 * Additional policy-specific redaction is required before production use.
 */
export function redactBeforeEgress(input:string):{text:string;redacted:boolean}{
 let redacted=false;
 const patterns=[
  /\bBearer\s+[A-Za-z0-9._~+/-]+/gi,
  /\b(?:sk|ghp|gho|github_pat|glpat|xox[baprs])[-_][A-Za-z0-9_-]{12,}/gi,
  /\b(?:api[_-]?key|access[_-]?token|secret|password|authorization)\s*[:=]\s*["']?[^\s,"'};]+/gi,
  /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----[\s\S]*?-----END (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/g
 ];
 let text=input;
 for(const pattern of patterns)text=text.replace(pattern,()=>{redacted=true;return "[REDACTED]";});
 return {text,redacted};
}
export class CMPCompanion{
 private session:AgentSession|undefined;
 private archiveActive=false;
 private excluded=false;
 private readonly outbox:CompanionOutbox;
 constructor(
  readonly client:CMPClient,
  readonly options:CompanionOptions,
  outbox?:CompanionOutbox
 ){
  this.outbox=outbox||new MemoryCompanionOutbox();
  // A missing durable outbox makes offline capture best effort, not guaranteed.
 }
 get sessionInfo(){return this.session;}
 get archiveEnabled(){return this.archiveActive&&!this.excluded;}
 async start(){
  const session=await this.client.joinAgent(this.options.agent);
  this.session=session;
  const excluded=this.options.archive.excludedSessionIds?.includes(session.sessionId)??false;
  this.excluded=excluded;
  if(this.options.archive.enabled&&!excluded){
   const opening=await this.client.openArchive(session,this.options.source,true,false,this.options.archive.coverage||"partial");
   this.archiveActive=opening.archiving;
  }
  await this.client.subscribeAgents(session);
  return {session,archiving:this.archiveActive};
 }
 async capture(event:HostVisibleEvent):Promise<ArchiveRevision|{skipped:true;reason:string}>{
  if(!this.session||!this.archiveActive||this.excluded)return {skipped:true,reason:"archive-not-enabled"};
  const record:SessionArchiveEvent={
   eventId:event.eventId,kind:event.kind,sequence:event.sequence,occurredAt:event.occurredAt,
   sourceOffset:event.sourceOffset,sourceHash:event.sourceHash,branch:event.branch,
   parentIds:event.parentIds,expectedHead:event.expectedHead,
   contentType:event.contentType||"text/plain",payloadRef:event.payloadRef
  };
  if(this.options.archive.captureRaw && event.payload!==undefined){
   record.payload=redactBeforeEgress(event.payload).text;
  }
  if(record.payload!==undefined&&this.outbox instanceof MemoryCompanionOutbox)
   throw new Error("Raw archiving requires an explicitly configured encrypted durable outbox");
  await this.outbox.enqueue(this.session.sessionId,record);
  try{
   // Never bypass older pending events: archival order follows host event order.
   const pending=await this.outbox.list(this.session.sessionId);
   if(pending[0]?.eventId!==record.eventId)return {skipped:true,reason:"queued-behind-pending-events"};
   const result=await this.client.appendArchive(this.session,record);
   await this.outbox.ack(this.session.sessionId,record.eventId);
   return result;
  }catch(error){
   if(!isRetryableCMPError(error))throw error;
   return {skipped:true,reason:"queued-for-retry"};
  }
 }
 async flush(){
  if(!this.session||!this.archiveActive||this.excluded)return {uploaded:0};
  const pending=await this.outbox.list(this.session.sessionId);
  let uploaded=0;
  for(const event of pending){
   await this.client.appendArchive(this.session,event);
   await this.outbox.ack(this.session.sessionId,event.eventId);
   uploaded++;
  }
  return {uploaded};
 }
 async exclude(){
  this.excluded=true;
  if(!this.session)return {excluded:true};
  await this.outbox.clear(this.session.sessionId);
  if(this.archiveActive){
   const result=await this.client.excludeArchive(this.session);
   this.archiveActive=false;
   return result;
  }
  return {excluded:true};
 }
 async delete(){
  if(!this.session)return {deleted:false};
  await this.outbox.clear(this.session.sessionId);
  if(this.archiveActive){
   const result=await this.client.deleteArchive(this.session);
   this.archiveActive=false;this.excluded=true;
   return result;
  }
  return {deleted:false};
 }
 async stop(){
  if(!this.session)return;
  if(this.archiveActive&&!this.excluded)await this.client.closeArchive(this.session);
  await this.client.leaveAgent(this.session);
  this.archiveActive=false;this.session=undefined;
 }
}
