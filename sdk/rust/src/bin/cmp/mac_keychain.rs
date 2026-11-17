// Use stable, Apple-signed /usr/bin/security instead of the ad-hoc signed CMP binary.
// Create a versioned Keychain item explicitly trusting ONLY this stable helper,
// so CLI rebuilds and refresh-token updates do not repeatedly reset permissions.
// The old item is migrated after one authorized read; never delete it first.
// For writes, send secrets through stdin to security -i, never argv.
#[cfg(target_os = "macos")]
pub(super) mod mac_keychain {
    use super::Result;
    use super::super::SERVICE;
    use std::{io::Write, process::{Command, Stdio}};
    const HELPER: &str = "/usr/bin/security";
    const ACCOUNT: &str = "default";
    // Legacy entries were created without an explicit trusted-app ACL.
    // A separate item avoids destructive delete-and-recreate migrations.
    const CURRENT_SERVICE: &str = "cmp.cloud.cli.v2";

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
    pub(super) fn get() -> Result<String> { get_with_migration(CURRENT_SERVICE,SERVICE,ACCOUNT) }
    pub(super) fn set(secret: &str) -> Result<()> {
        set_item(CURRENT_SERVICE,ACCOUNT,secret)?;
        // The new refresh token is safely stored before retiring the old item.
        let _ = delete_item(SERVICE,ACCOUNT);
        Ok(())
    }
    pub(super) fn delete() -> Result<()> {
        // Attempt both even if the first deletion fails, so logout cannot
        // silently leave one valid copy behind.
        let current = delete_item(CURRENT_SERVICE,ACCOUNT);
        let old = delete_item(SERVICE,ACCOUNT);
        current?;
        old
    }
    pub(super) fn available() -> bool {
        available_item(CURRENT_SERVICE,ACCOUNT)||available_item(SERVICE,ACCOUNT)
    }

    fn get_with_migration(current: &str, legacy: &str, account: &str) -> Result<String> {
        // If v2 exists but access is denied, do not fall back to a stale v1
        // refresh token. Report the Keychain access error instead.
        if available_item(current,account) {return get_item(current,account);}
        let token = get_item(legacy,account)?;
        set_item(current,account,&token)?;
        let _ = delete_item(legacy,account);
        Ok(token)
    }

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
    fn write_command(service:&str,account:&str,token:&str,existing:bool)->Result<String> {
        valid(service)?;valid(account)?;
        // -T is deliberately creation-only. Supplying -T on update may
        // request macOS change_acl authorization and cause another prompt.
        let operation=if existing {" -U"} else {""};
        let trust=if existing {""} else {" -T /usr/bin/security"};
        Ok(format!("add-generic-password{operation} -s {service} -a {account}{trust} -w {}\n",quote(token)?))
    }
    fn set_item(service:&str,account:&str,token:&str)->Result<()> {
        valid(service)?; valid(account)?;
        let line=write_command(service,account,token,available_item(service,account))?;
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
        fn trusted_acl_is_set_only_on_initial_creation() {
            let created=write_command("cmp.cli.test.permission","test","dummy",false).unwrap();
            assert!(created.contains(" -T /usr/bin/security "));
            assert!(!created.contains(" -U "));
            assert!(!created.contains(" -A "));
            let updated=write_command("cmp.cli.test.permission","test","dummy-next",true).unwrap();
            assert!(updated.contains(" -U "));
            assert!(!updated.contains(" -T "));
            assert!(!updated.contains(" -A "));
        }
        #[test]
        fn migrates_disposable_legacy_keychain_entry_without_deleting_it_first() {
            let mut bytes=[0u8;12];OsRng.fill_bytes(&mut bytes);
            let suffix=bytes.iter().map(|b|format!("{b:02x}")).collect::<String>();
            let legacy=format!("cmp.cli.test.migration.old.{suffix}");
            let current=format!("cmp.cli.test.migration.new.{suffix}");
            struct Cleanup(String,String);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    let _=delete_item(&self.0,"test");
                    let _=delete_item(&self.1,"test");
                }
            }
            let _cleanup=Cleanup(current.clone(),legacy.clone());
            set_item(&legacy,"test","fake-refresh-previous").unwrap();
            assert!(!available_item(&current,"test"));
            assert_eq!(
                get_with_migration(&current,&legacy,"test").unwrap(),
                "fake-refresh-previous"
            );
            assert!(available_item(&current,"test"));
            assert!(!available_item(&legacy,"test"));
            // Rotating in place keeps the trusted ACL established at creation.
            set_item(&current,"test","fake-refresh-rotated").unwrap();
            for _ in 0..3 {
                assert_eq!(get_with_migration(&current,&legacy,"test").unwrap(),"fake-refresh-rotated");
            }
        }
        #[test]
        fn reject_batch_command_injection() {
            assert!(quote("token\nmalicious").is_err());
            assert!(quote("token\rinject").is_err());
            assert!(quote("token\0inject").is_err());
        }
    }
}
