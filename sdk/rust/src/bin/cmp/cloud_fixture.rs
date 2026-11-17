//! Compile-time-only bridge for real, disposable CMP Cloud HTTP fixture tests.
//! Not compiled into normal CLI binaries or published SDKs.
use super::{cmp_credentials, home, load_config, Config, Result, CLOUD};
use std::{env, fs, path::Path};

fn loopback_fixture_origin(endpoint: &str) -> bool {
    let Ok(url) = url::Url::parse(endpoint) else { return false; };
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path() == "/"
}

fn isolated_session(home_path: &Path, session: &str) -> Result<bool> {
    if session.len() < 32 || !session.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(false);
    }
    let actual = home_path.canonicalize()?;
    let tmp = env::temp_dir().canonicalize()?;
    if actual == tmp || !actual.starts_with(&tmp) { return Ok(false); }
    if let Some(real_home) = env::var_os("HOME") {
        let real = std::path::PathBuf::from(real_home);
        if real.exists() && actual == real.canonicalize()? { return Ok(false); }
    }
    let marker = actual.join(".cmp/fixture-session");
    let meta = match fs::symlink_metadata(&marker) {
        Ok(meta) => meta,
        Err(_) => return Ok(false),
    };
    if !meta.file_type().is_file() || meta.file_type().is_symlink() { return Ok(false); }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 { return Ok(false); }
    }
    Ok(fs::read_to_string(marker)?.trim() == session)
}

/// A test binary only accepts a locally bound Cloud endpoint when an isolated
/// temp-home marker exactly matches a caller-provided random session ID.
/// No production/cloud.cmp.chat origin may be used while fixture mode is on.
pub fn authorized(cfg: &Config) -> Result<bool> {
    if env::var("CMP_CLOUD_FIXTURE_MODE").as_deref() != Ok("1") {
        return Ok(false);
    }
    if cfg.endpoint == CLOUD {
        return Err("Cloud fixture mode forbids the production CMP Cloud origin".into());
    }
    if !loopback_fixture_origin(&cfg.endpoint) {
        return Err("Cloud fixture mode requires http://127.0.0.1:<port> only".into());
    }
    let session = env::var("CMP_CLOUD_FIXTURE_SESSION")
        .map_err(|_| "Missing disposable Cloud fixture session marker")?;
    if !isolated_session(&home()?, &session)? {
        return Err("CMP Cloud fixture refused: isolated temporary home marker mismatch".into());
    }
    Ok(true)
}

/// Fixture-issued cmpr_ refresh grants enter via environment only in this
/// test-feature binary. They are written with real Argon2id/XChaCha20-Poly1305.
/// IMPORTANT: do NOT call normal save(EncryptedFile): that deletes the user's
/// shared macOS Keychain credential, even when CMP_HOME is disposable.
pub fn seed_refresh() -> Result<()> {
    let (cfg, _) = load_config()?;
    if cfg.backend != "cloud" || !authorized(&cfg)? {
        return Err("Fixture token seeding requires an isolated Cloud fixture".into());
    }
    let token = env::var("CMP_CLOUD_FIXTURE_REFRESH_TOKEN")
        .map_err(|_| "Missing disposable fixture refresh grant")?;
    if !token.starts_with("cmpr_")
        || token.len() < 16
        || token.len() > 4096
        || token.chars().any(char::is_control)
    {
        return Err("Invalid disposable CMP Cloud refresh grant".into());
    }
    cmp_credentials::seed_fixture_encrypted(&token)?;
    println!("Disposable fixture refresh grant saved to isolated encrypted CMP store.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::loopback_fixture_origin;
    #[test]
    fn marker_only_authorizes_isolated_temporary_home() {
        use std::{fs, time::{SystemTime,UNIX_EPOCH}};
        let unique=SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let home=std::env::temp_dir().join(format!("cmp-fixture-{}-{unique}",std::process::id()));
        fs::create_dir_all(home.join(".cmp")).unwrap();
        let marker=home.join(".cmp/fixture-session");
        let session="0123456789abcdef0123456789abcdef";
        fs::write(&marker,session).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&marker,fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(super::isolated_session(&home,session).unwrap());
        assert!(!super::isolated_session(&home,"abcdef0123456789abcdef0123456789").unwrap());
        assert!(!super::isolated_session(&home,"too-short").unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&marker,fs::Permissions::from_mode(0o644)).unwrap();
            assert!(!super::isolated_session(&home,session).unwrap());
        }
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn fixture_origin_never_accepts_public_or_external_endpoint() {
        assert!(loopback_fixture_origin("http://127.0.0.1:45670"));
        for bad in [
            "https://cloud.cmp.chat", "http://localhost:45670",
            "https://127.0.0.1:45670", "http://127.0.0.2:45670",
            "http://127.0.0.1", "http://127.0.0.1:45670/attack",
            "http://127.0.0.1:45670?token=x",
            "http://name:pass@127.0.0.1:45670",
            "http://192.168.1.1:45670",
        ] { assert!(!loopback_fixture_origin(bad), "{bad}"); }
    }
}
