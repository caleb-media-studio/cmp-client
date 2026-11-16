//! CMP SDK: versioned context HTTP client and native WebSocket event feed.
//! Pre-alpha development-only API; production authorization is not yet implemented server-side.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::error::Error;

#[derive(Clone)]
pub struct Client { pub endpoint: String, pub token: String }
#[derive(Debug, Serialize, Deserialize)]
pub struct Record {
 pub id: String, pub kind: String, pub content: String,
 #[serde(rename="sourceRefs")] pub source_refs: Vec<String>,
 pub status: String,
 #[serde(rename="authorId")] pub author_id: String,
 #[serde(rename="updatedAt")] pub updated_at: String
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Change { pub op: String, pub record: Option<Record>, pub id: Option<String> }
// A CMP context repository is one URL path segment, even when named owner/repo.
fn encode_repo(repo: &str) -> String {
 url::form_urlencoded::byte_serialize(repo.as_bytes()).collect()
}
impl Client {
 pub fn new(endpoint: impl Into<String>, token: impl Into<String>) -> Self {
  Self{ endpoint:endpoint.into().trim_end_matches('/').to_owned(), token:token.into() }
 }
 pub fn request(&self,method:&str,path:&str,body:Option<Value>)->Result<Value,Box<dyn Error>> {
  let url=format!("{}{}",self.endpoint,path);
  let req=ureq::request(method,&url).set("Authorization",&format!("Bearer {}",self.token)).set("Content-Type","application/json");
  let response=if let Some(value)=body{req.send_json(value)?}else{req.call()?};
  Ok(response.into_json::<Value>()?)
 }
 pub fn list_repositories(&self)->Result<Value,Box<dyn Error>>{self.request("GET","/v1/repos",None)}
 pub fn create_repository(&self,id:&str)->Result<Value,Box<dyn Error>>{
  self.request("POST","/v1/repos",Some(json!({"id":id})))
 }
 pub fn head(&self,repo:&str,branch:&str)->Result<Value,Box<dyn Error>>{
  self.request("GET",&format!("/v1/repos/{}/head?branch={}",encode_repo(repo),url::form_urlencoded::byte_serialize(branch.as_bytes()).collect::<String>()),None)
 }
 pub fn snapshot(&self,repo:&str,ref_id:&str)->Result<Value,Box<dyn Error>>{
  self.request("GET",&format!("/v1/repos/{}/records?ref={}",encode_repo(repo),url::form_urlencoded::byte_serialize(ref_id.as_bytes()).collect::<String>()),None)
 }
 pub fn commit(&self,repo:&str,payload:Value)->Result<Value,Box<dyn Error>>{
  self.request("POST",&format!("/v1/repos/{}/commits",encode_repo(repo)),Some(payload))
 }
 pub fn merge(&self,repo:&str,source:&str,target:&str,author:&str)->Result<Value,Box<dyn Error>>{
  self.request("POST",&format!("/v1/repos/{}/merge",encode_repo(repo)),Some(json!({"source":source,"target":target,"authorId":author})))
 }
 pub fn connect(&self)->Result<tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,Box<dyn Error>>{
  let data=self.request("POST","/v1/ws-ticket",Some(json!({})))?;
  let ticket=data["ticket"].as_str().ok_or("ticket missing")?;
  let base=if self.endpoint.starts_with("https://"){self.endpoint.replacen("https://","wss://",1)}else if self.endpoint.starts_with("http://"){self.endpoint.replacen("http://","ws://",1)}else{return Err("invalid endpoint".into())};
  if base.starts_with("ws://")&&!base.starts_with("ws://localhost")&&!base.starts_with("ws://127.0.0.1"){return Err("CMP requires WSS outside localhost".into())}
  let (socket,_response)=tungstenite::connect(format!("{base}/v1/cmp/connect?ticket={ticket}"))?;
  Ok(socket)
 }
}
#[cfg(test)]
mod tests{
 use super::*;
 #[test] fn namespaced_repo_is_one_encoded_path_segment(){
  assert_eq!(encode_repo("team/knowledge"),"team%2Fknowledge");
  assert_eq!(encode_repo("user/docs"),"user%2Fdocs");
 }
 #[test] fn model_roundtrip(){
  let value=json!({"id":"a","kind":"fact","content":"Test","sourceRefs":[],"status":"proposed","authorId":"agent","updatedAt":"2026"});
  let record:Record=serde_json::from_value(value.clone()).unwrap();
  assert_eq!(serde_json::to_value(record).unwrap(),value);
 }
}

pub mod coordination;

pub mod coordination_extra;
