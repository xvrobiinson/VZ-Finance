//! Opening a file or folder, or showing a file in Explorer, the way the
//! Windows shell expects: on a thread with COM set up, with the result
//! reported back instead of only logged.
//!
//! GPUI's `open_with_system` and `reveal_path` run on a worker thread without
//! COM and only log a failure, so on a PC where, say, no program opens
//! `.html` files, the Receipt or Open folder button seems to do nothing. Here
//! a failure falls back to Explorer (folders, Show in Explorer) or Microsoft
//! Edge (receipts, which are web pages), and if that fails too the caller gets
//! the reason to show.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Opens `path` with its program, or a folder in Explorer.
pub fn open(path: &Path) -> mpsc::Receiver<Result<(), String>> {
    run(path.to_path_buf(), imp::open)
}

/// Opens Explorer on `path`'s folder with `path` selected.
pub fn reveal(path: &Path) -> mpsc::Receiver<Result<(), String>> {
    run(path.to_path_buf(), imp::reveal)
}

/// Opens a web address in the person's browser. Only `https://` addresses,
/// so nothing else can be started through here.
pub fn browse(url: &str) -> mpsc::Receiver<Result<(), String>> {
    let (tx, rx) = mpsc::channel();
    if !url.starts_with("https://") || url.chars().any(|c| c.is_whitespace() || c == '"') {
        let _ = tx.send(Err("not a web address".into()));
        return rx;
    }
    let url = url.to_string();
    let spawned = std::thread::Builder::new().name("shell".into()).spawn({
        let tx = tx.clone();
        move || {
            let _ = tx.send(imp::browse(&url));
        }
    });
    if let Err(e) = spawned {
        let _ = tx.send(Err(e.to_string()));
    }
    rx
}

fn run(path: PathBuf, f: fn(&Path) -> Result<(), String>) -> mpsc::Receiver<Result<(), String>> {
    let (tx, rx) = mpsc::channel();
    if !path.exists() {
        let _ = tx.send(Err(crate::i18n::t("it no longer exists", "ya no existe").into()));
        return rx;
    }
    let spawned = std::thread::Builder::new().name("shell".into()).spawn({
        let tx = tx.clone();
        move || {
            let _ = tx.send(f(&path));
        }
    });
    if let Err(e) = spawned {
        let _ = tx.send(Err(e.to_string()));
    }
    rx
}

/// Waits for `open` or `reveal` without blocking the app.
pub async fn wait(rx: mpsc::Receiver<Result<(), String>>, executor: gpui_kit::BackgroundExecutor) -> Result<(), String> {
    loop {
        match rx.try_recv() {
            Ok(r) => return r,
            Err(mpsc::TryRecvError::Empty) => executor.timer(std::time::Duration::from_millis(40)).await,
            Err(mpsc::TryRecvError::Disconnected) => return Err("stopped unexpectedly".into()),
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::path::Path;

    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize};
    use windows::Win32::UI::Shell::{
        ILCreateFromPathW, ILFree, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, SHOpenFolderAndSelectItems, ShellExecuteExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};

    struct Apartment;
    impl Apartment {
        fn new() -> Self {
            // Already set up (or set up differently) is fine for the shell.
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
            Self
        }
    }
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    /// `ShellExecuteEx` with `file` and optional `params`; Windows shows no
    /// error box of its own, the error comes back instead.
    fn execute(file: &HSTRING, params: Option<&HSTRING>) -> Result<(), String> {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpVerb: w!("open"),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: params.map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr())),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut info) }.map_err(|e| e.message())
    }

    /// Quotes a path for a command line.
    fn quoted(path: &Path) -> HSTRING {
        HSTRING::from(format!("\"{}\"", path.display()))
    }

    pub fn open(path: &Path) -> Result<(), String> {
        let _com = Apartment::new();
        let first = match execute(&HSTRING::from(path.as_os_str()), None) {
            Ok(()) => return Ok(()),
            Err(e) => e,
        };
        log::warn!("opening a {} failed: {first}", kind(path));
        let html = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("html"));
        let fallback = if path.is_dir() {
            execute(&HSTRING::from("explorer.exe"), Some(&quoted(path)))
        } else if html {
            // Edge comes with Windows 10 and 11 and registers itself by name.
            execute(&HSTRING::from("msedge.exe"), Some(&quoted(path)))
        } else {
            Err(first.clone())
        };
        fallback.map_err(|second| {
            log::error!("fallback for the {} failed too: {second}", kind(path));
            first
        })
    }

    pub fn reveal(path: &Path) -> Result<(), String> {
        let _com = Apartment::new();
        let pidl = unsafe { ILCreateFromPathW(&HSTRING::from(path.as_os_str())) };
        let first = if pidl.is_null() {
            "the path could not be found".to_string()
        } else {
            let r = unsafe { SHOpenFolderAndSelectItems(pidl, None, 0) };
            unsafe { ILFree(Some(pidl)) };
            match r {
                Ok(()) => return Ok(()),
                Err(e) => e.message(),
            }
        };
        log::warn!("showing a {} in Explorer failed: {first}", kind(path));
        let select = HSTRING::from(format!("/select,\"{}\"", path.display()));
        execute(&HSTRING::from("explorer.exe"), Some(&select)).map_err(|second| {
            log::error!("Explorer fallback failed too: {second}");
            first
        })
    }

    pub fn browse(url: &str) -> Result<(), String> {
        let _com = Apartment::new();
        execute(&HSTRING::from(url), None)
    }

    /// What is being opened, for the log: never the path, which can hold a
    /// client's name.
    fn kind(path: &Path) -> String {
        if path.is_dir() {
            "folder".into()
        } else {
            path.extension().map(|e| format!(".{} file", e.to_string_lossy())).unwrap_or_else(|| "file".into())
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;
    pub fn open(_: &Path) -> Result<(), String> {
        Err("only available on Windows".into())
    }
    pub fn reveal(_: &Path) -> Result<(), String> {
        Err("only available on Windows".into())
    }
    pub fn browse(_: &str) -> Result<(), String> {
        Err("only available on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_path_is_an_error_not_a_stray_window() {
        let missing = std::env::temp_dir().join("acervus-does-not-exist").join("receipt.html");
        assert!(open(&missing).recv().unwrap().is_err());
        assert!(reveal(&missing).recv().unwrap().is_err());
    }

    #[test]
    fn only_web_addresses_open_in_the_browser() {
        assert!(browse("C:\\Windows\\System32\\calc.exe").recv().unwrap().is_err());
        assert!(browse("http://example.com").recv().unwrap().is_err());
        assert!(browse("https://example.com\" --evil").recv().unwrap().is_err());
    }
}
