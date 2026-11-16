// Use stable, Apple-signed /usr/bin/security instead of the ad-hoc signed CMP binary.
// The security helper's trusted application identity persists across CLI rebuilds.
// For writes, send the secret through stdin to security -i, never argv.
#[cfg(target_os = "macos")]
pub(super) mod mac_keychain {
    use super::{Result, SERVICE};
    use std::{io::Write, process::{Command, Stdio}};
    const HELPER: &str = "/usr/bin/security";
    const ACCOUNT: &str = "default";

    fn cmd(action: &str, service: &str, account: &str) -> Command {
        let mut c = Command::new(HELPER);
        c.arg(action).arg("-s").arg(service).arg("-a").arg(account);
        c
    }
    fn valid(label: &str) -> Result<()> {
        if label.is_empty() || !label.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) {
            return Err("Invalid CMP Keychain item name".into());
        }
        Ok(())
    }
    fn quote(token: &str) -> Result<String> {
        if token.is_empty() || token.len() > 16_384 || token.chars().any(char::is_control) {
            return Err("Invalid CMP Keychain credential data".into());
        }
        Ok(format!("\"{}\"",token.replace('\\',"\\\\").replace('"',"\\\"")))
    }
    pub(super) fn get() -> Result<String> { get_item(SERVICE,ACCOUNT) }
    pub(super) fn set(secret: &str) -> Result<()> { set_item(SERVICE,ACCOUNT,secret) }
    pub(super) fn delete() -> Result<()> { delete_item(SERVICE,ACCOUNT) }
    pub(super) fn available() -> bool { available_item(SERVICE,ACCOUNT) }

    fn available_item(service:&str,account:&str) -> bool {
        cmd("find-generic-password",service,account)
            .stdout(Stdio::null()).stderr(Stdio::null()).status()
            .is_ok_and(|s|s.success())
    }
    fn get_item(service:&str,account:&str)->Result<String> {
        valid(service)?; valid(account)?;
        let result=cmd("find-generic-password",service,account).arg("-w")
            .output().map_err(|_|"macOS Keychain helper unavailable")?;
        if !result.status.success() {
            return Err("CMP Keychain credential unavailable. Allow /usr/bin/security to read CMP once, or run cmp login".into());
        }
        let mut value=String::from_utf8(result.stdout)?;
        if value.ends_with('\n') { value.pop(); if value.ends_with('\r') {value.pop();} }
        if value.is_empty() {return Err("CMP Keychain credential empty".into());}
        Ok(value)
    }
    fn set_item(service:&str,account:&str,token:&str)->Result<()> {
        valid(service)?; valid(account)?;
        let line=format!("add-generic-password -U -s {service} -a {account} -w {}\n",quote(token)?);
        let mut child=Command::new(HELPER).arg("-i").stdin(Stdio::piped())
            .stdout(Stdio::null()).stderr(Stdio::null()).spawn()
            .map_err(|_|"macOS Keychain helper unavailable")?;
        {
            let mut stdin=child.stdin.take().ok_or("macOS Keychain helper stdin unavailable")?;
            stdin.write_all(line.as_bytes())?;
        }
        if !child.wait()?.success() {return Err("CMP Keychain write failed".into());}
        // Interactive security commands can exit 0 even when an operation fails.
        if get_item(service,account)? != token {
            return Err("CMP Keychain did not retain its updated credential".into());
        }
        Ok(())
    }
    fn delete_item(service:&str,account:&str)->Result<()> {
        valid(service)?;valid(account)?;
        let status=cmd("delete-generic-password",service,account)
            .stdout(Stdio::null()).stderr(Stdio::null()).status()?;
        if !status.success() && available_item(service,account) {
            return Err("CMP Keychain credential deletion failed".into());
        }
        Ok(())
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use rand::{rngs::OsRng,RngCore};
        #[test]
        fn stable_helper_roundtrip_with_unique_disposable_entry() {
            let mut bytes=[0u8;12];OsRng.fill_bytes(&mut bytes);
            let service=format!("cmp.cli.test.{}",bytes.iter().map(|b|format!("{b:02x}")).collect::<String>());
            struct Cleanup(String);
            impl Drop for Cleanup { fn drop(&mut self){let _=delete_item(&self.0,"test");} }
            let _cleanup=Cleanup(service.clone());
            set_item(&service,"test","safe-dummy-token-123").unwrap();
            assert_eq!(get_item(&service,"test").unwrap(),"safe-dummy-token-123");
            set_item(&service,"test","safe-dummy-ROTATED_.-token").unwrap();
            assert_eq!(get_item(&service,"test").unwrap(),"safe-dummy-ROTATED_.-token");
            delete_item(&service,"test").unwrap();
            assert!(!available_item(&service,"test"));
        }
        #[test]
        fn reject_batch_command_injection() {
            assert!(quote("token\nmalicious").is_err());
            assert!(quote("token\rinject").is_err());
            assert!(quote("token\0inject").is_err());
        }
    }
}
