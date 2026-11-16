use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CLI: &str = env!("CARGO_BIN_EXE_cmp");
const TOKEN: &str = "cmp-review-integration-local-token-0123456789abcdef";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // Prevent collisions when OS clocks share timestamps across parallel tests.
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "cmp-cli-review-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn path(&self, suffix: &str) -> PathBuf {
        self.0.join(suffix)
    }
    fn write(&self, suffix: &str, text: &str) -> PathBuf {
        let file = self.path(suffix);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, text).unwrap();
        file
    }
    fn cli(&self, cwd: &Path) -> Command {
        let mut command = Command::new(CLI);
        command
            .current_dir(cwd)
            .env("CMP_HOME", self.path("home"))
            .env("CMP_DEV_TOKEN", TOKEN)
            .env_remove("CMP_CONFIG")
            .env_remove("CMP_ENDPOINT")
            .env_remove("CMP_REPOSITORY");
        command
    }
    fn config(&self, endpoint: &str, branch: &str) {
        self.write("project/.cmp/config.toml", &format!(
            "version=1\nbackend='local'\nendpoint='{endpoint}'\nrepository='owner/knowledge'\nbranch='{branch}'\n"
        ));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "CLI stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
}
fn run(fixture: &Fixture, cwd: &Path, args: &[&str]) -> Output {
    fixture.cli(cwd).args(args).output().unwrap()
}

#[test]
fn fresh_cmp_init_writes_dot_cmp_without_creating_dot_git() {
    let t = Fixture::new();
    fs::create_dir_all(t.path("project")).unwrap();
    let setup = run(
        &t,
        &t.path("project"),
        &[
            "setup",
            "--scope",
            "global",
            "--backend",
            "local",
            "--no-interactive",
        ],
    );
    assert_ok(&setup);
    let init = run(
        &t,
        &t.path("project"),
        &[
            "init",
            "--repo",
            "team/context",
            "--offline",
            "--no-interactive",
        ],
    );
    assert_ok(&init);
    assert!(t.path("project/.cmp/config.toml").is_file());
    assert!(
        !t.path("project/.git").exists(),
        "CMP must never create Git metadata"
    );
    let file = fs::read_to_string(t.path("project/.cmp/config.toml")).unwrap();
    assert!(file.contains("team/context"));
}

#[test]
fn project_setup_then_init_reuses_dot_cmp_without_force() {
    let t = Fixture::new();
    fs::create_dir_all(t.path("project")).unwrap();
    assert_ok(&run(
        &t,
        &t.path("project"),
        &[
            "setup",
            "--scope",
            "project",
            "--backend",
            "local",
            "--repo",
            "team/context",
            "--no-interactive",
        ],
    ));
    let config = t.path("project/.cmp/config.toml");
    let prior = fs::read(&config).unwrap();
    assert_ok(&run(
        &t,
        &t.path("project"),
        &[
            "init",
            "--repo",
            "team/context",
            "--offline",
            "--no-interactive",
        ],
    ));
    assert_eq!(
        fs::read(&config).unwrap(),
        prior,
        "Matching project config must stay unchanged"
    );
    assert!(
        !t.path("project/.git").exists(),
        "CMP may not create Git metadata"
    );
}

#[test]
fn ancestor_binding_and_explicit_config_honor_environment_overrides() {
    let t = Fixture::new();
    t.write("home/.cmp/config.toml",
        "version=1\nbackend='local'\nendpoint='http://127.0.0.1:8787'\nrepository='global/repo'\nbranch='main'\n");
    t.write("project/.cmp/config.toml",
        "version=1\nbackend='local'\nendpoint='http://127.0.0.1:8788'\nrepository='project/repo'\nbranch='feature'\n");
    fs::create_dir_all(t.path("project/src/deeper")).unwrap();
    let cwd = t.path("project/src/deeper");
    let nested = run(&t, &cwd, &["status"]);
    assert_ok(&nested);
    let text = String::from_utf8_lossy(&nested.stdout);
    assert!(
        text.contains("project/repo") && text.contains("feature"),
        "Expected closest project config; got: {text}"
    );
    // PathBuf::join and Path::display can retain different mixed separator
    // spellings on Windows for the same file. Verify filesystem identity.
    let reported = text
        .lines()
        .find_map(|line| line.strip_prefix("Config: "))
        .expect("CMP status did not report a config source")
        .trim();
    assert_eq!(
        fs::canonicalize(reported).unwrap(),
        fs::canonicalize(t.path("project/.cmp/config.toml")).unwrap(),
        "CMP selected a different configuration file: {text}"
    );

    t.write("selected.toml",
        "version=1\nbackend='local'\nendpoint='http://127.0.0.1:8770'\nrepository='wrong/repo'\nbranch='main'\n");
    let explicit = t
        .cli(&cwd)
        .args(["status"])
        .env("CMP_CONFIG", t.path("selected.toml"))
        .env("CMP_ENDPOINT", "http://127.0.0.1:9999/")
        .env("CMP_REPOSITORY", "override/correct")
        .output()
        .unwrap();
    assert_ok(&explicit);
    let text = String::from_utf8_lossy(&explicit.stdout);
    assert!(text.contains("override/correct") && text.contains("127.0.0.1:9999"));
    assert!(!text.contains("wrong/repo"), "{text}");

    // Codex follow-up: explicitly selected local config never requires a
    // home directory when running in a minimal container or CI process.
    let without_home = t
        .cli(&cwd)
        .args(["status"])
        .env("CMP_CONFIG", t.path("selected.toml"))
        .env_remove("CMP_HOME")
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .output()
        .unwrap();
    assert_ok(&without_home);
    assert!(
        String::from_utf8_lossy(&without_home.stdout).contains("wrong/repo"),
        "Explicit config should work without any home directory"
    );

    let setup = run(
        &t,
        &t.path("project"),
        &[
            "setup",
            "--scope",
            "project",
            "--backend",
            "cloud",
            "--endpoint",
            "https://CLOUD.cmp.chat/",
            "--force",
            "--skip-login",
            "--no-interactive",
        ],
    );
    assert_ok(&setup);
    let config = fs::read_to_string(t.path("project/.cmp/config.toml")).unwrap();
    assert!(
        config.contains("https://cloud.cmp.chat\"") || config.contains("https://cloud.cmp.chat'"),
        "{config}"
    );
}

#[test]
fn opencode_existing_config_is_merged_and_directory_permissions_remain() {
    let t = Fixture::new();
    fs::create_dir_all(t.path("project")).unwrap();
    t.write(
        "project/opencode.json",
        r#"{"theme":"github","mcp":{"other":{"type":"local","command":["other"]}}}"#,
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(t.path("project"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_ok(&run(
        &t,
        &t.path("project"),
        &["agent", "install", "opencode"],
    ));
    let content = fs::read_to_string(t.path("project/opencode.json")).unwrap();
    let value: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(value["theme"], "github");
    assert_eq!(value["mcp"]["other"]["command"], json!(["other"]));
    assert_eq!(
        value["mcp"]["cmp"]["command"],
        json!(["cmp", "mcp", "serve"])
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(t.path("project"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
    assert_ok(&run(
        &t,
        &t.path("project"),
        &["agent", "remove", "opencode"],
    ));
    let after: Value =
        serde_json::from_str(&fs::read_to_string(t.path("project/opencode.json")).unwrap())
            .unwrap();
    assert!(after["mcp"].get("cmp").is_none());
    assert_eq!(after["mcp"]["other"]["command"], json!(["other"]));
}

#[test]
fn codex_custom_nested_cmp_configuration_is_not_destroyed() {
    let t = Fixture::new();
    let content="[features]\nplanning = true\n\n# CMP CLI managed MCP entry\n[mcp_servers.cmp]\ncommand = 'cmp'\nargs = ['mcp','serve']\ncwd = '/tmp'\n[mcp_servers.cmp.env]\nTOKEN = 'custom'\n";
    t.write("project/.codex/config.toml", content);
    let result = run(&t, &t.path("project"), &["agent", "remove", "codex"]);
    assert!(
        !result.status.success(),
        "Should refuse customized nested CMP config"
    );
    assert_eq!(
        fs::read_to_string(t.path("project/.codex/config.toml")).unwrap(),
        content
    );
    t.write("project/.codex/config.toml",
        "[features]\nplanning = true\n\n# CMP CLI managed MCP entry\n[mcp_servers.cmp]\ncommand = 'cmp'\nargs = ['mcp','serve']\ncwd = '/tmp'\n");
    assert_ok(&run(&t, &t.path("project"), &["agent", "remove", "codex"]));
    let after = fs::read_to_string(t.path("project/.codex/config.toml")).unwrap();
    assert!(after.contains("planning = true"));
    assert!(!after.contains("mcp_servers.cmp"));
}

fn mock_server(
    status: u16,
    body: &'static str,
) -> (String, Receiver<(String, Value)>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (sender, receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(7);
        let mut stream = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("Mock server did not receive expected request: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
            if let Some(separator) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&bytes[..separator]);
                let length = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        (key.eq_ignore_ascii_case("content-length"))
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if bytes.len() >= separator + 4 + length {
                    break;
                }
            }
        }
        let separator = bytes.windows(4).position(|x| x == b"\r\n\r\n").unwrap();
        let header = String::from_utf8_lossy(&bytes[..separator]);
        let request_line = header.lines().next().unwrap().to_owned();
        let json = serde_json::from_slice::<Value>(&bytes[separator + 4..]).unwrap_or(json!(null));
        sender.send((request_line, json)).unwrap();
        let message = if status == 409 { "Conflict" } else { "OK" };
        write!(stream,"HTTP/1.1 {status} {message}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        stream.flush().unwrap();
    });
    (format!("http://{addr}"), receiver, worker)
}

#[test]
fn caller_expected_head_is_not_refreshed_before_conflict() {
    let t = Fixture::new();
    let (endpoint, received, server) = mock_server(409, r#"{"error":"Conflicting head"}"#);
    t.config(&endpoint, "evidence");
    let stale = "a".repeat(64);
    let file = t.write(
        "project/changes.json",
        &json!({
            "branch":"evidence","message":"Stale claim","expectedHead":stale,
            "changes":[{"op":"delete","id":"claim"}]
        })
        .to_string(),
    );
    let out = run(&t, &t.path("project"), &["commit", file.to_str().unwrap()]);
    assert!(!out.status.success(), "Stale CAS must return failure");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("409"),
        "Expected server CAS conflict 409, got: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let (request, body) = received.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(
        request.starts_with("POST /v1/repos/owner%2Fknowledge/commits "),
        "{request}"
    );
    assert_eq!(body["expectedHead"], stale);
    assert_eq!(body["branch"], "evidence");
    server.join().unwrap();
}

#[test]
fn merged_namespaced_repository_is_encoded_as_single_path_segment() {
    let t = Fixture::new();
    let (endpoint, received, server) = mock_server(200, r#"{"merged":true}"#);
    t.config(&endpoint, "feature");
    assert_ok(&run(&t, &t.path("project"), &["merge", "feature", "main"]));
    let (request, body) = received.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(
        request.starts_with("POST /v1/repos/owner%2Fknowledge/merge "),
        "{request}"
    );
    assert_eq!(body["source"], "feature");
    server.join().unwrap();
}

#[test]
fn mcp_head_defaults_to_project_branch_not_main() {
    let t = Fixture::new();
    let (endpoint, received, server) = mock_server(200, r#"{"head":"feature-head"}"#);
    t.config(&endpoint, "feature");
    let mut child = t
        .cli(&t.path("project"))
        .args(["mcp", "serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        let req = json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"cmp_context_head","arguments":{}}});
        writeln!(stdin, "{req}").unwrap();
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert_ok(&output);
    let data: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        !data["result"]["isError"].as_bool().unwrap_or(true),
        "{data}"
    );
    let (request, _) = received.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(
        request.contains("/v1/repos/owner%2Fknowledge/head?branch=feature "),
        "{request}"
    );
    server.join().unwrap();
}
