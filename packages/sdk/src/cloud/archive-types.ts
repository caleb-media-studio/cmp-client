/** Session archive DAG: org policy AND local companion config must both permit capture. */
export type ArchivePolicy = {
 organizationId:string; repositoryId:string;enabled:boolean;rawEnabled:boolean;
 retentionDays:number;version:number;legalHold:boolean;
};
export type ArchiveLocalConfig = {
 enabled:boolean;
 /** Local session IDs excluded from capture. */
 excludedSessionIds?:string[];
 /** Default is partial if host events do not cover the complete transcript. */
 coverage?:"partial"|"complete";
 /** Raw payloads require explicit server policy and archive key. */
 captureRaw?:boolean;
};
export type SessionEventKind =
 "user.message"|"assistant.visible"|"tool.call"|"tool.result"|
 "session.boundary"|"session.checkpoint"|"context.reference"|"coordination.event";
export type SessionArchiveEvent = {
 eventId:string;kind:SessionEventKind;sequence:number;occurredAt:string;
 sourceOffset?:string;sourceHash?:string;branch?:string;expectedHead?:string;
 parentIds?:string[];visibility?:"private"|"organization";contentType?:string;
 payloadRef?:string;payload?:string;
};
export type ArchiveRevision = {
 accepted:boolean;duplicate:boolean;eventId?:string;revisionId:string;
 parents?:string[];branch?:string;redacted?:boolean;policyVersion?:number;
};
export type ArchiveStatus = {
 sessionId:string;organizationId:string;repositoryId:string;source:string;
 coverage:"partial"|"complete";eventCount:number;
 branches:{branch:string;revision_id:string}[];closed:boolean;policyVersion:number;
};
export const SESSION_METHODS=[
 "session.policy.set","session.policy.get","session.open","session.append","session.checkpoint",
 "session.get","session.events","session.close","session.exclude", "session.delete","session.admin.delete","session.maintenance.sweep"
] as const;
