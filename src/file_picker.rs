//! Picking a file or a folder with the Windows Open window.
//!
//! GPUI's Open window gives Windows no starting folder, so Windows opens the
//! last folder it remembers for the program, or the default library. When
//! that place is gone or broken (a removed USB drive, a disconnected network
//! or OneDrive folder, damaged Quick Access data), `IFileOpenDialog::Show`
//! fails with "Unspecified error (0x80004005)" before anything appears. The
//! Save window never had the problem because it is always given a folder.
//!
//! This picker always starts in a folder that exists, keeps its remembered
//! state under its own id, and if Windows still refuses, tries again with that
//! state cleared, then without an owner window, then with the older Windows
//! file window, which is a separate implementation. Every failed attempt is
//! logged with its error code, and the caller gets all of them in one message.
//! No file contents or names chosen are logged.

use std::path::PathBuf;
use std::sync::mpsc;

/// What to pick.
#[derive(Clone, Debug)]
pub enum Pick {
    /// One existing file, of the given type(s) or any.
    File { type_name: String, patterns: Vec<&'static str> },
    /// One existing folder.
    Folder,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub title: String,
    pub ok_label: String,
    pub pick: Pick,
    /// Folders to start in, best first; the first that exists is used.
    pub start: Vec<PathBuf>,
}

/// Picture files the app can read.
pub const PICTURES: [&str; 7] = ["*.png", "*.jpg", "*.jpeg", "*.gif", "*.bmp", "*.webp", "*.jfif"];

impl Request {
    pub fn picture(title: &str, ok_label: &str) -> Self {
        Self {
            title: title.into(),
            ok_label: ok_label.into(),
            pick: Pick::File { type_name: crate::i18n::t("Pictures", "Imágenes").into(), patterns: PICTURES.to_vec() },
            start: [dirs::picture_dir(), dirs::download_dir(), dirs::document_dir(), dirs::home_dir()].into_iter().flatten().collect(),
        }
    }

    /// The first start folder that exists, or the user's home folder.
    fn start_dir(&self) -> Option<PathBuf> {
        self.start.iter().find(|d| d.is_dir()).cloned().or_else(dirs::home_dir)
    }
}

/// Every attempt failed; the text lists each one with its error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickError(pub String);

impl std::fmt::Display for PickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `Ok(None)`: the user cancelled.
pub type PickResult = Result<Option<PathBuf>, PickError>;

/// Opens the window on its own thread (the Open window needs a COM
/// single-threaded apartment and runs its own message loop) and returns at
/// once; the answer arrives on the receiver. `owner` is the app window's
/// handle, which the Open window stays in front of.
pub fn pick(owner: Option<isize>, request: Request) -> mpsc::Receiver<PickResult> {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("open-window".into()).spawn({
        let tx = tx.clone();
        move || {
            let _ = tx.send(imp::pick_blocking(owner, &request));
        }
    });
    if let Err(e) = spawned {
        let _ = tx.send(Err(PickError(format!("could not start the file window: {e}"))));
    }
    rx
}

/// Waits for the answer from `pick` without blocking the app.
pub async fn wait(rx: mpsc::Receiver<PickResult>, executor: gpui_kit::BackgroundExecutor) -> PickResult {
    loop {
        match rx.try_recv() {
            Ok(r) => return r,
            Err(mpsc::TryRecvError::Empty) => executor.timer(std::time::Duration::from_millis(40)).await,
            Err(mpsc::TryRecvError::Disconnected) => return Err(PickError("the file window stopped unexpectedly".into())),
        }
    }
}

/// The window handle of a GPUI window, for `pick`.
pub fn owner_of(window: &gpui_kit::Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

#[cfg(windows)]
pub(crate) mod imp {
    use std::path::{Path, PathBuf};

    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
    };
    use windows::Win32::UI::Controls::Dialogs::{
        CommDlgExtendedError, GetOpenFileNameW, OFN_ENABLEHOOK, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_NOCHANGEDIR,
        OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST,
        FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem, SHBrowseForFolderW, SHCreateItemFromParsingName,
        SHGetPathFromIDListW, SIGDN_FILESYSPATH,
    };
    use windows::core::{GUID, HSTRING, PCWSTR, PWSTR};

    use super::{Pick, PickError, PickResult, Request};

    /// The Open window remembers its last folder per id; ours is separate
    /// from any state Windows keeps for the program as a whole.
    const CLIENT_ID: GUID = GUID::from_u128(0x6f1c2a4e_9b0d_4f57_a3c1_5e2d8b7a9c41);

    /// `HRESULT_FROM_WIN32(ERROR_CANCELLED)`: the user closed the window.
    const CANCELLED: i32 = 0x800704C7_u32 as i32;

    struct Apartment;
    impl Apartment {
        fn new() -> Result<Self, String> {
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok().map_err(|e| format!("COM: {e}"))?;
            Ok(Self)
        }
    }
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    /// How an attempt sets up the modern Open window.
    #[derive(Clone, Copy)]
    struct Attempt {
        name: &'static str,
        /// Forget what this window remembered (last folder, size) first.
        clear_state: bool,
        /// Show it in front of the app window (otherwise on its own).
        owned: bool,
        /// Start in the requested folder even if Windows remembers another.
        force_folder: bool,
    }

    const ATTEMPTS: [Attempt; 3] = [
        Attempt { name: "Open window", clear_state: false, owned: true, force_folder: false },
        Attempt { name: "Open window, remembered folder cleared", clear_state: true, owned: true, force_folder: true },
        Attempt { name: "Open window without owner", clear_state: true, owned: false, force_folder: true },
    ];

    pub fn pick_blocking(owner: Option<isize>, request: &Request) -> PickResult {
        let _com = Apartment::new().map_err(PickError)?;
        let owner = owner.map(|h| HWND(h as *mut _));
        let mut failures = Vec::new();
        for attempt in ATTEMPTS {
            match modern(owner.filter(|_| attempt.owned), request, attempt) {
                Ok(r) => {
                    if !failures.is_empty() {
                        log::info!("file window opened on attempt '{}'", attempt.name);
                    }
                    return Ok(r);
                }
                Err(e) => {
                    log::warn!("file window attempt '{}' failed: {e}", attempt.name);
                    failures.push(format!("{}: {e}", attempt.name));
                }
            }
        }
        let classic = match &request.pick {
            Pick::File { .. } => classic_file(owner, request),
            Pick::Folder => classic_folder(owner, request),
        };
        match classic {
            Ok(r) => {
                log::info!("file window opened with the classic window");
                Ok(r)
            }
            Err(e) => {
                log::error!("classic file window failed: {e}");
                failures.push(format!("classic window: {e}"));
                Err(PickError(failures.join("; ")))
            }
        }
    }

    fn shell_item(dir: &Path) -> Result<IShellItem, String> {
        unsafe { SHCreateItemFromParsingName(&HSTRING::from(dir.as_os_str()), None) }.map_err(|e| format!("folder {e}"))
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// The Vista-and-later Open window (`IFileOpenDialog`).
    fn modern(owner: Option<HWND>, request: &Request, attempt: Attempt) -> Result<Option<PathBuf>, String> {
        fn step(what: &'static str) -> impl Fn(windows::core::Error) -> String {
            move |e| format!("{what}: {e}")
        }
        unsafe {
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).map_err(step("create"))?;
            dialog.SetClientGuid(&CLIENT_ID).map_err(step("id"))?;
            if attempt.clear_state {
                // Nothing to clear the first time is not an error.
                let _ = dialog.ClearClientData();
            }
            let mut options = dialog.GetOptions().map_err(step("options"))? | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR;
            // Kept alive until Show returns: the window reads them then.
            let mut specs_text = Vec::new();
            match &request.pick {
                Pick::Folder => options |= FOS_PICKFOLDERS,
                Pick::File { type_name, patterns } => {
                    options |= FOS_FILEMUSTEXIST;
                    specs_text.push((wide(type_name), wide(&patterns.join(";"))));
                    specs_text.push((wide(crate::i18n::t("All files", "Todos los archivos")), wide("*.*")));
                }
            }
            dialog.SetOptions(options).map_err(step("options"))?;
            if !specs_text.is_empty() {
                let specs: Vec<COMDLG_FILTERSPEC> = specs_text
                    .iter()
                    .map(|(n, s)| COMDLG_FILTERSPEC { pszName: PCWSTR(n.as_ptr()), pszSpec: PCWSTR(s.as_ptr()) })
                    .collect();
                dialog.SetFileTypes(&specs).map_err(step("file types"))?;
            }
            if let Some(dir) = request.start_dir() {
                // A folder that can't be used is not worth failing over.
                if let Ok(item) = shell_item(&dir) {
                    let r = if attempt.force_folder { dialog.SetFolder(&item) } else { dialog.SetDefaultFolder(&item) };
                    if let Err(e) = r {
                        log::warn!("file window start folder not set: {e}");
                    }
                }
            }
            if !request.title.is_empty() {
                let _ = dialog.SetTitle(&HSTRING::from(request.title.as_str()));
            }
            if !request.ok_label.is_empty() {
                let _ = dialog.SetOkButtonLabel(&HSTRING::from(request.ok_label.as_str()));
            }
            if let Err(e) = dialog.Show(owner) {
                if e.code().0 == CANCELLED {
                    return Ok(None);
                }
                return Err(format!("show: {e}"));
            }
            let item = dialog.GetResult().map_err(step("result"))?;
            let name = item.GetDisplayName(SIGDN_FILESYSPATH).map_err(step("path"))?;
            let path = name.to_string().map_err(|e| format!("path: {e}"));
            CoTaskMemFree(Some(name.0 as *const _));
            Ok(Some(PathBuf::from(path?)))
        }
    }

    /// Forces the older Explorer-style window: a hook procedure makes Windows
    /// use its previous implementation instead of `IFileOpenDialog`.
    unsafe extern "system" fn classic_hook(_: HWND, _: u32, _: WPARAM, _: LPARAM) -> usize {
        0
    }

    fn classic_file(owner: Option<HWND>, request: &Request) -> Result<Option<PathBuf>, String> {
        let Pick::File { type_name, patterns } = &request.pick else { unreachable!() };
        let filter: Vec<u16> = [type_name.as_str(), &patterns.join(";"), crate::i18n::t("All files", "Todos los archivos"), "*.*"]
            .iter()
            .flat_map(|s| s.encode_utf16().chain(Some(0)))
            .chain(Some(0))
            .collect();
        let start: Option<Vec<u16>> = request.start_dir().map(|d| d.as_os_str().encode_wide_z());
        let title = wide(&request.title);
        let mut buffer = vec![0u16; 4096];
        let mut ofn = OPENFILENAMEW {
            lStructSize: size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: owner.unwrap_or_default(),
            lpstrFilter: PCWSTR(filter.as_ptr()),
            nFilterIndex: 1,
            lpstrFile: PWSTR(buffer.as_mut_ptr()),
            nMaxFile: buffer.len() as u32,
            lpstrInitialDir: start.as_ref().map_or(PCWSTR::null(), |s| PCWSTR(s.as_ptr())),
            lpstrTitle: if request.title.is_empty() { PCWSTR::null() } else { PCWSTR(title.as_ptr()) },
            Flags: OFN_EXPLORER | OFN_ENABLEHOOK | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR,
            lpfnHook: Some(classic_hook),
            ..Default::default()
        };
        if unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() {
            let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
            return Ok(Some(PathBuf::from(String::from_utf16_lossy(&buffer[..len]))));
        }
        match unsafe { CommDlgExtendedError() }.0 {
            0 => Ok(None),
            code => Err(format!("error 0x{code:04X}")),
        }
    }

    fn classic_folder(owner: Option<HWND>, request: &Request) -> Result<Option<PathBuf>, String> {
        let title = wide(&request.title);
        let info = BROWSEINFOW {
            hwndOwner: owner.unwrap_or_default(),
            lpszTitle: PCWSTR(title.as_ptr()),
            ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
            ..Default::default()
        };
        let list = unsafe { SHBrowseForFolderW(&info) };
        if list.is_null() {
            // The folder window has no error report; nothing chosen.
            return Ok(None);
        }
        let mut buffer = [0u16; 260];
        let ok = unsafe { SHGetPathFromIDListW(list, &mut buffer) }.as_bool();
        unsafe { CoTaskMemFree(Some(list as *const _)) };
        if !ok {
            return Err("the chosen place is not a folder on disk".into());
        }
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Ok(Some(PathBuf::from(String::from_utf16_lossy(&buffer[..len]))))
    }

    trait WideZ {
        fn encode_wide_z(&self) -> Vec<u16>;
    }
    impl WideZ for std::ffi::OsStr {
        fn encode_wide_z(&self) -> Vec<u16> {
            use std::os::windows::ffi::OsStrExt as _;
            self.encode_wide().chain(Some(0)).collect()
        }
    }

    /// Test support: shows each kind of window on this thread and closes it
    /// from a timer as soon as it is on screen. `Ok(None)` proves it opened.
    #[cfg(test)]
    pub fn open_and_close(request: &Request, which: usize) -> PickResult {
        use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};
        let _com = Apartment::new().map_err(PickError)?;
        let timer = unsafe { SetTimer(None, 0, 400, Some(close_open_windows)) };
        let r = match which {
            0..=2 => modern(None, request, ATTEMPTS[which]).map_err(PickError),
            _ => match &request.pick {
                Pick::File { .. } => classic_file(None, request),
                Pick::Folder => classic_folder(None, request),
            }
            .map_err(PickError),
        };
        unsafe {
            let _ = KillTimer(None, timer);
        }
        r
    }

    /// Closes this thread's dialog windows, as the Cancel button would.
    #[cfg(test)]
    unsafe extern "system" fn close_open_windows(_: HWND, _: u32, _: usize, _: u32) {
        use windows::Win32::Foundation::TRUE;
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetClassNameW, PostMessageW, WM_CLOSE};
        unsafe extern "system" fn close(hwnd: HWND, _: LPARAM) -> windows::core::BOOL {
            let mut class = [0u16; 16];
            let n = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
            if String::from_utf16_lossy(&class[..n]) == "#32770" {
                let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            }
            TRUE
        }
        let _ = unsafe { EnumThreadWindows(GetCurrentThreadId(), Some(close), LPARAM(0)) };
    }
}

#[cfg(not(windows))]
pub(crate) mod imp {
    use super::{PickError, PickResult, Request};
    pub fn pick_blocking(_: Option<isize>, _: &Request) -> PickResult {
        Err(PickError("file windows are only available on Windows".into()))
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn picture() -> Request {
        Request::picture("Test", "Use")
    }

    fn folder() -> Request {
        Request { title: "Test".into(), ok_label: "Use".into(), pick: Pick::Folder, start: vec![std::env::temp_dir()] }
    }

    #[test]
    fn starts_in_a_folder_that_exists() {
        let r = Request { start: vec![PathBuf::from(r"Z:\does\not\exist"), std::env::temp_dir()], ..picture() };
        assert_eq!(r.start_dir(), Some(std::env::temp_dir()));
        let none = Request { start: vec![PathBuf::from(r"Z:\does\not\exist")], ..picture() };
        assert_eq!(none.start_dir(), dirs::home_dir());
    }

    /// Opens every kind of Open window this picker can use and closes it from
    /// a timer. It puts windows on the screen for a moment, so it only runs
    /// on request: `cargo test -- --ignored file_windows_open`.
    #[test]
    #[ignore = "shows real windows on the desktop"]
    fn file_windows_open() {
        for which in 0..4 {
            assert_eq!(imp::open_and_close(&picture(), which), Ok(None), "picture window {which}");
            assert_eq!(imp::open_and_close(&folder(), which), Ok(None), "folder window {which}");
        }
    }
}
