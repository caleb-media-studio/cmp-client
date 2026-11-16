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
  // Never follow redirects with the CMP bearer token across origins.
  let agent=ureq::AgentBuilder::new().redirects(0).build();
  let req=agent.request(method,&url).set("Authorization",&format!("Bearer {}",self.token)).set("Content-Type","application/json");
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

/// Stable, versioned CMP JSON repository transport. This is not Git smart HTTP.
pub fn cmp_transport_path(repo:&str)->Result<String,Box<dyn Error>> {
    let parts:Vec<&str>=repo.split('/').collect();
    if parts.len()!=2 || parts.iter().any(|part| part.is_empty()||part.len()>100
        || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || *part=="." || *part=="..") {
        return Err("CMP repository must have owner/name format".into());
    }
    Ok(format!("/{}/{}.cmp",parts[0],parts[1]))
}
/// Parse a CMP remote URL without ever accepting a different origin than the
/// configured server, preventing redirected/cross-host bearer credential leaks.
pub fn cmp_repository_from_url(address:&str,endpoint:&str)->Result<String,Box<dyn Error>> {
    let url=url::Url::parse(address)?;
    let base=url::Url::parse(endpoint)?;
    if url.origin()!=base.origin() || url.username()!="" || url.password().is_some()
        || url.query().is_some() || url.fragment().is_some() {
        return Err("CMP clone URL must match the configured server and contain no query or credentials".into());
    }
    let segments=url.path_segments().ok_or("Invalid CMP remote URL")?.collect::<Vec<_>>();
    if segments.len()!=2 || !segments[1].ends_with(".cmp") {
        return Err("Expected https://server/owner/repository.cmp".into());
    }
    let owner=percent_encoding::percent_decode_str(segments[0]).decode_utf8()?;
    let name=percent_encoding::percent_decode_str(&segments[1][..segments[1].len()-4]).decode_utf8()?;
    let repo=format!("{owner}/{name}");
    cmp_transport_path(&repo)?;
    Ok(repo)
}
impl Client {
    pub fn cmp_remote_url(&self,repo:&str)->Result<String,Box<dyn Error>>{
        Ok(self.endpoint.clone()+&cmp_transport_path(repo)?)
    }
    pub fn cmp_manifest(&self,repo:&str)->Result<Value,Box<dyn Error>>{
        self.request("GET",&cmp_transport_path(repo)?,None)
    }
    pub fn cmp_refs(&self,repo:&str)->Result<Value,Box<dyn Error>>{
        self.request("GET",&format!("{}?view=refs",cmp_transport_path(repo)?),None)
    }
    pub fn cmp_snapshot(&self,repo:&str,reference:&str)->Result<Value,Box<dyn Error>>{
        if reference.is_empty() || reference.len()>128 || reference.bytes().any(|b| b.is_ascii_control()) {
            return Err("Invalid CMP reference".into());
        }
        let refencoded=url::form_urlencoded::byte_serialize(reference.as_bytes()).collect::<String>();
        self.request("GET",&format!("{}?view=snapshot&ref={refencoded}",cmp_transport_path(repo)?),None)
    }
    pub fn cmp_push(&self,repo:&str,payload:Value)->Result<Value,Box<dyn Error>>{
        self.request("POST",&cmp_transport_path(repo)?,Some(payload))
    }
}
#[cfg(test)]
mod transport_tests {
 use super::*;
 #[test] fn transport_paths_and_url_origin_are_checked(){
  assert_eq!(cmp_transport_path("team/repo-first").unwrap(),"/team/repo-first.cmp");
  for bad in ["team/../repo","team/repo/x","/repo","team/.","team/.."] {
   assert!(cmp_transport_path(bad).is_err(),"{bad}");
  }
  let base="https://cloud.cmp.chat";
  assert_eq!(cmp_repository_from_url("https://cloud.cmp.chat/team/repo-first.cmp",base).unwrap(),"team/repo-first");
  for bad in ["https://evil.example/team/repo-first.cmp",
   "https://cloud.cmp.chat@evil.example/team/repo-first.cmp",
   "https://cloud.cmp.chat/team/repo-first.cmp?token=x",
   "https://cloud.cmp.chat/team/repo-first.git"] {
   assert!(cmp_repository_from_url(bad,base).is_err(),"{bad}");
  }
 }
}
