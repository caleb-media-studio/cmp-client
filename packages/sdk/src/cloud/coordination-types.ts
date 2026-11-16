/** CMP Agent Coordination proposal v0.1. Development-server wire contract. */
export type SourceRepository = { provider:string; repositoryId:string };
export type AgentJoinRequest = {
 organizationId:string; repositoryId:string; agentId:string;
 sourceRepository:SourceRepository; intent?:string; paths?:string[]; ttlMs?:number;
 /** Host-initiated continuation of an expired lease using an encrypted client credential. */
 resumeSessionId?:string;resumeSessionToken?:string;
};
export type AgentSession = {
 sessionId:string; sessionToken:string; repositoryId:string; organizationId:string;
 canonicalSourceKey:string; peers:AgentPeer[]; expiresInMs:number;
 authorization:"development-only";resumed?:boolean;
};
export type AgentPeer = {sessionId:string;agentId:string;intent:string;paths:string[];expiresAt?:string};
export type AgentCredentials = {sessionId:string;sessionToken:string};
export type WorkScopes = Record<string,string[]>;
export type WorkContractProposal = AgentCredentials & {
 objective:string;participants:string[];ownershipScopes:WorkScopes;ttlMs?:number;
};
export type WorkContract = {contractId:string;revision:number;status:string;participants:string[];ownershipScopes:WorkScopes};
export type ClaimAcquireRequest = AgentCredentials & {
 contractId:string;scope:string;durationMs?:number;
};
export type WorkClaim = {claimId:string;scope:string;fencingToken:number;expiresAt:string;enforcement:"controlled-CMP-operations-only"};
export type AgentCoordinationEvent = {
 v:1;id:string;eventId:string;cursor:string;type:"event";
 method:string;params:Record<string,unknown>&{organizationId:string;repositoryId:string};
};
export const COORDINATION_METHODS = [
 "agent.join","agent.list","agent.events","agent.heartbeat","agent.intent.publish","agent.leave",
 "coordination.message","coordination.propose","coordination.revise","coordination.accept","coordination.reject",
 "coordination.contract.get","coordination.claim.acquire","coordination.claim.renew",
 "coordination.claim.release","coordination.change.propose"
] as const;
