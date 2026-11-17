use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};
use std::sync::atomic::{AtomicU64, Ordering};
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const CLI: &str = env!("CARGO_BIN_EXE_cmp");
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence=FIXTURE_SEQUENCE.fetch_add(1,Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("cmp-offline-{}-{n}-{sequence}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn project(&self) -> PathBuf {
        self.0.join("project")
    }
    fn command(&self, args: &[&str]) -> Output {
        fs::create_dir_all(self.project()).unwrap();
        Command::new(CLI)
            .current_dir(self.project())
            .env("CMP_HOME", &self.0)
            .env_remove("CMP_CONFIG")
            .env_remove("CMP_ENDPOINT")
            .env_remove("CMP_REPOSITORY")
            .args(args)
            .output()
            .unwrap()
    }
    fn check(&self, args: &[&str]) -> String {
        let output = self.command(args);
        assert!(
            output.status.success(),
            "cmp {:?} failed:\n{}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    fn json(&self, args: &[&str]) -> Value {
        serde_json::from_str(&self.check(args)).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn local_offline_repositories_survive_multiple_commands_without_a_server() {
    let fx = Fixture::new();
    fx.check(&[
        "setup",
        "--scope",
        "project",
        "--backend",
        "offline",
        "--endpoint",
        "https://offline.invalid",
        "--repo",
        "person/test",
        "--no-streaming",
        "--no-interactive",
    ]);
    fx.check(&["init", "--repo", "person/test", "--no-interactive"]);
    assert!(fx.project().join(".cmp/config.toml").is_file());
    assert!(!fx.project().join(".git").exists());
    let config = fs::read_to_string(fx.project().join(".cmp/config.toml")).unwrap();
    assert!(config.contains("backend = \"offline\""));
    assert!(config.contains("streaming = false"));
    let head = fx.json(&["head"])["head"].as_str().unwrap().to_owned();
    assert_eq!(head.len(), 64);
    let message = json!({"message":"Local sourced fact","changes":[{"op":"put","record":{
        "id":"fact:one","kind":"fact","content":"Local device stores context",
        "sourceRefs":["notes.md"],"status":"approved",
        "authorId":"cmp-test","updatedAt":"2026-11-16T12:00:00Z"
    }}]});
    let changes = fx.project().join("changes.json");
    fs::write(&changes, message.to_string()).unwrap();
    let created = fx.json(&["commit", changes.to_str().unwrap()]);
    assert_eq!(created["branch"], "main");
    let new_head = fx.json(&["head"])["head"].as_str().unwrap().to_owned();
    assert_ne!(head, new_head);
    let snapshot = fx.json(&["snapshot"]);
    assert_eq!(snapshot["records"][0]["id"], "fact:one");
    assert_eq!(
        snapshot["records"][0]["content"],
        "Local device stores context"
    );
    assert_eq!(
        fx.json(&["repo", "list"])["repositories"],
        json!(["person/test"])
    );
    fx.json(&["branch", "create", "feature"]);
    assert_eq!(fx.json(&["head", "feature"])["head"], new_head);
    assert_eq!(fx.json(&["diff", "main", "feature"])["changes"], json!([]));
    assert!(fx.check(&["stream", "status"]).contains("0"));
    assert!(fx
        .check(&["doctor"])
        .contains("Local context store: available"));
    let content = fs::read_to_string(
        fx.project()
            .join(".cmp/state/repos")
            .read_dir()
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path(),
    )
    .unwrap();
    assert!(content.contains("Local device stores context"));
    assert!(!fx.project().join(".git").exists());
}
#[test]
fn stale_cas_never_changes_offline_branch() {
    let fx = Fixture::new();
    fx.check(&[
        "setup",
        "--scope",
        "project",
        "--backend",
        "offline",
        "--repo",
        "person/test",
        "--no-interactive",
    ]);
    fx.check(&["init", "--repo", "person/test", "--no-interactive"]);
    let old = fx.json(&["head"])["head"].as_str().unwrap().to_owned();
    let changes = fx.project().join("stale.json");
    fs::write(
        &changes,
        json!({"message":"Stale record","expectedHead":"f".repeat(64),"changes":[]}).to_string(),
    )
    .unwrap();
    let failure = fx.command(&["commit", changes.to_str().unwrap()]);
    assert!(!failure.status.success());
    assert!(String::from_utf8_lossy(&failure.stderr).contains("CAS conflict"));
    assert_eq!(fx.json(&["head"])["head"], old);
}
#[test]
fn streaming_remains_off_by_default_and_is_explicitly_opt_in() {
    let fx = Fixture::new();
    fx.check(&[
        "setup",
        "--scope",
        "project",
        "--backend",
        "offline",
        "--endpoint",
        "http://127.0.0.1:59999",
        "--repo",
        "person/test",
        "--no-interactive",
    ]);
    fx.check(&["init", "--repo", "person/test", "--no-interactive"]);
    assert!(fx.check(&["status"]).contains("Streaming: disabled"));
    assert!(fx.check(&["stream", "status"]).contains("0"));
    let output = fx.command(&["stream", "push"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Streaming is disabled"));
}

#[test]
fn streaming_sends_changes_only_after_opt_in() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };
    let fx = Fixture::new();
    let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = server.local_addr().unwrap();
    server.set_nonblocking(true).unwrap();
    let worker = thread::spawn(move || {
        let replies = [
            r#"{"repositories":[]}"#,
            r#"{"id":"person/test","head":"genesis"}"#,
            r#"{"repositories":["person/test"]}"#,
            r#"{"head":"remote-genesis"}"#,
            r#"{"records":[]}"#,
            r#"{"id":"remote-commit"}"#,
        ];
        let mut requests = Vec::new();
        for response in replies {
            let deadline = Instant::now() + Duration::from_secs(12);
            let (mut stream, _) = loop {
                match server.accept() {
                    Ok(conn) => break conn,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(e) => panic!("Missing stream operation: {e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut buf = [0u8; 4096];
                let count = stream.read(&mut buf).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buf[..count]);
                if let Some(split) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&request[..split]);
                    let size = header
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .and_then(|(_, size)| size.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= split + 4 + size {
                        break;
                    }
                }
            }
            let text = String::from_utf8(request).unwrap();
            assert!(text
                .to_ascii_lowercase()
                .contains("authorization: bearer test-stream-token"));
            requests.push(text);
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
            stream.flush().unwrap();
        }
        requests
    });
    let endpoint = format!("http://{address}");
    let send = |args: &[&str]| {
        fs::create_dir_all(fx.project()).unwrap();
        let out = Command::new(CLI)
            .current_dir(fx.project())
            .env("CMP_HOME", &fx.0)
            .env("CMP_DEV_TOKEN", "test-stream-token")
            .env_remove("CMP_CONFIG")
            .env_remove("CMP_ENDPOINT")
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    send(&[
        "setup",
        "--scope",
        "project",
        "--backend",
        "offline",
        "--endpoint",
        &endpoint,
        "--repo",
        "person/test",
        "--streaming",
        "--no-interactive",
    ]);
    send(&["init", "--repo", "person/test", "--no-interactive"]);
    let file = fx.project().join("change.json");
    fs::write(
        &file,
        json!({"message":"Local commit","changes":[{"op":"put","record":{
            "id":"fact:one","kind":"fact","content":"Streaming was explicitly enabled",
            "sourceRefs":[],"status":"approved","authorId":"test","updatedAt":"2026-11-16T12:00:00Z"
        }}]})
        .to_string(),
    )
    .unwrap();
    send(&["commit", file.to_str().unwrap()]);
    let requests = worker.join().unwrap();
    assert_eq!(requests.len(), 6);
    assert!(requests[1].starts_with("POST /v1/repos "));
    assert!(requests[3].contains("GET /v1/repos/person%2Ftest/head?branch=main"));
    assert!(requests[4].contains("GET /v1/repos/person%2Ftest/records?ref=main"));
    assert!(requests[5].contains(r#""expectedHead":"remote-genesis""#));
    assert!(requests[5].contains(r#""op":"put""#));
    assert!(fx.check(&["stream", "status"]).contains("0"));
}

#[test]
fn offline_mcp_serves_context_without_cloud_credentials() {
    use std::{io::Write, process::Stdio};
    let fx = Fixture::new();
    fx.check(&[
        "setup",
        "--scope",
        "project",
        "--backend",
        "offline",
        "--endpoint",
        "https://unreachable.invalid",
        "--repo",
        "person/test",
        "--no-interactive",
    ]);
    fx.check(&["init", "--repo", "person/test", "--no-interactive"]);
    let mut child = Command::new(CLI)
        .current_dir(fx.project())
        .env("CMP_HOME", &fx.0)
        .env_remove("CMP_CONFIG")
        .env_remove("CMP_ENDPOINT")
        .env_remove("CMP_REPOSITORY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args(["mcp", "serve"])
        .spawn()
        .unwrap();
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"cmp_context_head","arguments":{}}});
    writeln!(child.stdin.as_mut().unwrap(), "{request}").unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!response["result"]["isError"].as_bool().unwrap_or(true));
    let body = response["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(body).unwrap()["head"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
}

#[test]
fn offline_updates_survive_streaming_transport_failure() {
    let fx = Fixture::new();
    fx.check(&[
        "setup",
        "--scope",
        "project",
        "--backend",
        "offline",
        "--endpoint",
        "https://offline.invalid",
        "--repo",
        "person/test",
        "--streaming",
        "--no-interactive",
    ]);
    // Streaming has no credentials or remote connectivity, but local init succeeds.
    fx.check(&["init", "--repo", "person/test", "--no-interactive"]);
    let head = fx.json(&["head"]);
    assert_eq!(head["head"].as_str().unwrap().len(), 64);
    assert!(fx.check(&["stream", "status"]).contains("1"));
    let data = fs::read_to_string(fx.project().join(".cmp/state/stream-outbox.json")).unwrap();
    assert!(data.contains("person/test"));
    assert!(fx.project().join(".cmp/.gitignore").exists());
}

#[test]
fn streaming_refuses_existing_remote_records_without_losing_local_commit() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };
    let fx = Fixture::new();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let responses = [
            r#"{"repositories":["person/test"]}"#,
            r#"{"repositories":["person/test"]}"#,
            r#"{"head":"remote-existing-head"}"#,
            r#"{"records":[{"id":"existing-record"}]}"#,
        ];
        let mut paths = Vec::new();
        for response in responses {
            let until = Instant::now() + Duration::from_secs(10);
            let (mut conn, _) = loop {
                match listener.accept() {
                    Ok(result) => break result,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < until =>
                    {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => panic!("Streaming expected additional request: {e}"),
                }
            };
            conn.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
            let mut buffer = [0u8; 4096];
            let len = conn.read(&mut buffer).unwrap();
            let request = String::from_utf8_lossy(&buffer[..len]);
            paths.push(request.lines().next().unwrap_or("").to_owned());
            write!(
                conn,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            ).unwrap();
            conn.flush().unwrap();
        }
        paths
    });
    let endpoint = format!("http://{addr}");
    let exec = |args: &[&str]| {
        let output = Command::new(CLI)
            .current_dir(fx.project())
            .env("CMP_HOME", &fx.0)
            .env("CMP_DEV_TOKEN", "mock-stream-token")
            .env_remove("CMP_CONFIG")
            .env_remove("CMP_ENDPOINT")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    fs::create_dir_all(fx.project()).unwrap();
    exec(&[
        "setup",
        "--scope",
        "project",
        "--backend",
        "offline",
        "--endpoint",
        &endpoint,
        "--repo",
        "person/test",
        "--streaming",
        "--no-interactive",
    ]);
    exec(&["init", "--repo", "person/test", "--no-interactive"]);
    let change_file = fx.project().join("commit.json");
    fs::write(
        &change_file,
        json!({"message":"Do not overwrite a remote record","changes":[]}).to_string(),
    )
    .unwrap();
    exec(&["commit", change_file.to_str().unwrap()]);
    let calls = server.join().unwrap();
    assert_eq!(calls.len(), 4);
    assert!(calls[3].contains("/v1/repos/person%2Ftest/records?ref=main"));
    assert!(fx.check(&["stream", "status"]).contains("1"));
    assert_ne!(fx.json(&["head"])["head"], Value::Null);
}

#[test]
fn disabling_streaming_preserves_pending_without_enqueuing_new_context_changes() {
    let fx = Fixture::new();
    fx.check(&[
        "setup", "--scope", "project", "--backend", "offline",
        "--endpoint", "https://offline.invalid", "--repo", "local/knowledge",
        "--streaming", "--no-interactive",
    ]);
    fx.check(&["init", "--repo", "local/knowledge", "--no-interactive"]);
    // Initial repository creation is queued because streaming was explicitly enabled.
    let queue = fx.project().join(".cmp/state/stream-outbox.json");
    let before: Value = serde_json::from_slice(&fs::read(&queue).unwrap()).unwrap();
    assert_eq!(before["queued"].as_array().unwrap().len(), 1);
    fx.check(&["config", "set", "streaming", "false"]);
    assert_eq!(fx.check(&["config", "get", "streaming"]).trim(), "false");
    let blocked = fx.command(&["stream", "push"]);
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("Streaming is disabled"));

    // Local writes continue to persist, but do not enqueue or attempt upload.
    let file = fx.project().join("change-while-streaming-disabled.json");
    fs::write(&file,json!({
        "message":"Local only context",
        "changes":[{"op":"put","record":{
            "id":"fact:offline","kind":"fact","content":"Never uploaded while disabled",
            "sourceRefs":[],"status":"approved","authorId":"fixture",
            "updatedAt":"2026-11-16T12:00:00Z"
        }}]
    }).to_string()).unwrap();
    fx.json(&["commit",file.to_str().unwrap()]);
    let after: Value = serde_json::from_slice(&fs::read(&queue).unwrap()).unwrap();
    assert_eq!(after["queued"],before["queued"]);
    assert_eq!(fx.json(&["snapshot"])["records"][0]["id"],"fact:offline");

    // Re-enabling is explicit and cannot silently erase queued history.
    fx.check(&["config","set","streaming","true"]);
    let reopened: Value = serde_json::from_slice(&fs::read(&queue).unwrap()).unwrap();
    assert_eq!(reopened["queued"],before["queued"]);
    assert_eq!(fx.check(&["config","get","streaming"]).trim(),"true");
}
