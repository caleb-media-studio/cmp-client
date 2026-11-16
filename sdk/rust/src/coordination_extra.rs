//! Additional typed helpers for all v0.1 CMP coordination/session lifecycle calls.
use crate::Client;
use crate::coordination::AgentSession;
use serde_json::{json,Value};
use std::error::Error;

impl Client {
 /// Bind every scoped call to a specific agent session. The call's arguments
 /// cannot override the host-managed session identity.
 pub fn scoped_rpc(&self,method:&str,session:&AgentSession,params:Value)->Result<Value,Box<dyn Error>> {
  let mut obj=match params {
   Value::Object(values)=>values,
   _=>return Err("CMP method arguments must be a JSON object".into())
  };
  if obj.contains_key("sessionId")||obj.contains_key("sessionToken"){
   return Err("CMP identity fields cannot be overridden".into());
  }
  obj.insert("sessionId".into(),json!(session.session_id));
  obj.insert("sessionToken".into(),json!(session.session_token));
  self.rpc(method,Value::Object(obj))
 }
 pub fn agent_events(&self,s:&AgentSession,after:&str)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("agent.events",s,json!({"after":after}))
 }
 pub fn heartbeat_agent(&self,s:&AgentSession,ttl_ms:u64)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("agent.heartbeat",s,json!({"ttlMs":ttl_ms}))
 }
 pub fn leave_agent(&self,s:&AgentSession)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("agent.leave",s,json!({}))
 }
 pub fn send_message(&self,s:&AgentSession,message:&str,to_session:Option<&str>)->Result<Value,Box<dyn Error>>{
  let mut params=json!({"message":message});
  if let Some(to)=to_session{params["toSessionId"]=json!(to);}
  self.scoped_rpc("coordination.message",s,params)
 }
 pub fn revise_contract(&self,s:&AgentSession,id:&str,objective:&str,scopes:Value)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("coordination.revise",s,json!({"contractId":id,"objective":objective,"ownershipScopes":scopes}))
 }
 pub fn reject_contract(&self,s:&AgentSession,id:&str)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("coordination.reject",s,json!({"contractId":id}))
 }
 pub fn get_contract(&self,s:&AgentSession,id:&str)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("coordination.contract.get",s,json!({"contractId":id}))
 }
 pub fn renew_claim(&self,s:&AgentSession,id:&str,fencing_token:u64,duration_ms:u64)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("coordination.claim.renew",s,json!({
   "claimId":id,"fencingToken":fencing_token,"durationMs":duration_ms}))
 }
 pub fn release_claim(&self,s:&AgentSession,id:&str,fencing_token:u64)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("coordination.claim.release",s,json!({"claimId":id,"fencingToken":fencing_token}))
 }
 pub fn propose_change(&self,s:&AgentSession,id:&str,fence:u64,path:&str,git_revision:&str)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("coordination.change.propose",s,json!({
   "claimId":id,"fencingToken":fence,"path":path,"gitRevision":git_revision}))
 }
 pub fn archive_policy(&self,s:&AgentSession)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("session.policy.get",s,json!({}))
 }
 pub fn archive_status(&self,s:&AgentSession)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("session.get",s,json!({}))
 }
 pub fn archive_events(&self,s:&AgentSession,include_raw:bool)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("session.events",s,json!({"includeRaw":include_raw}))
 }
 pub fn checkpoint_archive(&self,s:&AgentSession,event:Value)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("session.checkpoint",s,event)
 }
 pub fn close_archive(&self,s:&AgentSession)->Result<Value,Box<dyn Error>>{
  self.scoped_rpc("session.close",s,json!({}))
 }
 pub fn resume_agent(&self,s:&AgentSession,agent_id:&str,provider:&str,source_repo:&str)->Result<AgentSession,Box<dyn Error>>{
  let response=self.rpc("agent.join",json!({
   "organizationId":s.organization_id,"repositoryId":s.repository_id,
   "agentId":agent_id,
   "sourceRepository":{"provider":provider,"repositoryId":source_repo},
   "resumeSessionId":s.session_id,"resumeSessionToken":s.session_token
  }))?;
  Ok(serde_json::from_value(response)?)
 }
 /// Development administrator operation; production requires separate RBAC.
 pub fn configure_archive_policy(&self,org:&str,repo:&str,enabled:bool,raw:bool,retention:u64)->Result<Value,Box<dyn Error>>{
  self.rpc("session.policy.set",json!({
   "organizationId":org,"repositoryId":repo,"enabled":enabled,
   "rawEnabled":raw,"retentionDays":retention
  }))
 }
}
