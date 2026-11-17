//! Cross-platform CMP OAuth refresh credential storage.
//! OS vault use is always opt-in after the browser authorization callback.
//! Fallback is an Argon2id-derived XChaCha20-Poly1305 encrypted file.
use super::{home, private_dir, write_secure_file, Result};
#[cfg(not(target_os = "macos"))]
use super::SERVICE;
#[cfg(target_os = "macos")]
include!("mac_keychain.rs");
use argon2::Argon2;
use base64::{engine::general_purpose::STANDARD, Engine};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{self, IsTerminal},
    path::PathBuf,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u8,
    salt: String,
    nonce: String,
    ciphertext: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Storage {
    Keychain,
    EncryptedFile,
    None,
}
impl Storage {
    fn marker(self) -> &'static str {
        match self {
            Self::Keychain => "keychain",
            Self::EncryptedFile => "encrypted",
            Self::None => "none",
        }
    }
}
pub struct Credential {
    pub token: String,
    pub storage: Storage,
    passphrase: Option<String>,
}
fn vault_dir() -> Result<PathBuf> {
    let dir = home()?.join(".cmp");
    private_dir(&dir)?;
    Ok(dir)
}
fn marker_file() -> Result<PathBuf> {
    Ok(vault_dir()?.join("credential-store"))
}
fn encrypted_file() -> Result<PathBuf> {
    Ok(vault_dir()?.join("credentials.enc"))
}
#[cfg(not(target_os = "macos"))]
fn entry() -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(SERVICE, "default")?)
}
#[cfg(target_os = "macos")]
fn keychain_get() -> Result<String> { mac_keychain::get() }
#[cfg(not(target_os = "macos"))]
fn keychain_get() -> Result<String> { Ok(entry()?.get_password()?) }
#[cfg(target_os = "macos")]
fn keychain_set(value: &str) -> Result<()> { mac_keychain::set(value) }
#[cfg(not(target_os = "macos"))]
fn keychain_set(value: &str) -> Result<()> { Ok(entry()?.set_password(value)?) }
#[cfg(target_os = "macos")]
fn keychain_delete() -> Result<()> { mac_keychain::delete() }
#[cfg(not(target_os = "macos"))]
fn keychain_delete() -> Result<()> { Ok(entry()?.delete_credential()?) }
#[cfg(target_os = "macos")]
fn keychain_available() -> bool { mac_keychain::available() }
#[cfg(not(target_os = "macos"))]
fn keychain_available() -> bool { keychain_get().is_ok() }
fn passphrase(confirm: bool) -> Result<String> {
    if let Ok(value) = env::var("CMP_CREDENTIAL_PASSPHRASE") {
        if value.len() < 12 {
            return Err("CMP_CREDENTIAL_PASSPHRASE must contain at least 12 characters".into());
        }
        return Ok(value);
    }
    if !io::stdin().is_terminal() {
        return Err("Encrypted CMP credentials require a terminal or CMP_CREDENTIAL_PASSPHRASE environment variable (do not put passwords in project configuration)".into());
    }
    let value = rpassword::prompt_password("CMP credential encryption passphrase: ")?;
    if value.len() < 12 {
        return Err("Use a credential passphrase of at least 12 characters".into());
    }
    if confirm && rpassword::prompt_password("Confirm passphrase: ")? != value {
        return Err("Passphrases did not match".into());
    }
    Ok(value)
}
fn cipher_key(pass: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(pass.as_bytes(), salt, &mut key)
        .map_err(|_| "Unable to derive CMP credential encryption key")?;
    Ok(key)
}
fn encrypt(token: &str, pass: &str) -> Result<Envelope> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);
    let key = cipher_key(pass, &salt)?;
    let cipher =
        XChaCha20Poly1305::new_from_slice(&key).map_err(|_| "Credential encryption key invalid")?;
    let data = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: token.as_bytes(),
                aad: b"cmp-oauth-refresh-v1",
            },
        )
        .map_err(|_| "Unable to encrypt CMP credentials")?;
    Ok(Envelope {
        version: 1,
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(data),
    })
}
fn decrypt(file: &Envelope, pass: &str) -> Result<String> {
    if file.version != 1 {
        return Err("Unsupported encrypted CMP credential version".into());
    }
    let salt = STANDARD.decode(&file.salt)?;
    let nonce = STANDARD.decode(&file.nonce)?;
    let ciphertext = STANDARD.decode(&file.ciphertext)?;
    if salt.len() != 16 || nonce.len() != 24 {
        return Err("Invalid encrypted CMP credential format".into());
    }
    let key = cipher_key(pass, &salt)?;
    let cipher =
        XChaCha20Poly1305::new_from_slice(&key).map_err(|_| "Credential decryption key invalid")?;
    let raw = cipher
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: b"cmp-oauth-refresh-v1",
            },
        )
        .map_err(|_| "Incorrect CMP passphrase or damaged encrypted credentials")?;
    Ok(String::from_utf8(raw)?)
}
fn save_encrypted_at(file: &std::path::Path, token: &str, pass: &str) -> Result<()> {
    let blob = encrypt(token, pass)?;
    write_secure_file(file, &serde_json::to_string(&blob)?, true)
}
fn load_encrypted_at(file: &std::path::Path, pass: &str) -> Result<String> {
    let blob: Envelope = serde_json::from_slice(&fs::read(file)?)?;
    decrypt(&blob, pass)
}
pub fn stored_mode() -> Result<Option<Storage>> {
    let marker = marker_file()?;
    if marker.is_file() {
        return Ok(match fs::read_to_string(marker)?.trim() {
            "keychain" => Some(Storage::Keychain),
            "encrypted" => Some(Storage::EncryptedFile),
            "none" => Some(Storage::None),
            _ => return Err("Unrecognized CMP credential storage selection".into()),
        });
    }
    // Backward compatibility for existing CMP installations.
    if encrypted_file()?.is_file() {
        Ok(Some(Storage::EncryptedFile))
    } else {
        Ok(Some(Storage::Keychain))
    }
}
pub fn available_status() -> Result<&'static str> {
    match stored_mode()? {
        Some(Storage::EncryptedFile) => Ok(if encrypted_file()?.exists() {
            "encrypted file (passphrase required)"
        } else {
            "not authenticated"
        }),
        Some(Storage::Keychain) => Ok(
            if keychain_available()
            {
                "system credential vault"
            } else {
                "not authenticated / OS keychain unavailable"
            },
        ),
        _ => Ok("not saved"),
    }
}
fn write_mode(mode: Storage) -> Result<()> {
    write_secure_file(&marker_file()?, mode.marker(), true)
}
pub fn save(token: &str, mode: Storage) -> Result<()> {
    match mode {
        Storage::Keychain => {
            keychain_set(token)?;
            write_mode(mode)?;
            if encrypted_file()?.exists() {
                fs::remove_file(encrypted_file()?)?;
            }
        }
        Storage::EncryptedFile => {
            let pass = passphrase(true)?;
            save_encrypted_at(&encrypted_file()?, token, &pass)?;
            write_mode(mode)?;
            let _ = keychain_delete();
        }
        Storage::None => {
            clear()?;
            write_mode(Storage::None)?;
        }
    }
    Ok(())
}
pub fn load() -> Result<Credential> {
    match stored_mode()? {
        Some(Storage::Keychain) => Ok(Credential {
            token: keychain_get().map_err(|_| {
                "CMP Cloud is not signed in or system credential vault unavailable. Run cmp login"
            })?,
            storage: Storage::Keychain,
            passphrase: None,
        }),
        Some(Storage::EncryptedFile) => {
            let pass = passphrase(false)?;
            Ok(Credential {
                token: load_encrypted_at(&encrypted_file()?, &pass)?,
                storage: Storage::EncryptedFile,
                passphrase: Some(pass),
            })
        }
        _ => Err("CMP Cloud credentials were not saved. Run cmp login".into()),
    }
}
pub fn rotate(credential: &Credential, next: &str) -> Result<()> {
    match credential.storage {
        Storage::Keychain => {
            keychain_set(next)?;
        }
        Storage::EncryptedFile => {
            let pass = credential
                .passphrase
                .as_ref()
                .ok_or("Missing CMP credential decryption passphrase")?;
            save_encrypted_at(&encrypted_file()?, next, pass)?;
        }
        Storage::None => return Err("Cannot rotate unsaved CMP credentials".into()),
    }
    Ok(())
}
pub fn clear() -> Result<()> {
    let _ = keychain_delete();
    let file = encrypted_file()?;
    if file.exists() {
        fs::remove_file(file)?;
    }
    let marker = marker_file()?;
    if marker.exists() {
        fs::remove_file(marker)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_file_persistence_rotation_and_tamper_fail_closed() {
        use rand::{rngs::OsRng, RngCore};
        let mut entropy = [0u8; 12];
        OsRng.fill_bytes(&mut entropy);
        let name: String = entropy.iter().map(|byte| format!("{byte:02x}")).collect();
        let directory = std::env::temp_dir().join(format!("cmp-credential-fixture-{name}"));
        std::fs::create_dir(&directory).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
        }
        let _cleanup = Cleanup(directory.clone());
        let file = directory.join("credentials.enc");
        let pass = "credential file roundtrip passphrase of sufficient length";

        save_encrypted_at(&file, "refresh-old-local-fixture", pass).unwrap();
        let before = std::fs::read_to_string(&file).unwrap();
        assert!(!before.contains("refresh-old-local-fixture"));
        assert!(!before.contains(pass));
        assert_eq!(load_encrypted_at(&file, pass).unwrap(), "refresh-old-local-fixture");
        assert!(load_encrypted_at(&file, "a completely incorrect passphrase").is_err());

        save_encrypted_at(&file, "refresh-rotated-local-fixture", pass).unwrap();
        let after = std::fs::read_to_string(&file).unwrap();
        assert_ne!(before, after);
        assert!(!after.contains("refresh-old-local-fixture"));
        assert!(!after.contains("refresh-rotated-local-fixture"));
        assert_eq!(load_encrypted_at(&file, pass).unwrap(), "refresh-rotated-local-fixture");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let mut envelope: Envelope = serde_json::from_str(&after).unwrap();
        envelope.ciphertext = STANDARD.encode(b"corrupted local test ciphertext");
        std::fs::write(&file, serde_json::to_string(&envelope).unwrap()).unwrap();
        assert!(load_encrypted_at(&file, pass).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn encrypted_file_rejects_symlink_without_overwriting_target() {
        use std::os::unix::fs::symlink;
        let mut entropy=[0u8;12];
        OsRng.fill_bytes(&mut entropy);
        let id: String = entropy.iter().map(|byte| format!("{byte:02x}")).collect();
        let directory=std::env::temp_dir().join(format!("cmp-credential-symlink-{id}"));
        fs::create_dir(&directory).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) { let _=fs::remove_dir_all(&self.0); }
        }
        let _cleanup=Cleanup(directory.clone());
        let victim=directory.join("victim.txt");
        fs::write(&victim,"unchanged").unwrap();
        let link=directory.join("credentials.enc");
        symlink(&victim,&link).unwrap();
        assert!(save_encrypted_at(&link,"must-not-appear","safe credential passphrase").is_err());
        assert_eq!(fs::read_to_string(&victim).unwrap(),"unchanged");
    }
    #[test]
    fn encrypted_store_roundtrip_and_tamper_detection() {
        let pass = "an adequately long local test password";
        let data = encrypt("secret-refresh-token", pass).unwrap();
        assert!(!serde_json::to_string(&data)
            .unwrap()
            .contains("secret-refresh-token"));
        assert_eq!(decrypt(&data, pass).unwrap(), "secret-refresh-token");
        assert!(decrypt(&data, "an entirely different password").is_err());
        let mut changed = data;
        changed.ciphertext = STANDARD.encode(b"garbage");
        assert!(decrypt(&changed, pass).is_err());
    }
}
