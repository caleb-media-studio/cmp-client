use serde_json::{json,Value};
use std::{fs,io::{Read,Write},net::TcpListener,path::PathBuf,process::{Command,Output},
 thread,time::{Duration,Instant,SystemTime,UNIX_EPOCH}};
const BIN:&str=env!("CARGO_BIN_EXE_cmp");
fn fixture()->PathBuf {
 let n=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
 let p=std::env::temp_dir().join(format!("cmp-gh-cli-{}-{n}",std::process::id()));
 fs::create_dir_all(&p).unwrap();p
}
fn run(cwd:&std::path::Path,home:&std::path::Path,args:&[&str])->Output {
 Command::new(BIN).current_dir(cwd).env("CMP_HOME",home)
  .env("CMP_DEV_TOKEN","very-long-cmp-development-test-token-123456")
  .env_remove("CMP_CONFIG").env_remove("CMP_ENDPOINT").env_remove("CMP_REPOSITORY")
  .args(args).output().unwrap()
}
fn ok(output:Output)->String{
 assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
 String::from_utf8(output.stdout).unwrap()
}
#[test]
fn gh_style_help_and_local_repo_commands() {
 let home=fixture();
 let root=home.join("project");
 fs::create_dir_all(&root).unwrap();
 for args in [
  vec!["--help"],vec!["repo","--help"],vec!["auth","--help"],
  vec!["config","--help"],vec!["branch","--help"],vec!["help","repo"],
  vec!["api","--help"],
 ] {
  assert!(!ok(run(&root,&home,&args)).is_empty());
 }
 assert!(ok(run(&root,&home,&["--help"])).contains("CORE COMMANDS"));
 ok(run(&root,&home,&["setup","--scope","project","--backend","offline","--repo","owner/test","--no-interactive"]));
 ok(run(&root,&home,&["init","--repo","owner/test","--no-interactive"]));
 assert!(ok(run(&root,&home,&["repo","view"])).contains("owner/test"));
 assert!(ok(run(&root,&home,&["branch","list"])).contains("main"));
 assert!(ok(run(&root,&home,&["api","/v1/repos"])).contains("owner/test"));
 assert!(ok(run(&root,&home,&["config","get","branch"])).contains("main"));
 ok(run(&root,&home,&["config","set","branch","feature"]));
 assert!(ok(run(&root,&home,&["config","get","branch"])).contains("feature"));
 assert!(!root.join(".git").exists());
 let invalid=run(&root,&home,&["api","https://example.com/steal"]);
 assert!(!invalid.status.success());
 let _=fs::remove_dir_all(&home);
}
#[test]
fn clone_imports_consistent_cmp_snapshot_and_never_enables_streaming() {
 let home=fixture();
 let project=home.join("parent");
 fs::create_dir_all(&project).unwrap();
 let listener=TcpListener::bind(("127.0.0.1",0)).unwrap();
 listener.set_nonblocking(true).unwrap();
 let addr=listener.local_addr().unwrap();
 let sha="a".repeat(64);
 let record=json!({"id":"fact:one","kind":"fact","content":"Imported remote CMP context",
  "sourceRefs":[],"status":"approved","authorId":"fixture",
  "updatedAt":"2026-11-16T12:00:00Z"});
 let bodies=[
  json!({"protocol":"cmp","transportVersion":1,"repository":"local/example",
   "defaultBranch":"main","head":sha,"capabilities":{"refs":true,"snapshot":true,"commit":true},
   "links":{"refs":"/local/example.cmp?view=refs","snapshot":"/local/example.cmp?view=snapshot&ref=main"}}),
  json!({"repository":"local/example","ref":"main","head":sha,"records":[record]})
 ];
 let worker=thread::spawn(move || {
  let mut requests=Vec::new();
  for body in bodies {
   let deadline=Instant::now()+Duration::from_secs(10);
   let (mut conn,_)=loop{
    match listener.accept(){
     Ok(value)=>break value,
     Err(e) if e.kind()==std::io::ErrorKind::WouldBlock&&Instant::now()<deadline=>
      thread::sleep(Duration::from_millis(10)),
     Err(e)=>panic!("CMP .cmp request missing: {e}")
    }
   };
   conn.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
   let mut buffer=[0u8;4096];
   let n=conn.read(&mut buffer).unwrap();
   assert!(n>0);
   let request=String::from_utf8_lossy(&buffer[..n]).to_string();
   assert!(request.contains("Authorization: Bearer very-long-cmp-development-test-token-123456")
      || request.to_ascii_lowercase().contains("authorization: bearer very-long-cmp-development-test-token-123456"));
   requests.push(request.lines().next().unwrap_or("").to_owned());
   let bytes=body.to_string();
   write!(conn,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
    bytes.len(),bytes).unwrap();conn.flush().unwrap();
  }
  requests
 });
 let endpoint=format!("http://{addr}");
 ok(run(&project,&home,&["setup","--scope","project","--backend","local","--endpoint",&endpoint,
    "--repo","local/example","--no-interactive"]));
 let stdout=ok(run(&project,&home,&["repo","clone","local/example","cloned"]));
 assert!(stdout.contains("Cloned CMP snapshot"));
 let calls=worker.join().unwrap();
 assert_eq!(calls,vec!["GET /local/example.cmp HTTP/1.1",
     "GET /local/example.cmp?view=snapshot&ref=main HTTP/1.1"]);
 let dest=project.join("cloned");
 let config=fs::read_to_string(dest.join(".cmp/config.toml")).unwrap();
 assert!(config.contains("backend = \"offline\""));
 assert!(config.contains("streaming = false"));
 assert!(dest.join(".cmp/state/stream-outbox.json").exists());
 assert!(!dest.join(".git").exists());
 let snapshot:Value=serde_json::from_str(&ok(run(&dest,&home,&["snapshot"]))).unwrap();
 assert_eq!(snapshot["records"][0]["content"],"Imported remote CMP context");
 let duplicate=run(&project,&home,&["repo","clone","local/example","cloned"]);
 assert!(!duplicate.status.success());
 let _=fs::remove_dir_all(&home);
}
