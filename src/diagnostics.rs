//! Facts that help explain a problem on another computer: Windows version,
//! where data lives and whether those folders can be written. Shown in
//! Settings and written to the log at startup. Never includes financial data.

use std::path::{Path, PathBuf};

/// The running Windows version, e.g. "Windows 10.0 build 19045".
pub fn windows_version() -> Option<String> {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct OsVersionInfo {
            size: u32,
            major: u32,
            minor: u32,
            build: u32,
            platform: u32,
            service_pack: [u16; 128],
        }
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn RtlGetVersion(info: *mut OsVersionInfo) -> i32;
        }
        let mut info =
            OsVersionInfo { size: size_of::<OsVersionInfo>() as u32, major: 0, minor: 0, build: 0, platform: 0, service_pack: [0; 128] };
        // SAFETY: `info` is a valid, correctly sized OSVERSIONINFOW.
        let status = unsafe { RtlGetVersion(&mut info) };
        if status != 0 {
            return None;
        }
        let name = if info.major == 10 && info.build >= 22000 { "Windows 11" } else { "Windows" };
        Some(format!("{name} ({}.{} build {})", info.major, info.minor, info.build))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// The folder holding the log file.
pub fn log_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("VZFinance").join("logs"))
}

/// Creates `dir` if needed and checks a file can be written and removed in
/// it. Returns the error in words when it can't.
pub fn check_writable(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{e}"))?;
    let probe = dir.join(format!(".write-check-{}", std::process::id()));
    std::fs::write(&probe, b"ok").map_err(|e| format!("{e}"))?;
    std::fs::remove_file(&probe).map_err(|e| format!("{e}"))?;
    Ok(())
}

/// One folder the app needs, and whether it can write there.
#[derive(Clone, Debug)]
pub struct FolderCheck {
    pub label: &'static str,
    pub path: PathBuf,
    pub result: Result<(), String>,
}

pub fn check_folders(folders: &[(&'static str, PathBuf)]) -> Vec<FolderCheck> {
    folders.iter().map(|(label, path)| FolderCheck { label, path: path.clone(), result: check_writable(path) }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_writable_and_unwritable_folders() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_writable(&dir.path().join("nested").join("ok")).is_ok());
        // A path below a regular file can't be created.
        let file = dir.path().join("file.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(check_writable(&file.join("sub")).is_err());
        #[cfg(windows)]
        assert!(windows_version().unwrap().contains("build"));
    }
}
