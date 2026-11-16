use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const CLI: &str = env!("CARGO_BIN_EXE_cmp");
const TOKEN: &str = "m1-local-core-test-token-0123456789";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let n=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let seq=SEQUENCE.fetch_add(1,Ordering::Relaxed);
        let dir=std::env::temp_dir().join(format!("cmp-doctor-{}-{n}-{seq}",std::process::id()));
        fs::create_dir(&dir).unwrap();
        fs::create_dir(dir.join("project")).unwrap();
        Self(dir)
    }
    fn project(&self)->PathBuf {self.0.join("project")}
    fn configure(&self,backend:&str,endpoint:&str,repo:Option<&str>){
        let dir=self.project().join(".cmp");
        fs::create_dir_all(&dir).unwrap();
        let repository=repo.map(|id|format!("repository={id:?}\n")).unwrap_or_default();
        fs::write(dir.join("config.toml"),
          format!("version=1\nbackend={backend:?}\nendpoint={endpoint:?}\nbranch='main'\n{repository}")).unwrap();
    }
    fn command(&self,args:&[&str])->Output {
        Command::new(CLI).args(args).current_dir(self.project())
            .env("CMP_HOME",&self.0).env("CMP_DEV_TOKEN",TOKEN)
            .env_remove("CMP_CONFIG").env_remove("CMP_ENDPOINT").env_remove("CMP_REPOSITORY")
            .output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {let _=fs::remove_dir_all(&self.0);}
}
struct Reply {path:&'static str,status:u16,body:Value}
fn server(replies:Vec<Reply>)->(String,thread::JoinHandle<Vec<String>>){
    let listener=TcpListener::bind(("127.0.0.1",0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint=format!("http://{}",listener.local_addr().unwrap());
    let worker=thread::spawn(move||{
        let mut paths=Vec::new();
        for reply in replies {
            let deadline=Instant::now()+Duration::from_secs(8);
            let (mut conn,_)=loop {
                match listener.accept() {
                    Ok(v)=>break v,
                    Err(e) if e.kind()==std::io::ErrorKind::WouldBlock&&Instant::now()<deadline=>
                        thread::sleep(Duration::from_millis(10)),
                    Err(e)=>panic!("CMP doctor did not issue expected Core request: {e}"),
                }
            };
            conn.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut data=Vec::new();
            loop {
                let mut buf=[0u8;4096];
                let read=conn.read(&mut buf).unwrap();
                assert!(read>0);
                data.extend_from_slice(&buf[..read]);
                if data.windows(4).any(|w|w==b"\r\n\r\n"){break;}
            }
            let request=String::from_utf8_lossy(&data);
            let line=request.lines().next().unwrap().to_owned();
            assert!(line.contains(reply.path),"Expected path {}, got {line}",reply.path);
            assert!(request.to_ascii_lowercase().contains(&format!(
                "authorization: bearer {TOKEN}").to_ascii_lowercase()),
                "Core request missing expected bearer token");
            paths.push(line);
            let payload=reply.body.to_string();
            let reason=if reply.status==401{"Unauthorized"}else{"OK"};
            write!(conn,"HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                reply.status,reason,payload.len(),payload).unwrap();
            conn.flush().unwrap();
        }
        paths
    });
    (endpoint,worker)
}
fn core_caps()->Value {json!({"protocolVersion":1,"capabilities":{
    "listRepositories":true,"getHead":true,"getSnapshot":true,"createBranch":true,
    "commit":true,"compareAndSwap":true,"diff":true,"merge":true,
    "websocket":false,"agentCoordination":false,"sessionArchive":false
}})}
fn caps()->Reply {Reply{path:"GET /v1/capabilities ",status:200,body:core_caps()}}
fn repos(body:Value,status:u16)->Reply{
    Reply{path:"GET /v1/repos ",status,body}
}
#[test]
fn doctor_fails_when_authenticated_repository_api_returns_unauthorized(){
    let t=Fixture::new();
    let (endpoint,worker)=server(vec![caps(),repos(json!({"error":"Unauthorized"}),401)]);
    t.configure("local",&endpoint,None);
    let result=t.command(&["doctor"]);
    assert!(!result.status.success(),"Unauthorized Core API must fail doctor");
    let stdout=String::from_utf8_lossy(&result.stdout);
    assert!(String::from_utf8_lossy(&result.stderr).contains("Authenticated repository API: unavailable"));
    assert!(!stdout.contains("Authenticated repository API: connected"));
    assert!(!String::from_utf8_lossy(&result.stderr).contains(TOKEN));
    assert_eq!(worker.join().unwrap().len(),2);
}
#[test]
fn doctor_rejects_invalid_capabilities_before_querying_repo(){
    let t=Fixture::new();
    let (endpoint,worker)=server(vec![Reply{path:"GET /v1/capabilities ",
        status:200,body:json!({"capabilities":{"listRepositories":true}})}]);
    t.configure("local",&endpoint,None);
    let out=t.command(&["doctor"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("required Core capability"));
    assert_eq!(worker.join().unwrap().len(),1);
}
#[test]
fn doctor_checks_selected_repository_and_branch_head(){
    let t=Fixture::new();
    let (endpoint,worker)=server(vec![
        caps(),
        repos(json!({"repositories":["owner/context"]}),200),
        Reply{path:"GET /v1/repos/owner%2Fcontext/head?branch=main ",status:200,
            body:json!({"head":"a".repeat(64)})}
    ]);
    t.configure("local",&endpoint,Some("owner/context"));
    let out=t.command(&["doctor"]);
    assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains(
        "Selected repository: owner/context (branch main accessible)"));
    assert_eq!(worker.join().unwrap().len(),3);
}
#[test]
fn doctor_fails_when_selected_repository_is_not_accessible(){
    let t=Fixture::new();
    let (endpoint,worker)=server(vec![caps(),repos(json!({"repositories":[]}),200)]);
    t.configure("local",&endpoint,Some("owner/context"));
    let out=t.command(&["doctor"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not accessible"));
    assert_eq!(worker.join().unwrap().len(),2);
}
#[test]
fn doctor_offline_flag_skips_remote_and_does_not_claim_local_storage(){
    let t=Fixture::new();
    t.configure("local","http://127.0.0.1:9",None);
    let out=t.command(&["doctor","--offline"]);
    assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
    let text=String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Remote checks: skipped"),"{text}");
    assert!(!text.contains("Local context store: available"));
    assert!(!t.project().join(".cmp/state").exists());
}
#[test]
fn offline_doctor_checks_local_repository_binding_without_remote_calls(){
    let t=Fixture::new();
    t.configure("offline","https://cloud.cmp.chat",Some("owner/context"));
    let absent=t.command(&["doctor"]);
    assert!(!absent.status.success(),"Missing local repo should fail doctor");
    assert!(String::from_utf8_lossy(&absent.stderr).contains("Run cmp init"));
    let init=t.command(&["init","--repo","owner/context","--no-interactive"]);
    assert!(init.status.success(),"{}",String::from_utf8_lossy(&init.stderr));
    let checked=t.command(&["doctor"]);
    assert!(checked.status.success(),"{}",String::from_utf8_lossy(&checked.stderr));
    assert!(String::from_utf8_lossy(&checked.stdout).contains(
        "Selected repository: owner/context (branch main accessible)"));
}

#[test]
fn doctor_does_not_accept_empty_selected_branch_head(){
    let t=Fixture::new();
    let (endpoint,worker)=server(vec![
        caps(),
        repos(json!({"repositories":["owner/context"]}),200),
        Reply{path:"GET /v1/repos/owner%2Fcontext/head?branch=main ",status:200,
            body:json!({"head":""})}
    ]);
    t.configure("local",&endpoint,Some("owner/context"));
    let out=t.command(&["doctor"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no branch head"));
    assert_eq!(worker.join().unwrap().len(),3);
}
#[test]
fn cloud_offline_diagnostics_never_load_credentials_or_use_the_network(){
    let t=Fixture::new();
    t.configure("cloud","https://cloud.cmp.chat",None);
    let out=t.command(&["doctor","--offline"]);
    assert!(out.status.success(),"{}",String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Remote checks: skipped"));
    assert!(!t.0.join(".cmp/credential-store").exists());
    assert!(!t.project().join(".cmp/state").exists());
}
