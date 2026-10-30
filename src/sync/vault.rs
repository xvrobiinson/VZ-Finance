//! Secrets kept by Windows for the signed-in Windows user:
//!
//! - the cloud session, in Credential Manager (one entry per deployment), so
//!   it is never in a file next to the database or in a backup;
//! - ID photos kept for offline use, encrypted with DPAPI, so a copied file
//!   or another Windows user on the same PC can't read them.

use crate::sync::auth::Session;

/// What is remembered about a signed-in session.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Stored {
    pub email: String,
    #[serde(with = "session_text")]
    pub session: Session,
}

mod session_text {
    use super::Session;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(s: &Session, out: S) -> Result<S::Ok, S::Error> {
        out.serialize_str(s.as_str())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Session, D::Error> {
        Ok(Session::from_stored(String::deserialize(d)?))
    }
}

/// The Credential Manager entry for a deployment.
fn target(cloud: &str) -> String {
    format!("Acervus Finance/sync/{cloud}")
}

pub fn save(cloud: &str, stored: &Stored) -> Result<(), String> {
    let json = serde_json::to_vec(stored).map_err(|e| e.to_string())?;
    imp::write(&target(cloud), &stored.email, &json)
}

/// The remembered session, or `None` when there is none (or it can't be read).
pub fn load(cloud: &str) -> Option<Stored> {
    let bytes = imp::read(&target(cloud))?;
    serde_json::from_slice(&bytes).ok()
}

pub fn forget(cloud: &str) {
    imp::delete(&target(cloud));
}

/// `data` encrypted for the current Windows user.
pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    imp::protect(data)
}

/// The original bytes of something `protect` encrypted, for the same Windows
/// user.
pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    imp::unprotect(data)
}

/// A value nobody can guess (Windows' cryptographic random generator), as
/// `len` letters and digits.
pub fn random_token(len: usize) -> Result<String, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut out = String::with_capacity(len);
    while out.len() < len {
        let mut bytes = [0u8; 64];
        imp::random(&mut bytes)?;
        // 248 = 4 × 62: dropping larger bytes keeps every letter equally likely.
        out.extend(bytes.iter().filter(|&&b| b < 248).map(|&b| ALPHABET[(b % 62) as usize] as char).take(len - out.len()));
    }
    Ok(out)
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Credentials::{
        CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW,
    };
    use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData};
    use windows::core::{HSTRING, PCWSTR, PWSTR};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn write(target: &str, user: &str, blob: &[u8]) -> Result<(), String> {
        let mut target = wide(target);
        let mut user = wide(user);
        let mut blob = blob.to_vec();
        let cred = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_mut_ptr()),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(user.as_mut_ptr()),
            ..Default::default()
        };
        // SAFETY: every pointer refers to a buffer that lives until the call
        // returns; Windows copies what it keeps.
        unsafe { CredWriteW(&cred, 0) }.map_err(|e| e.message())
    }

    pub fn read(target: &str) -> Option<Vec<u8>> {
        let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: on success Windows hands back one allocation, read here and
        // released with CredFree.
        unsafe {
            CredReadW(&HSTRING::from(target), CRED_TYPE_GENERIC, None, &mut cred).ok()?;
            let c = &*cred;
            let bytes = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec();
            CredFree(cred as *const _);
            Some(bytes)
        }
    }

    pub fn delete(target: &str) {
        // SAFETY: a plain call with a string that outlives it. A missing entry
        // is fine.
        let _ = unsafe { CredDeleteW(&HSTRING::from(target), CRED_TYPE_GENERIC, None) };
    }

    fn crypt(data: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: `input` points at `data` for the duration of the call; the
        // output is one LocalAlloc'd buffer, copied and then freed.
        unsafe {
            if encrypt {
                CryptProtectData(&input, PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            } else {
                CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            }
            .map_err(|e| e.message())?;
            let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            LocalFree(Some(HLOCAL(out.pbData as _)));
            Ok(bytes)
        }
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
        crypt(data, true)
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
        crypt(data, false)
    }

    pub fn random(buf: &mut [u8]) -> Result<(), String> {
        use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};
        // SAFETY: Windows fills `buf`, which outlives the call.
        unsafe { BCryptGenRandom(None, buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.ok().map_err(|e| e.message())
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn write(_: &str, _: &str, _: &[u8]) -> Result<(), String> {
        Err("sync needs Windows".into())
    }
    pub fn read(_: &str) -> Option<Vec<u8>> {
        None
    }
    pub fn delete(_: &str) {}
    pub fn protect(_: &[u8]) -> Result<Vec<u8>, String> {
        Err("sync needs Windows".into())
    }
    pub fn unprotect(_: &[u8]) -> Result<Vec<u8>, String> {
        Err("sync needs Windows".into())
    }
    pub fn random(_: &mut [u8]) -> Result<(), String> {
        Err("sync needs Windows".into())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn photos_round_trip_and_are_not_stored_in_the_clear() {
        let photo = b"\xff\xd8\xff not really a jpeg";
        let sealed = protect(photo).unwrap();
        assert!(!sealed.windows(photo.len()).any(|w| w == photo));
        assert_eq!(unprotect(&sealed).unwrap(), photo);
        assert!(unprotect(b"garbage").is_err());
    }

    #[test]
    fn random_tokens_are_letters_and_digits_and_differ() {
        let a = random_token(40).unwrap();
        assert_eq!(a.len(), 40);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(a, random_token(40).unwrap());
    }

    #[test]
    fn sessions_are_kept_and_forgotten() {
        let cloud = format!("test://vault-{}", std::process::id());
        let stored = Stored { email: "test@example.com".into(), session: Session::from_stored("a=1; b=2".into()) };
        save(&cloud, &stored).unwrap();
        assert_eq!(load(&cloud), Some(stored));
        forget(&cloud);
        assert_eq!(load(&cloud), None);
    }
}
