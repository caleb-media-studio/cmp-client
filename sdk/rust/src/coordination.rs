//! Development-only CMP agent coordination and session archive RPC.
//! Work claims protect CMP-mediated actions, not arbitrary local shell writes.
use crate::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::error::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRepository {
 pub provider: String,
 pub repository_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentJoinRequest {
 pub organization_id: String,
 pub repository_id: String,
 pub agent_id: String,
 pub source_repository: SourceRepository,
 pub intent: String,
 pub paths: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPeer {
 pub session_id: String,
 pub agent_id: String,
 pub intent: String,
 pub paths: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
 pub session_id: String,
 pub session_token: String,
 pub organization_id: String,
 pub repository_id: String,
 pub canonical_source_key: String,
 pub peers: Vec<AgentPeer>,
 pub expires_in_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkClaim {
 pub claim_id: String,
 pub scope: String,
 pub fencing_token: u64,
 pub expires_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEvent {
 pub event_id: String,
 pub kind: String,
 pub sequence: u64,
 pub occurred_at: String,
 #[serde(skip_serializing_if="Option::is_none")]
 pub source_offset: Option<String>,
 #[serde(skip_serializing_if="Option::is_none")]
 pub source_hash: Option<String>,
 #[serde(skip_serializing_if="Option::is_none")]
 pub branch: Option<String>,
 #[serde(skip_serializing_if="Option::is_none")]
 pub payload_ref: Option<String>,
 #[serde(skip_serializing_if="Option::is_none")]
 pub payload: Option<String>,
}
impl Client {
 pub fn rpc(&self,method:&str,params:Value)->Result<Value,Box<dyn Error>>{
  let result=self.request("POST","/v1/rpc",Some(json!({"method":method,"params":params})))?;
  Ok(result.get("result").ok_or("CMP RPC result missing")?.clone())
 }
 pub fn join_agent(&self,request:AgentJoinRequest)->Result<AgentSession,Box<dyn Error>>{
  Ok(serde_json::from_value(self.rpc("agent.join",serde_json::to_value(request)?)?)?)
 }
 pub fn list_agents(&self,session:&AgentSession)->Result<Value,Box<dyn Error>>{
  self.rpc("agent.list",json!({"sessionId":session.session_id,"sessionToken":session.session_token}))
 }
 pub fn publish_intent(&self,session:&AgentSession,intent:&str,paths:Vec<String>)->Result<Value,Box<dyn Error>>{
  self.rpc("agent.intent.publish",json!({
   "sessionId":session.session_id,"sessionToken":session.session_token,"intent":intent,"paths":paths
  }))
 }
 pub fn propose_contract(&self,session:&AgentSession,objective:&str,participants:Vec<String>,scopes:Value)->Result<Value,Box<dyn Error>>{
  self.rpc("coordination.propose",json!({
   "sessionId":session.session_id,"sessionToken":session.session_token,
   "objective":objective,"participants":participants,"ownershipScopes":scopes
  }))
 }
 pub fn accept_contract(&self,session:&AgentSession,contract_id:&str,revision:u64)->Result<Value,Box<dyn Error>>{
  self.rpc("coordination.accept",json!({
   "sessionId":session.session_id,"sessionToken":session.session_token,
   "contractId":contract_id,"revision":revision
  }))
 }
 pub fn acquire_claim(&self,session:&AgentSession,contract_id:&str,scope:&str)->Result<WorkClaim,Box<dyn Error>>{
  let result=self.rpc("coordination.claim.acquire",json!({
   "sessionId":session.session_id,"sessionToken":session.session_token,
   "contractId":contract_id,"scope":scope
  }))?;
  Ok(serde_json::from_value(result)?)
 }
 pub fn open_archive(&self,session:&AgentSession,source:&str,local_enabled:bool,exclude:bool)->Result<Value,Box<dyn Error>>{
  self.rpc("session.open",json!({
   "sessionId":session.session_id,"sessionToken":session.session_token,
   "source":source,"archiveEnabled":local_enabled,"exclude":exclude
  }))
 }
 pub fn append_archive(&self,session:&AgentSession,event:ArchiveEvent)->Result<Value,Box<dyn Error>>{
  let mut payload=serde_json::to_value(event)?;
  payload["sessionId"]=json!(session.session_id);
  payload["sessionToken"]=json!(session.session_token);
  self.rpc("session.append",payload)
 }
 pub fn delete_archive(&self,session:&AgentSession)->Result<Value,Box<dyn Error>>{
  self.rpc("session.delete",json!({
   "sessionId":session.session_id,"sessionToken":session.session_token
  }))
 }
 pub fn exclude_archive(&self,session:&AgentSession)->Result<Value,Box<dyn Error>>{
  self.rpc("session.exclude",json!({
   "sessionId":session.session_id,"sessionToken":session.session_token
  }))
 }
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test] fn agent_and_archive_model_roundtrip(){
  let request=AgentJoinRequest{
   organization_id:"team".into(),repository_id:"repo".into(),agent_id:"coder".into(),
   source_repository:SourceRepository{provider:"github".into(),repository_id:"123".into()},
   intent:"review".into(),paths:vec!["src/**".into()]
  };
  assert_eq!(serde_json::to_value(request).unwrap()["sourceRepository"]["repositoryId"],"123");
  let event=ArchiveEvent{event_id:"e".into(),kind:"assistant.visible".into(),sequence:1,
   occurred_at:"2026-11-13T16:00:00Z".into(),source_offset:None,source_hash:None,
   branch:None,payload_ref:None,payload:None};
  assert!(serde_json::to_value(event).unwrap().get("payload").is_none());
 }
}
