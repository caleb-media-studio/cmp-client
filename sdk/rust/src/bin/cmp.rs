//! CMP's native developer CLI. The protocol library is in cmp_context.
#[path = "cmp/agents.rs"]
mod cmp_agents;
#[path = "cmp/mcp.rs"]
mod cmp_mcp;
#[path = "cmp/tui.rs"]
mod cmp_tui;
#[path = "cmp/offline.rs"]
mod cmp_offline;
#[path = "cmp/credentials.rs"]
mod cmp_credentials;
#[path = "cmp/stream.rs"]
mod cmp_stream;
#[cfg(feature = "cloud-fixture-tests")]
#[path = "cmp/cloud_fixture.rs"]
mod cmp_cloud_fixture;
#[path = "cmp/gh.rs"]
mod cmp_gh;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cmp_context::Client;
use fs2::FileExt;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    env,
    error::Error,
    fs::{self, OpenOptions},
    io::{self, IsTerminal, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const CLOUD: &str = "https://cloud.cmp.chat";
const LOCAL: &str = "http://127.0.0.1:8787";
const SERVICE: &str = "cmp.cloud.cli";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u8,
    backend: String,
    endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    repository: Option<String>,
    branch: String,
    #[serde(default)]
    streaming: bool,
}
impl Config {
    fn new(backend: &str, endpoint: &str) -> Self {
        Self {
            version: 1,
            backend: backend.to_string(),
            endpoint: endpoint.to_string(),
            repository: None,
            branch: "main".into(),
            streaming: false,
        }
    }
}
fn home() -> Result<PathBuf> {
    let root = env::var_os("CMP_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(PathBuf::from))
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
        .ok_or("Unable to determine home directory")?;
    if !root.is_absolute() {
        return Err("CMP home must be an absolute path".into());
    }
    Ok(root)
}
fn global_file() -> Result<PathBuf> {
    Ok(home()?.join(".cmp/config.toml"))
}
fn project_file() -> Result<PathBuf> {
    Ok(env::current_dir()?.join(".cmp/config.toml"))
}
fn normalize_endpoint(value: &str) -> Result<String> {
    validate_endpoint(value)?;
    // URL parsing normalizes host casing, default ports, and trailing root '/'.
    // OAuth must compare canonical origins, not textual variants of the same URL.
    Ok(url::Url::parse(value)?
        .as_str()
        .trim_end_matches('/')
        .to_owned())
}
fn load_file(path: &Path) -> Result<Config> {
    let file = fs::read_to_string(path)?;
    let mut config: Config = toml::from_str(&file)?;
    if config.version != 1 {
        return Err("Unsupported CMP config version".into());
    }
    config.endpoint = normalize_endpoint(&config.endpoint)?;
    if !["cloud", "local", "offline"].contains(&config.backend.as_str()) {
        return Err("Invalid CMP backend".into());
    }
    Ok(config)
}
fn nearest_project_file(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(".cmp/config.toml"))
        .find(|path| path.is_file())
}
fn load_config() -> Result<(Config, PathBuf)> {
    // Explicit config selects the source, but does not bypass environment overrides.
    let selected = env::var_os("CMP_CONFIG").map(PathBuf::from);
    let project = nearest_project_file(&env::current_dir()?);
    let source = if let Some(path) = selected.or(project) {
        path
    } else {
        // A selected local configuration must work without HOME, such as in
        // a minimal container. Discover ~/.cmp only as a final fallback.
        let global = global_file()?;
        if !global.is_file() {
            return Err("CMP is not configured. Run cmp setup".into());
        }
        global
    };
    let mut cfg = load_file(&source)?;
    if let Ok(endpoint) = env::var("CMP_ENDPOINT") {
        cfg.endpoint = normalize_endpoint(&endpoint)?;
    }
    if let Ok(repo) = env::var("CMP_REPOSITORY") {
        validate_repo(&repo)?;
        cfg.repository = Some(repo);
    }
    // Fail closed at config loading, BEFORE any public capabilities HTTP GET.
    // Merely checking in auth_token is too late: doctor makes a public
    // capabilities request first. A production CLI must never probe an
    // arbitrary Cloud origin while assuming the official Cloud identity.
    if cfg.backend == "cloud" && cfg.endpoint != CLOUD {
        #[cfg(feature = "cloud-fixture-tests")]
        {
            if !cmp_cloud_fixture::authorized(&cfg)? {
                return Err("Delegated CMP Cloud OAuth currently supports only cloud.cmp.chat".into());
            }
        }
        #[cfg(not(feature = "cloud-fixture-tests"))]
        return Err("Delegated CMP Cloud OAuth currently supports only cloud.cmp.chat".into());
    }
    #[cfg(feature = "cloud-fixture-tests")]
    if env::var("CMP_CLOUD_FIXTURE_MODE").as_deref() == Ok("1") {
        // A feature-gated test binary still must not contact Cloud production.
        cmp_cloud_fixture::authorized(&cfg)?;
    }
    Ok((cfg, source))
}
fn validate_repo(id: &str) -> Result<()> {
    let parts: Vec<_> = id.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|p| {
            p.is_empty()
                || p.len() > 100
                || !p
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        })
    {
        return Err("Repository must have the form owner/name".into());
    }
    Ok(())
}
fn validate_endpoint(value: &str) -> Result<()> {
    let url = url::Url::parse(value)?;
    if url.username() != ""
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
        || url.path() != "/"
    {
        return Err("CMP endpoint cannot contain credentials, query or a path".into());
    }
    if url.scheme() != "https"
        && !(url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1")))
    {
        return Err("Remote CMP endpoints must use HTTPS; localhost may use HTTP".into());
    }
    Ok(())
}
#[cfg(unix)]
fn private_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
#[cfg(not(unix))]
fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    Ok(())
}
fn save_config(file: &Path, config: &Config) -> Result<()> {
    let parent = file
        .parent()
        .ok_or("Invalid CMP configuration destination")?;
    private_dir(parent)?;
    if fs::symlink_metadata(file).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Refusing to overwrite symbolic link in CMP configuration".into());
    }
    let tmp = parent.join(format!(".config-{}.tmp", random_short()));
    #[cfg(unix)]
    let mut handle = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?
    };
    #[cfg(not(unix))]
    let mut handle = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    handle.write_all(toml::to_string_pretty(config)?.as_bytes())?;
    handle.sync_all()?;
    drop(handle);
    if let Err(err) = fs::rename(&tmp, file) {
        let _ = fs::remove_file(&tmp);
        return Err(err.into());
    }
    Ok(())
}
fn write_secure_file(file: &Path, body: &str, replace: bool) -> Result<()> {
    let parent = file.parent().ok_or("Invalid CMP config path")?;
    // Host configuration may be at the project root (opencode.json).
    // Do not chmod a pre-existing project or agent-owned directory to 0700.
    // CMP-owned .cmp directories are restricted separately by save_config().
    fs::create_dir_all(parent)?;
    if fs::symlink_metadata(file).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("Refusing to modify symlinked config".into());
    }
    if file.exists() && !replace {
        return Err("Target file already exists; refusing to overwrite".into());
    }
    let tmp = parent.join(format!(".cmp-edit-{}.tmp", random_short()));
    #[cfg(unix)]
    let mut handle = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?
    };
    #[cfg(not(unix))]
    let mut handle = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    handle.write_all(body.as_bytes())?;
    handle.sync_all()?;
    drop(handle);
    if let Err(err) = fs::rename(&tmp, file) {
        let _ = fs::remove_file(&tmp);
        return Err(err.into());
    }
    Ok(())
}
fn explicit_commit_head(payload: &Value) -> Result<Option<&str>> {
    let Some(value) = payload.get("expectedHead") else {
        return Ok(None);
    };
    let head = value
        .as_str()
        .ok_or("expectedHead must be a commit hash string")?;
    if head.len() != 64 || !head.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("expectedHead must contain exactly 64 hexadecimal characters".into());
    }
    Ok(Some(head))
}
fn random_short() -> String {
    let mut raw = [0u8; 8];
    OsRng.fill_bytes(&mut raw);
    URL_SAFE_NO_PAD.encode(raw)
}
fn prompt(question: &str, default: &str) -> Result<String> {
    cmp_tui::input(question, default)
}

fn require_tty() -> Result<()> {
    if !io::stdin().is_terminal() {
        return Err(
            "Noninteractive session: supply --scope, --backend and relevant options".into(),
        );
    }
    Ok(())
}
fn argument(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}
fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|x| x == name)
}
fn scope_path(scope: &str) -> Result<PathBuf> {
    match scope {
        "global" => global_file(),
        "project" => project_file(),
        _ => Err("Scope must be global or project".into()),
    }
}
fn setup(args: &[String]) -> Result<()> {
    let noninteractive = flag(args, "--no-interactive");
    if !noninteractive {
        require_tty()?;
    }
    let scope = match argument(args, "--scope") {
        Some(s) => s,
        None if noninteractive => return Err("--scope required for noninteractive setup".into()),
        None => {
            let choices = ["Global (~/.cmp) · all projects", "This project (./.cmp)"];
            if cmp_tui::choose("Where should CMP store its configuration?", &choices, 0)? == 0 {
                "global".into()
            } else {
                "project".into()
            }
        }
    };
    let backend = match argument(args, "--backend") {
        Some(s) => s,
        None if noninteractive => return Err("--backend required for noninteractive setup".into()),
        None => {
            let choices = ["CMP Cloud · cloud.cmp.chat", "Self-hosted CMP server", "Local / Offline · device-only context"];
            match cmp_tui::choose("Where should CMP context live?", &choices, 0)? {
                0 => "cloud".into(),
                1 => "local".into(),
                _ => "offline".into(),
            }
        }
    };
    if !["cloud", "local", "offline"].contains(&backend.as_str()) {
        return Err("Backend must be cloud or local".into());
    }
    let fallback = if backend == "local" { LOCAL } else { CLOUD };
    let endpoint = match argument(args, "--endpoint") {
        Some(s) => s,
        None if noninteractive => fallback.into(),
        None if backend != "local" => fallback.into(),
        None => prompt("CMP server endpoint", fallback)?,
    };
    let endpoint = normalize_endpoint(&endpoint)?;
    #[cfg(feature = "cloud-fixture-tests")]
    if env::var("CMP_CLOUD_FIXTURE_MODE").as_deref() == Ok("1") {
        cmp_cloud_fixture::authorized(&Config::new(&backend, &endpoint))?;
    }
    let dest = scope_path(&scope)?;
    let mut cfg = Config::new(&backend, &endpoint);
    if flag(args, "--streaming") && flag(args, "--no-streaming") {
        return Err("Choose --streaming or --no-streaming, not both".into());
    }
    if backend == "offline" {
        cfg.streaming = if flag(args, "--streaming") {
            true
        } else if flag(args, "--no-streaming") || noninteractive {
            false
        } else {
            cmp_tui::confirm("Enable streaming? Context may leave this device.", false)?
        };
    } else if flag(args, "--streaming") {
        return Err("Streaming is an option for the offline backend only".into());
    }
    if scope == "project" {
        let repo = match argument(args, "--repo") {
            Some(s) => s,
            None if noninteractive => String::new(),
            None => prompt("Context repository (owner/name, or leave empty)", "")?,
        };
        if !repo.is_empty() {
            validate_repo(&repo)?;
            cfg.repository = Some(repo);
        }
    }
    if dest.exists() && !flag(args, "--force") {
        if noninteractive {
            return Err("CMP config exists; use --force to replace explicitly".into());
        }
        if !cmp_tui::confirm("Replace existing CMP configuration?", false)? {
            return Err("Setup cancelled; existing config preserved".into());
        }
    }
    save_config(&dest, &cfg)?;
    cmp_tui::success(format!(
        "Configured CMP {}: {} ({})",
        scope,
        endpoint,
        dest.display()
    ));
    if backend == "cloud" {
        if noninteractive || flag(args, "--skip-login") {
            println!("Next: cmp login");
        } else if cmp_tui::confirm("Sign in to CMP Cloud now?", true)? {
            login(args)?;
        }
    } else if backend == "offline" {
        println!("Offline context storage: .cmp/state (no network while streaming disabled).");
        println!("Streaming: {}", if cfg.streaming { "enabled (opt-in)" } else { "disabled" });
        println!("Next: cmp init");
    } else {
        println!("Self-hosted mode: use CMP_DEV_TOKEN to authenticate to the development server.");
        println!("Next: cmp doctor");
    }
    Ok(())
}
fn context_request(cfg: &Config, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
    if cfg.backend == "offline" {
        let result = cmp_offline::request(cfg, method, path, body.clone())?;
        if let Some(value) = &body {
            cmp_stream::enqueue_and_try(cfg, method, path, value)?;
        }
        Ok(result)
    } else {
        Ok(client(cfg)?.request(method, path, body)?)
    }
}
fn auth_token(cfg: &Config) -> Result<String> {
    // This compile-time-only Cloud fixture path is NEVER present in the
    // normal CLI build and cannot address external/production hosts.
    #[cfg(feature = "cloud-fixture-tests")]
    let local_cloud_fixture = cmp_cloud_fixture::authorized(cfg)?;
    #[cfg(not(feature = "cloud-fixture-tests"))]
    let local_cloud_fixture = false;
    if cfg.backend == "local" || (cfg.backend == "offline" && cfg.endpoint != CLOUD && !local_cloud_fixture) {
        return env::var("CMP_DEV_TOKEN")
            .map_err(|_| "Local CMP server requires CMP_DEV_TOKEN".into());
    }
    if cfg.endpoint != CLOUD && !local_cloud_fixture {
        return Err("Delegated CMP Cloud OAuth currently supports only cloud.cmp.chat".into());
    }
    let root = home()?.join(".cmp");
    private_dir(&root)?;
    let lock_path = root.join(".refresh.lock");
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    lock.lock_exclusive()?;
    let result = (|| {
        let credential = cmp_credentials::load()?;
        let refresh = credential.token.as_str();
        let response = ureq::post(&(cfg.endpoint.clone() + "/api/oauth/token"))
            .set("Content-Type", "application/x-www-form-urlencoded")
            .timeout(Duration::from_secs(12))
            .send_string(
                &url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("grant_type", "refresh_token")
                    .append_pair("client_id", "cmp-cli")
                    .append_pair("refresh_token", refresh)
                    .finish(),
            )?;
        let response: Value = response.into_json()?;
        let access = response["access_token"]
            .as_str()
            .ok_or("CMP Cloud returned no access token")?;
        let next = response["refresh_token"]
            .as_str()
            .ok_or("CMP Cloud returned no refresh token")?;
        cmp_credentials::rotate(&credential, next)?;
        Ok(access.to_owned())
    })();
    let _ = fs2::FileExt::unlock(&lock);
    result
}
fn client(cfg: &Config) -> Result<Client> {
    Ok(Client::new(&cfg.endpoint, auth_token(cfg)?))
}
fn open_url(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        // Execute Explorer directly: cmd /C start reparses ampersands in
        // OAuth query strings as shell metacharacters on Windows.
        let mut c = Command::new("explorer.exe");
        c.arg(url);
        c
    };
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.spawn().is_ok()
}
fn login(args: &[String]) -> Result<()> {
    // Use the same effective configuration as repo commands, including
    // CMP_CONFIG and environment overrides. A missing config is allowed so
    // first-time users can sign in before running setup.
    let cfg = match load_config() {
        Ok((config, _)) => config,
        Err(error) if error.to_string() == "CMP is not configured. Run cmp setup" => {
            Config::new("cloud", CLOUD)
        }
        Err(error) => return Err(error),
    };
    if !["cloud", "offline"].contains(&cfg.backend.as_str()) || cfg.endpoint != CLOUD {
        return Err("cmp login requires CMP Cloud or offline mode with CMP Cloud as streaming destination".into());
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let redirect = format!(
        "http://127.0.0.1:{}/callback",
        listener.local_addr()?.port()
    );
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let verifier = URL_SAFE_NO_PAD.encode(bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    OsRng.fill_bytes(&mut bytes);
    let state = URL_SAFE_NO_PAD.encode(bytes);
    let url = url::Url::parse_with_params(
        &(cfg.endpoint.clone() + "/oauth/authorize"),
        &[
            ("client_id", "cmp-cli"),
            ("response_type", "code"),
            ("code_challenge_method", "S256"),
            ("code_challenge", &challenge),
            ("state", &state),
            ("scope", "repo:read repo:write offline_access"),
            ("redirect_uri", &redirect),
        ],
    )?;
    println!("Authorize CMP CLI in your browser. Never paste tokens into the terminal.");
    if flag(args, "--no-browser") || !open_url(url.as_str()) {
        println!("Open this authorization URL on the same device:\n{url}");
    }
    let start = Instant::now();
    let code = loop {
        if start.elapsed() > Duration::from_secs(180) {
            return Err("CMP login timed out".into());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_secs(4)))?;
                let mut buffer = [0u8; 4096];
                let n = stream.read(&mut buffer)?;
                let request = String::from_utf8_lossy(&buffer[..n]);
                let target = request
                    .lines()
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("");
                let parsed = url::Url::parse(&format!("http://127.0.0.1{target}"));
                let params = parsed.as_ref().ok().filter(|u| u.path() == "/callback");
                let valid_state = params
                    .and_then(|u| {
                        u.query_pairs()
                            .find(|(k, _)| k == "state")
                            .map(|(_, v)| v.to_string())
                    })
                    .is_some_and(|given| given == state);
                let authorized = if valid_state {
                    params.and_then(|u| {
                        u.query_pairs()
                            .find(|(k, _)| k == "code")
                            .map(|(_, v)| v.to_string())
                    })
                } else {
                    None
                };
                let body = if authorized.is_some() {
                    "CMP authorization received. You may return to your terminal."
                } else {
                    "CMP authorization was rejected or cancelled. Return to your terminal."
                };
                let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body);
                let _ = stream.write_all(response.as_bytes());
                if let Some(code) = authorized {
                    break code;
                }
                if params.is_some() {
                    return Err("CMP authorization denied or state validation failed".into());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(120))
            }
            Err(e) => return Err(e.into()),
        }
    };
    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "authorization_code")
        .append_pair("client_id", "cmp-cli")
        .append_pair("code", &code)
        .append_pair("code_verifier", &verifier)
        .append_pair("redirect_uri", &redirect)
        .finish();
    let token: Value = ureq::post(&(cfg.endpoint + "/api/oauth/token"))
        .set("Content-Type", "application/x-www-form-urlencoded")
        .timeout(Duration::from_secs(15))
        .send_string(&form)?
        .into_json()?;
    let refresh = token["refresh_token"]
        .as_str()
        .ok_or("No refresh token returned by CMP Cloud")?;
    let chosen = match argument(args, "--credential-store") {
        Some(value) => value,
        None if !io::stdin().is_terminal() => "none".into(),
        None if cmp_tui::confirm("Would you like to save in keychain?", true)? => "keychain".into(),
        None if cmp_tui::confirm("Save credentials in an encrypted CMP file instead?", true)? => "encrypted".into(),
        None => "none".into(),
    };
    let mode = match chosen.as_str() {
        "keychain" => cmp_credentials::Storage::Keychain,
        "encrypted" => cmp_credentials::Storage::EncryptedFile,
        "none" => cmp_credentials::Storage::None,
        _ => return Err("Credential store must be keychain, encrypted, or none".into()),
    };
    if let Err(error) = cmp_credentials::save(refresh, mode) {
        if mode == cmp_credentials::Storage::Keychain && io::stdin().is_terminal()
            && cmp_tui::confirm("Keychain unavailable. Save in an encrypted CMP file instead?", true)? {
            cmp_credentials::save(refresh, cmp_credentials::Storage::EncryptedFile)?;
        } else {
            // Do not leave an untracked refresh token active after storage failure.
            let _ = revoke_refresh(refresh);
            return Err(error);
        }
    }
    if mode == cmp_credentials::Storage::None {
        let _ = revoke_refresh(refresh);
        println!("CMP Cloud login completed; credentials were NOT saved. Run cmp login for a new session.");
    } else {
        println!("CMP Cloud authorization completed. Credential storage selection saved.");
    }
    Ok(())
}
fn revoke_refresh(refresh: &str) -> Result<()> {
    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", "cmp-cli")
        .append_pair("token", refresh)
        .finish();
    ureq::post(&format!("{CLOUD}/api/oauth/revoke"))
        .set("Content-Type", "application/x-www-form-urlencoded")
        .timeout(Duration::from_secs(7))
        .send_string(&form)?;
    Ok(())
}
fn logout() -> Result<()> {
    if let Ok(credential) = cmp_credentials::load() {
        // If Cloud is unreachable, do not silently claim the refresh token was revoked.
        revoke_refresh(&credential.token)?;
    }
    cmp_credentials::clear()?;
    println!("CMP Cloud credentials cleared and remote token revoked when available.");
    Ok(())
}

fn required_doctor_capabilities(backend: &str) -> &'static [&'static str] {
    if backend == "cloud" {
        // Cloud runtime writes are optional. Diagnostics must still verify
        // real authorized repository list and selected branch access.
        &["listRepositories", "getHead", "getSnapshot", "diff"]
    } else {
        &["listRepositories", "getHead", "getSnapshot", "createBranch",
          "commit", "compareAndSwap", "diff", "merge"]
    }
}

fn doctor(args: &[String]) -> Result<()> {
    let (cfg, path) = load_config()?;
    println!("Configuration: {}", path.display());
    println!("Backend: {} | Server: {}", cfg.backend, cfg.endpoint);

    // --offline means "do not contact a remote server", not "pretend an
    // embedded store exists" when the selected backend is Cloud or Core HTTP.
    if flag(args, "--offline") && cfg.backend != "offline" {
        println!("Remote checks: skipped (--offline). No local context store was checked.");
        return Ok(());
    }
    if cfg.backend == "offline" {
        let repos = cmp_offline::request(&cfg, "GET", "/v1/repos", None)?;
        let names = repos["repositories"].as_array()
            .ok_or("Invalid local CMP repository list")?;
        println!("Local context store: available (verified; no remote requests).");
        if let Some(repo) = &cfg.repository {
            if !names.iter().any(|name| name.as_str() == Some(repo)) {
                return Err(format!("Local context repository {repo} is missing. Run cmp init").into());
            }
            let head_path = format!(
                "/v1/repos/{}/head?branch={}",
                url::form_urlencoded::byte_serialize(repo.as_bytes()).collect::<String>(),
                url::form_urlencoded::byte_serialize(cfg.branch.as_bytes()).collect::<String>()
            );
            let head = cmp_offline::request(&cfg, "GET", &head_path, None)?;
            if head["head"].as_str().is_none_or(str::is_empty) {
                return Err("Local CMP repository returned no branch head".into());
            }
            println!("Selected repository: {repo} (branch {} accessible)", cfg.branch);
        } else {
            println!("Selected repository: not bound (run cmp init)");
        }
        println!("Streaming: {}", if cfg.streaming { "enabled" } else { "disabled" });
        println!("Pending stream operations: {}", cmp_stream::pending()?);
        return Ok(());
    }
    let caps: Value = if cfg.backend == "local" {
        context_request(&cfg, "GET", "/v1/capabilities", None)?
    } else {
        ureq::get(&(cfg.endpoint.clone() + "/v1/capabilities"))
            .timeout(Duration::from_secs(8))
            .call()?.into_json()?
    };
    let advertised = caps["capabilities"].as_object()
        .ok_or("CMP capability response is malformed: capabilities object missing")?;
    // Doctor is a *read/access* diagnostic for CMP Cloud. Global write
    // features may be unavailable in a read-only deployment and must never
    // turn an otherwise authorized read-only Cloud session into a false error.
    // Self-hosted Core still requires the full advertised Core contract.
    for feature in required_doctor_capabilities(&cfg.backend) {
        if advertised.get(*feature).and_then(Value::as_bool) != Some(true) {
            return Err(format!("CMP backend lacks required Core capability: {feature}").into());
        }
    }
    println!("Protocol capabilities: {}", caps["capabilities"]);
    if cfg.backend == "cloud" && ["createBranch", "commit", "merge"].iter().any(|feature|
        advertised.get(*feature).and_then(Value::as_bool) != Some(true)) {
        println!("Cloud write capabilities: unavailable (read-only runtime); repository access will still be checked");
    }

    // A 401/403 or malformed JSON is NOT a healthy doctor run. In particular,
    // CI and MCP installation instructions must never treat this as success.
    let list = match context_request(&cfg, "GET", "/v1/repos", None) {
        Ok(list) => list,
        Err(error) => {
            eprintln!("Authenticated repository API: unavailable ({error})");
            return Err(error);
        }
    };
    let names = list["repositories"].as_array()
        .ok_or("Authenticated repository API returned an invalid repository list")?;
    println!("Authenticated repository API: connected");
    if let Some(repo) = &cfg.repository {
        if !names.iter().any(|name| name.as_str() == Some(repo)) {
            return Err(format!(
                "Selected repository {repo} is not accessible. Check permissions or run cmp init"
            ).into());
        }
        let head_path = format!(
            "/v1/repos/{}/head?branch={}",
            url::form_urlencoded::byte_serialize(repo.as_bytes()).collect::<String>(),
            url::form_urlencoded::byte_serialize(cfg.branch.as_bytes()).collect::<String>()
        );
        let head = context_request(&cfg, "GET", &head_path, None)?;
        if head["head"].as_str().is_none_or(str::is_empty) {
            return Err("Selected CMP repository returned no branch head".into());
        }
        println!("Selected repository: {repo} (branch {} accessible)", cfg.branch);
    } else {
        println!("Selected repository: not bound (run cmp init)");
    }
    Ok(())
}
fn status() -> Result<()> {
    let (cfg, file) = load_config()?;
    println!(
        "Scope: {}\nBackend: {}\nEndpoint: {}\nRepository: {}\nBranch: {}\nStreaming: {}",
        if env::var_os("CMP_CONFIG").is_some() {
            "explicit"
        } else if global_file().is_ok_and(|global| global == file) {
            "global"
        } else {
            "project"
        },
        cfg.backend,
        cfg.endpoint,
        cfg.repository.as_deref().unwrap_or("not selected"),
        cfg.branch,
        if cfg.streaming { "enabled" } else { "disabled" }
    );
    println!("Config: {}", file.display());
    if cfg.backend == "offline" {
        println!("Storage: {}", file.parent().unwrap().join("state").display());
        println!("Pending stream operations: {}", cmp_stream::pending()?);
    }
    if cfg.backend == "cloud" || (cfg.backend == "offline" && cfg.streaming && cfg.endpoint == CLOUD) {
        let status = cmp_credentials::available_status()?;
        println!(
            "Authentication: {}",
            status
        );
    } else if cfg.backend == "local" {
        println!("Authentication: {}", if env::var("CMP_DEV_TOKEN").is_ok() { "local server token available" } else { "local server token required" });
    }
    Ok(())
}
fn init(args: &[String]) -> Result<()> {
    let (mut cfg, _from) = load_config()?;
    let noninteractive = flag(args, "--no-interactive");
    // Preserve legacy self-hosted setup: `init --offline` historically meant
    // skip the network check, not silently change an existing local-server config.
    if flag(args, "--offline") && cfg.backend != "local" {
        cfg.backend = "offline".into();
        cfg.streaming = false;
    }
    if flag(args, "--streaming") && flag(args, "--no-streaming") {
        return Err("Choose --streaming or --no-streaming, not both".into());
    }
    if flag(args, "--streaming") || flag(args, "--no-streaming") {
        if cfg.backend != "offline" { return Err("Streaming requires local / offline mode".into()); }
        cfg.streaming = flag(args, "--streaming");
    } else if cfg.backend == "offline" && !noninteractive {
        cfg.streaming = cmp_tui::confirm("Stream future local context updates to the configured server?", cfg.streaming)?;
    }
    // Inspect the current project before any remote repo creation.
    let path = project_file()?;
    let cli = if cfg.backend == "offline" || flag(args, "--offline") {
        None
    } else {
        Some(client(&cfg)?)
    };
    let repo = match argument(args, "--repo").or(cfg.repository.clone()) {
        Some(s) => s,
        None if noninteractive => {
            return Err("cmp init --repo owner/name is required without a TTY".into());
        }
        None => {
            require_tty()?;
            let mut repositories = if cfg.backend == "offline" {
                let values = cmp_offline::request(&cfg, "GET", "/v1/repos", None)?;
                values["repositories"].as_array().into_iter().flatten()
                    .filter_map(Value::as_str).map(ToOwned::to_owned).collect::<Vec<_>>()
            } else if let Some(c) = &cli {
                let values = c.list_repositories()?;
                values["repositories"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str())
                    .filter(|name| validate_repo(name).is_ok())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            repositories.sort();
            repositories.dedup();
            if repositories.is_empty() {
                prompt("CMP context repository (owner/name)", "")?
            } else {
                let mut options = repositories.clone();
                options.push("Create a new context repository...".into());
                let labels: Vec<&str> = options.iter().map(String::as_str).collect();
                let selected = cmp_tui::choose("Which CMP repository?", &labels, 0)?;
                if selected == repositories.len() {
                    prompt("New repository (owner/name)", "")?
                } else {
                    repositories[selected].clone()
                }
            }
        }
    };
    validate_repo(&repo)?;
    let preserve_existing = if path.exists() && !flag(args, "--force") {
        let current = load_file(&path)?;
        if current.backend != cfg.backend
            || current.endpoint != cfg.endpoint
            || current
                .repository
                .as_deref()
                .is_some_and(|name| name != repo)
        {
            return Err(
                "Project already has different CMP settings. Use --force to replace explicitly"
                    .into(),
            );
        }
        // Project setup can leave a repository unbound; init can fill it in.
        // If the binding already matches, leave it untouched.
        current.repository.as_deref() == Some(repo.as_str())
            && current.streaming == cfg.streaming
    } else {
        false
    };
    if let Some(c) = &cli {
        let repositories = c.list_repositories()?;
        let found = repositories["repositories"]
            .as_array()
            .is_some_and(|list| list.iter().any(|item| item.as_str() == Some(&repo)));
        if !found {
            let create = flag(args, "--create")
                || (!noninteractive
                    && cmp_tui::confirm(
                        &format!("Repository {repo} does not exist. Create it now?"),
                        true,
                    )?);
            if create {
                let visibility = match argument(args, "--visibility") {
                    Some(value) => value,
                    None if noninteractive => "private".into(),
                    None => {
                        let choices = [
                            "Private · only permitted members",
                            "Public · visible to everyone",
                        ];
                        if cmp_tui::choose("Repository visibility", &choices, 0)? == 0 {
                            "private".into()
                        } else {
                            "public".into()
                        }
                    }
                };
                if visibility != "private" && visibility != "public" {
                    return Err("Visibility must be private or public".into());
                }
                c.request(
                    "POST",
                    "/v1/repos",
                    Some(json!({"id":repo,"visibility":visibility})),
                )?;
            } else {
                return Err(
                    "Repository not accessible. Use --create or select an existing repository"
                        .into(),
                );
            }
        }
    }
    cfg.repository = Some(repo.clone());
    if !preserve_existing {
        save_config(&path, &cfg)?;
    }
    if cfg.backend == "offline" {
        let repositories = cmp_offline::request(&cfg, "GET", "/v1/repos", None)?;
        let found = repositories["repositories"].as_array()
            .is_some_and(|list| list.iter().any(|item| item.as_str() == Some(&repo)));
        if !found {
            context_request(&cfg, "POST", "/v1/repos", Some(json!({"id":repo,"visibility":"private"})))?;
        }
    }
    let docs = find_docs(&env::current_dir()?)?;
    cmp_tui::success(format!(
        "CMP project initialized: {repo} ({})",
        path.display()
    ));
    if !docs.is_empty() {
        println!(
            "Found {} documentation sources. No files were uploaded.",
            docs.len()
        );
        if !noninteractive
            && !flag(args, "--skip-import")
            && cmp_tui::confirm("Generate a documentation review prompt?", true)?
        {
            let review = path.parent().unwrap().join("import-review.md");
            if !review.exists() {
                let mut content=String::from("# CMP context evidence review\n\nVerify CMP MCP access before proposing any changes. Extract sourced claims, timestamps, and contradictions; retain both claims until reviewed. Do not upload secrets or overwrite existing records.\n\nDocuments to review:\n");
                for item in &docs {
                    content.push_str(&format!("- {}\n", item.display()))
                }
                fs::write(&review, content)?;
                println!("Review prompt: {}", review.display());
            }
        }
    }
    if !noninteractive && cmp_tui::confirm("Install CMP MCP for an agent?", true)? {
        let choices = ["Claude Code", "Codex", "OpenCode"];
        let host = ["claude", "codex", "opencode"]
            [cmp_tui::choose("Which agent should CMP connect to?", &choices, 0)?];
        cmp_agents::install(host)?;
        println!("CMP MCP installed. Hooks require a supported session backend and are not installed automatically.");
    }
    println!("Next: cmp doctor");
    Ok(())
}
fn find_docs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<()> {
        if depth > 8 || out.len() >= 200 {
            return Ok(());
        }
        for ent in fs::read_dir(dir)? {
            let ent = ent?;
            let path = ent.path();
            if ent.file_type()?.is_symlink() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_lowercase();
            if ent.file_type()?.is_dir() {
                if [
                    ".git",
                    ".cmp",
                    "node_modules",
                    "target",
                    "dist",
                    "build",
                    "out",
                    ".next",
                    ".venv",
                ]
                .contains(&name.as_str())
                {
                    continue;
                }
                walk(root, &path, depth + 1, out)?;
            } else if ent.file_type()?.is_file() {
                let ext = path
                    .extension()
                    .and_then(|x| x.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if ["md", "mdx", "mmd", "mermaid", "puml"].contains(&ext.as_str())
                    && ent.metadata()?.len() <= 1024 * 1024
                    && !name.starts_with(".env")
                {
                    if let Ok(relative) = path.strip_prefix(root) {
                        out.push(relative.to_path_buf())
                    }
                }
            }
            if out.len() >= 200 {
                break;
            }
        }
        Ok(())
    }
    walk(root, root, 0, &mut results)?;
    results.sort();
    Ok(results)
}
fn command(args: Vec<String>) -> Result<()> {
    #[cfg(feature = "cloud-fixture-tests")]
    if env::var("CMP_CLOUD_FIXTURE_MODE").as_deref() == Ok("1")
        && args.first().is_some_and(|name| !matches!(
            name.as_str(),
            "setup"|"fixture"|"init"|"int"|"doctor"|"repo"|"status"|
            "config"|"api"|"head"|"snapshot"|"branch"|"commit"|
            "diff"|"merge"|"stream"
        )) {
        return Err("Cloud fixture mode disallows browser login and unrelated commands".into());
    }
    // Match gh: nested --help never needs a configured server or token.
    if args.len()>1 && args.iter().skip(1).any(|v| v=="--help"||v=="-h")
        && matches!(args[0].as_str(),"repo"|"auth"|"config"|"branch"|"api") {
        cmp_gh::help_category(&args[0]);
        return Ok(());
    }
    if args.is_empty() {
        return help();
    }
    match args[0].as_str() {
        "help" if args.len()>1 => {cmp_gh::help_category(&args[1]);Ok(())},
        "--help" | "-h" | "help" => help(),
        "--version" | "-V" | "version" => {
            println!("cmp {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "setup" => setup(&args[1..]),
        #[cfg(feature = "cloud-fixture-tests")]
        "fixture" if args.get(1).is_some_and(|s| s == "seed-refresh") => {
            cmp_cloud_fixture::seed_refresh()
        },
        "login" => login(&args[1..]),
        "stream" if args.get(1).is_some_and(|x| x == "push") => {
            let (cfg, _) = load_config()?;
            println!("Flushed {} pending CMP stream operations.", cmp_stream::push(&cfg)?);
            Ok(())
        }
        "stream" if args.get(1).is_some_and(|x| x == "status") => {
            println!("Pending CMP stream operations: {}", cmp_stream::pending()?);
            Ok(())
        },
        "logout" => logout(),
        "auth" => cmp_gh::auth(&args[1..]),
        "browse" => cmp_gh::browse(&args[1..]),
        "api" => cmp_gh::api(&args[1..]),
        "repo" if args.get(1).is_some_and(|v| matches!(v.as_str(), "view"|"clone"|"set-default"|"help"|"--help")) || args.len()==1 => cmp_gh::repo(&args[1..]),
        "config" if args.get(1).is_some_and(|v| matches!(v.as_str(), "list"|"get"|"set"|"help"|"--help")) || args.len()==1 => cmp_gh::config(&args[1..]),
        "branch" if args.get(1).is_some_and(|v| v=="list") => cmp_gh::branch_list(),
        "branch" if args.get(1).is_some_and(|v| v=="--help"||v=="help") || args.len()==1 => {cmp_gh::help_category("branch");Ok(())},
        "status" => status(),
        "doctor" => doctor(&args[1..]),
        "init" | "int" => init(&args[1..]),
        "mcp" if args.get(1).is_some_and(|x| x == "serve") => cmp_mcp::serve(),
        "agent" if args.get(1).is_some_and(|x| x == "list") => {
            cmp_agents::list();
            Ok(())
        }
        "agent" if args.get(1).is_some_and(|x| x == "install") => cmp_agents::install(
            args.get(2)
                .map(String::as_str)
                .ok_or("Usage: cmp agent install claude|codex|opencode")?,
        ),
        "agent" if args.get(1).is_some_and(|x| x == "remove") => cmp_agents::remove(
            args.get(2)
                .map(String::as_str)
                .ok_or("Usage: cmp agent remove claude|codex|opencode")?,
        ),
        "config" if args.get(1).is_some_and(|x| x == "show") => status(),
        "repo" if args.get(1).is_some_and(|x| x == "list") => {
            let (cfg, _) = load_config()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&context_request(&cfg, "GET", "/v1/repos", None)?)?
            );
            Ok(())
        }
        "repo" if args.get(1).is_some_and(|x| x == "create") => {
            let repo = args
                .get(2)
                .ok_or("Usage: cmp repo create owner/name [--visibility private|public]")?;
            validate_repo(repo)?;
            if flag(&args,"--public") && flag(&args,"--private") {return Err("Choose --public or --private".into());}
            let visibility = if flag(&args,"--public") {"public".into()} else if flag(&args,"--private") {"private".into()} else {argument(&args,"--visibility").unwrap_or_else(|| "private".into())};
            if visibility != "private" && visibility != "public" {
                return Err("Visibility must be private or public".into());
            }
            let (cfg, _) = load_config()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&context_request(
                    &cfg, "POST", "/v1/repos",
                    Some(json!({"id":repo,"visibility":visibility}))
                )?)?
            );
            Ok(())
        }
        "head" | "snapshot" => {
            let (cfg, _) = load_config()?;
            let repo = cfg
                .repository
                .clone()
                .ok_or("No repo configured. Run cmp init")?;
            let reference = args.get(1).map(String::as_str).unwrap_or(&cfg.branch);
            let segment = if args[0] == "head" {
                "head?branch="
            } else {
                "records?ref="
            };
            let path = format!(
                "/v1/repos/{}/{}{}",
                url::form_urlencoded::byte_serialize(repo.as_bytes()).collect::<String>(),
                segment,
                url::form_urlencoded::byte_serialize(reference.as_bytes()).collect::<String>()
            );
            println!(
                "{}",
                serde_json::to_string_pretty(&context_request(&cfg, "GET", &path, None)?)?
            );
            Ok(())
        }
        "branch" if args.get(1).is_some_and(|x| x == "create") => {
            let name = args.get(2).ok_or("Usage: cmp branch create name [from]")?;
            let (cfg, _) = load_config()?;
            let repo = cfg
                .repository
                .clone()
                .ok_or("No repo configured. Run cmp init")?;
            let path = format!(
                "/v1/repos/{}/branches",
                url::form_urlencoded::byte_serialize(repo.as_bytes()).collect::<String>()
            );
            let from = args.get(3).map(String::as_str).unwrap_or(&cfg.branch);
            println!(
                "{}",
                serde_json::to_string_pretty(&context_request(
                    &cfg, "POST", &path,
                    Some(json!({"name":name,"from":from}))
                )?)?
            );
            Ok(())
        }
        "commit" => {
            let file = args.get(1).ok_or("Usage: cmp commit <changes.json>")?;
            let data = fs::metadata(file)?;
            if !data.is_file() || data.len() > 5_000_000 {
                return Err("Commit file must be a JSON file under 5 MB".into());
            }
            let mut payload: Value = serde_json::from_slice(&fs::read(file)?)?;
            if !payload["changes"].is_array() || !payload["message"].is_string() {
                return Err("Commit JSON requires message and changes array".into());
            }
            let (cfg, _) = load_config()?;
            let repo = cfg
                .repository
                .clone()
                .ok_or("No repo configured. Run cmp init")?;
            let branch = payload["branch"].as_str().unwrap_or(&cfg.branch).to_owned();
            // Never replace an explicit CAS precondition with a fresh head:
            // that would silently accept stale edits from another writer.
            let explicit = explicit_commit_head(&payload)?.map(str::to_owned);
            let expected = if let Some(head) = explicit {
                head
            } else {
                let path = format!(
                    "/v1/repos/{}/head?branch={}",
                    url::form_urlencoded::byte_serialize(repo.as_bytes()).collect::<String>(),
                    url::form_urlencoded::byte_serialize(branch.as_bytes()).collect::<String>()
                );
                let current = context_request(&cfg, "GET", &path, None)?;
                current["head"]
                    .as_str()
                    .ok_or("Branch head not returned")?
                    .to_owned()
            };
            payload["branch"] = json!(branch);
            payload["expectedHead"] = json!(expected);
            payload["authorId"] = json!("cmp-cli");
            let commit_path = format!(
                "/v1/repos/{}/commits",
                url::form_urlencoded::byte_serialize(repo.as_bytes()).collect::<String>()
            );
            println!(
                "{}",
                serde_json::to_string_pretty(&context_request(&cfg, "POST", &commit_path, Some(payload))?)?
            );
            Ok(())
        }
        "diff" => {
            let (cfg, _) = load_config()?;
            let repo = cfg
                .repository
                .clone()
                .ok_or("No repo configured. Run cmp init")?;
            let from = args.get(1).map(String::as_str).unwrap_or("main");
            let to = args.get(2).map(String::as_str).unwrap_or(&cfg.branch);
            let path = format!(
                "/v1/repos/{}/diff?from={}&to={}",
                url::form_urlencoded::byte_serialize(repo.as_bytes()).collect::<String>(),
                url::form_urlencoded::byte_serialize(from.as_bytes()).collect::<String>(),
                url::form_urlencoded::byte_serialize(to.as_bytes()).collect::<String>()
            );
            println!(
                "{}",
                serde_json::to_string_pretty(&context_request(&cfg, "GET", &path, None)?)?
            );
            Ok(())
        }
        "merge" => {
            let (cfg, _) = load_config()?;
            let repo = cfg
                .repository
                .clone()
                .ok_or("No repo configured. Run cmp init")?;
            let source = args.get(1).ok_or("Usage: cmp merge <source> [target]")?;
            let target = args.get(2).map(String::as_str).unwrap_or("main");
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &context_request(&cfg, "POST", &format!(
                        "/v1/repos/{}/merge",
                        url::form_urlencoded::byte_serialize(repo.as_bytes()).collect::<String>()
                    ), Some(json!({"source":source,"target":target,"authorId":"cmp-cli"})))?
                )?
            );
            Ok(())
        }
        _ => Err(
            "Unknown/unimplemented command. Use cmp --help for currently supported commands".into(),
        ),
    }
}
fn help() -> Result<()> {
    println!(r#"Work with CMP context repositories from the command line.

USAGE
  cmp <command> <subcommand> [flags]

CORE COMMANDS
  auth:          Authenticate CMP Cloud and manage saved credentials
  browse:        Open a context repository in your browser
  repo:          Create, list, view, clone, and select repositories
  branch:        Manage context repository branches
  agent:         Configure agent MCP integration

CONTEXT COMMANDS
  init:          Connect a project to CMP with .cmp/config.toml
  head:          Inspect a context branch head
  snapshot:      Retrieve versioned context records
  commit:        Commit knowledge changes with compare-and-swap
  diff:          Compare context revisions
  merge:         Merge context branches
  stream:        Manage opt-in local-to-server context streaming

ADDITIONAL COMMANDS
  api:           Call the configured CMP server API
  config:        Read and update non-secret CLI settings
  setup:         Configure Cloud, self-hosted, or offline CMP
  doctor:        Check the active backend and its capabilities
  status:        Inspect active configuration and authentication
  mcp:           Serve CMP tools to AI coding agents

FLAGS
  --help         Show help for a command
  --version      Show CMP version

EXAMPLES
  cmp auth login
  cmp repo create owner/knowledge --private
  cmp repo clone https://cloud.cmp.chat/owner/knowledge.cmp
  cmp repo view owner/knowledge --web
  cmp branch list
  cmp api /v1/repos

This is CMP context version control, not a Git implementation.
GitHub-only categories (issues, PRs, Actions, Codespaces) are not implemented.
Use `cmp <command> --help` to see available subcommands."#);
    Ok(())
}
fn main() {
    if let Err(e) = command(env::args().skip(1).collect()) {
        eprintln!("cmp: {e}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn block_http_remote() {
        assert!(validate_endpoint("http://server.internal:8787").is_err());
        assert!(validate_endpoint("https://cmp.example").is_ok());
        assert!(validate_endpoint(LOCAL).is_ok());
        assert!(validate_endpoint("https://person:pass@example.com").is_err());
        assert_eq!(
            normalize_endpoint("https://CLOUD.cmp.chat/").unwrap(),
            CLOUD
        );
        assert_eq!(
            normalize_endpoint("https://cloud.cmp.chat:443/").unwrap(),
            CLOUD
        );
    }
    #[test]
    fn discovers_nearest_project_from_nested_subdirectory() {
        let root = env::temp_dir().join(format!("cmp-config-{}", random_short()));
        fs::create_dir_all(root.join("src/deep")).unwrap();
        fs::create_dir_all(root.join(".cmp")).unwrap();
        fs::write(root.join(".cmp/config.toml"), "version=1").unwrap();
        let expected = root.join(".cmp/config.toml");
        assert_eq!(nearest_project_file(&root.join("src/deep")), Some(expected));
        fs::create_dir_all(root.join("src/.cmp")).unwrap();
        fs::write(root.join("src/.cmp/config.toml"), "version=1").unwrap();
        assert_eq!(
            nearest_project_file(&root.join("src/deep")),
            Some(root.join("src/.cmp/config.toml"))
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cloud_doctor_does_not_require_global_write_capabilities() {
        let cloud = required_doctor_capabilities("cloud");
        assert!(cloud.contains(&"listRepositories"));
        assert!(cloud.contains(&"getHead"));
        assert!(!cloud.contains(&"commit"));
        assert!(!cloud.contains(&"merge"));
        assert!(!cloud.contains(&"createBranch"));
        let core = required_doctor_capabilities("local");
        assert!(core.contains(&"commit"));
        assert!(core.contains(&"merge"));
        assert!(core.contains(&"createBranch"));
        assert!(core.contains(&"compareAndSwap"));
    }
    #[test]
    fn repo_identifier_validation() {
        assert!(validate_repo("user/repository").is_ok());
        assert!(validate_repo("/bad").is_err());
        assert!(validate_repo("user/bad/extra").is_err());
    }
    #[test]
    fn toml_roundtrip() {
        let mut cfg = Config::new("cloud", CLOUD);
        cfg.repository = Some("user/docs".into());
        let decoded: Config = toml::from_str(&toml::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(decoded, cfg);
    }
    #[test]
    fn explicit_commit_cas_head_is_not_replaced() {
        let stale = "a".repeat(64);
        let payload = json!({"expectedHead":stale});
        assert_eq!(
            explicit_commit_head(&payload).unwrap(),
            Some(stale.as_str())
        );
        assert_eq!(explicit_commit_head(&json!({})).unwrap(), None);
        for invalid in [
            json!({"expectedHead":null}),
            json!({"expectedHead":42}),
            json!({"expectedHead":"abc"}),
            json!({"expectedHead":"z".repeat(64)}),
        ] {
            assert!(explicit_commit_head(&invalid).is_err());
        }
    }
    #[test]
    fn no_symlink_import() {
        let root = env::temp_dir().join(format!("cmp-cli-test-{}", random_short()));
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/guide.md"), "document").unwrap();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git/secret.md"), "secret").unwrap();
        let found = find_docs(&root).unwrap();
        assert_eq!(found, vec![PathBuf::from("docs/guide.md")]);
        fs::remove_dir_all(root).unwrap();
    }
}
