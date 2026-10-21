// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]


use std::borrow::Cow;
use std::io::Write as _;
use std::sync::Mutex;

use gpui_kit::*;

use vz_finance::app::{self, VzApp};
use vz_finance::services::Store;
use vz_finance::{database, ui};

gpui_kit::assets::icon_assets!(
    AppIcons,
    [
        LayoutDashboard, Users, HandCoins, ArrowLeftRight, ChartLine, Settings, DatabaseBackup, PanelLeftClose, PanelLeftOpen, ShieldCheck,
        Plus, Search, SearchX, Wallet, TrendingUp, Landmark, FileText, CircleAlert, CircleCheck, Check, Receipt, Calculator, UserPlus,
        Pencil, Archive, ArchiveRestore, ArrowLeft, FolderOpen, FileSpreadsheet, ChartColumn, Table
    ]
);

/// Application icons plus the default component bundle.
struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(found) = AppIcons.load(path)? {
            return Ok(Some(found));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all = AppIcons.list(path)?;
        all.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(all)
    }
}

/// Minimal file logger. Logs operational events only; names, amounts and
/// notes are never written.
struct FileLogger(Mutex<std::fs::File>);

impl log::Log for FileLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info && !m.target().starts_with("naga") && !m.target().starts_with("wgpu")
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata())
            && let Ok(mut f) = self.0.lock()
        {
            let _ = writeln!(f, "{} {:<5} {}: {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), r.level(), r.target(), r.args());
        }
    }
    fn flush(&self) {}
}

fn init_logging() {
    if cfg!(debug_assertions) {
        let _ = env_logger::try_init();
        return;
    }
    let Some(dir) = dirs::data_local_dir().map(|d| d.join("VZFinance").join("logs")) else { return };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("vz-finance.log");
    // Keep the log small: start fresh once it passes 1 MB.
    if std::fs::metadata(&path).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("vz-finance.old.log"));
    }
    if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let logger: &'static FileLogger = Box::leak(Box::new(FileLogger(Mutex::new(file))));
        if log::set_logger(logger).is_ok() {
            log::set_max_level(log::LevelFilter::Info);
        }
    }
}

/// Shown instead of the app when the database cannot be opened.
struct StartupError {
    message: String,
    path: String,
}

impl Render for StartupError {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = ui::theme::palette(cx);
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(p.ground)
            .font_family(ui::theme::FONT)
            .child(
                ui::components::card(&p)
                    .w(px(560.))
                    .p(px(32.))
                    .gap(px(12.))
                    .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).text_color(p.text).child("VZ Finance could not open your data"))
                    .child(div().text_color(p.muted).child(self.message.clone()))
                    .child(div().text_size(px(12.)).text_color(p.faint).child(format!("Database: {}", self.path)))
                    .child(div().text_color(p.muted).child(
                        "Nothing was changed. If you have a backup, you can copy it over this file after closing the app.",
                    )),
            )
    }
}

fn window_options(cx: &mut App) -> WindowOptions {
    let mut size_wanted = size(px(1440.), px(920.));
    if let Some(display) = cx.primary_display() {
        let avail = display.bounds().size;
        size_wanted.width = size_wanted.width.min(avail.width * 0.92);
        size_wanted.height = size_wanted.height.min(avail.height * 0.9);
    }
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size_wanted, cx))),
        titlebar: Some(TitlebarOptions { title: Some("VZ Finance".into()), ..Default::default() }),
        window_min_size: Some(size(px(1080.), px(680.))),
        app_id: Some("VZFinance".into()),
        ..Default::default()
    }
}

fn main() {
    init_logging();
    log::info!("starting VZ Finance {}", env!("CARGO_PKG_VERSION"));
    let path = database::default_db_path();
    let store = path.as_ref().map_err(|e| format!("{e:#}")).and_then(|p| Store::open(p.clone()).map_err(|e| format!("{e:#}")));
    let theme_pref = store.as_ref().ok().and_then(|s| s.snapshot().ok()).map(|s| s.settings.theme).unwrap_or_default();
    let path_text = path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();

    gpui_kit::application().with_assets(AppAssets).run(move |cx| {
        gpui_kit::init(cx);
        ui::theme::install(theme_pref, cx);
        app::bind_keys(cx);
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let options = window_options(cx);
        match store {
            Ok(store) => {
                let opened = gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| VzApp::new(store, window, cx)));
                if let Err(e) = opened {
                    log::error!("could not open window: {e:#}");
                    cx.quit();
                }
            }
            Err(message) => {
                log::error!("database open failed: {message}");
                let _ = gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| StartupError { message, path: path_text }));
            }
        }
        cx.activate(true);
    });
}
