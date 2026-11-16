import { createHash } from "node:crypto";
import type { SessionArchiveEvent, SessionEventKind } from "./cloud/archive-types.js";
import { redactBeforeEgress } from "./companion.js";

export type HostKind="claude-code"|"codex"|"opencode"|"generic";
export type HookInput=Record<string,unknown>;
export interface NormalizedHook {
  sourceSessionId:string;
  event:SessionArchiveEvent;
  ended:boolean;
  /** Explicitly partial if the host cannot provide a stable event offset. */
  coverage:"partial"|"complete";
}
const eventKinds:Record<string,SessionEventKind>={
 "SessionStart":"session.boundary",
 "UserPromptSubmit":"user.message",
 "PreToolUse":"tool.call",
 "PostToolUse":"tool.result",
 "PreCompact":"session.checkpoint",
 "PostCompact":"session.boundary",
 "Stop":"session.boundary",
 "SessionEnd":"session.boundary",
 "Interrupt":"session.boundary",
 "session.created":"session.boundary",
 "session.compacted":"session.checkpoint",
 "session.idle":"session.boundary",
 "session.updated":"session.boundary",
 "message.updated":"assistant.visible",
 "tool.execute.before":"tool.call",
 "tool.execute.after":"tool.result"
};
function unwrap(host:HostKind,payload:HookInput):HookInput{
 if(host==="opencode" && payload.properties && typeof payload.properties==="object"
   && !Array.isArray(payload.properties))
  return {...(payload.properties as HookInput),...payload};
 return payload;
}
export function normalizeHostHook(host:HostKind,payload:HookInput,{
  captureRaw=false,sequence=0
}: {captureRaw?:boolean;sequence?:number}={}):NormalizedHook|null{
 const data=unwrap(host,payload);
 const rawKind=data.hook_event_name||data.eventType||data.type||data.event;
 if(typeof rawKind!=="string")return null;
 const kind=eventKinds[rawKind];
 if(!kind)return null;
 const sourceSessionId=data.session_id||data.sessionId||data.sessionID||data.id;
 if(typeof sourceSessionId!=="string"||sourceSessionId.length<3||sourceSessionId.length>200)return null;
 // Only content made visible to this specific hook is captured.
 let visibleContent:string|undefined;
 if(rawKind==="UserPromptSubmit"&&typeof data.prompt==="string")visibleContent=data.prompt;
 if(rawKind==="PreToolUse"){
  const name=typeof data.tool_name==="string"?data.tool_name:"tool";
  visibleContent=JSON.stringify({name,arguments:data.tool_input??null});
 }
 if(rawKind==="PostToolUse"||rawKind==="tool.execute.after"){
  visibleContent=JSON.stringify({tool:data.tool_name??data.tool??null,result:data.tool_response??data.output??null});
 }
 if(rawKind==="message.updated"&&typeof data.visibleText==="string")visibleContent=data.visibleText;
 const stableSourceId=data.event_id||data.eventId||data.turn_id||data.part_id||data.offset;
 const stable=typeof stableSourceId==="string"||typeof stableSourceId==="number";
 // A retried hook with a stable host event ID must have the same archive
 // identity, regardless of local hook invocation counter.
 const base=JSON.stringify({host,sourceSessionId,kind,rawKind,
  sourceEventId:stable?String(stableSourceId):undefined,
  ...(!stable?{sequence}:{})});
 const eventId=host+":"+createHash("sha256").update(base).digest("hex");
 const event:SessionArchiveEvent={
  eventId,kind,sequence,occurredAt:typeof data.timestamp==="string"&&!Number.isNaN(Date.parse(data.timestamp))
   ?data.timestamp:new Date().toISOString(),
  sourceOffset:stable?String(stableSourceId):undefined,
  contentType:"application/json",
  visibility:"organization"
 };
 if(captureRaw&&visibleContent!==undefined)
  event.payload=redactBeforeEgress(visibleContent).text;
 const ended=rawKind==="SessionEnd"||rawKind==="session.deleted";
 return {sourceSessionId,event,ended,coverage:"partial"};
}
