# VZ Finance

A private, offline Windows desktop app for running a personal lending
portfolio: clients, loans, interest, repayments and the growth of your capital.
Built in Rust with [GPUI](https://gpui.rs) and SQLite.

![Overview](docs/screenshots/overview.png)

## What it does

- **Overview**: portfolio value, outstanding principal, interest earned,
  available capital, active loans and overdue amount, with a portfolio growth
  chart, where your money is, recent payments and loans needing attention.
- **Clients**: searchable directory; each client has a profile with loans, a
  payment timeline, totals and notes. Clients are archived, never deleted.
- **Loans**: simple interest, amortized and interest-only loans with a live
  repayment preview before you create them; detail pages show the schedule
  with paid, partly paid and overdue installments.
- **Payments**: record a payment in a few keystrokes with a live split between
  interest (income) and principal (capital returned). Mistakes are corrected
  with audited reversals.
- **Portfolio Analytics**: returns, capital ledger, monthly interest, capital
  deployment, loan status breakdown and a monthly cash-flow table.
- **Settings**: currency, number and date formats, defaults, light and dark
  theme, verified backup and restore, CSV export.

Every figure is derived from recorded transactions. The rules (day count,
rounding, allocation order, overdue logic, portfolio value) are written down in
[docs/ACCOUNTING.md](docs/ACCOUNTING.md).

![Loan detail](docs/screenshots/loan.png)

## Privacy

No account, no network requests, no telemetry. Data lives in
`%LOCALAPPDATA%\VZFinance\data\portfolio.db`. Installing, upgrading or
uninstalling never touches that folder. The database is not encrypted by the
app; use BitLocker or device encryption for encryption at rest.

## Keyboard

| Action | Keys |
| --- | --- |
| Record payment | Ctrl P |
| New loan | Ctrl L |
| New client | Ctrl N |
| Search clients | Ctrl F |
| Back up | Ctrl S |
| Toggle sidebar | Ctrl B |
| Sections | Ctrl 1 – 6 |
| Back | Alt ← |
| Confirm a dialog | Enter |

## Building

Requirements: Windows 10/11 x64, Rust stable (1.95+), and the MSVC C++ build
tools with a Windows 10/11 SDK (Visual Studio Build Tools, "Desktop development
with C++").

```powershell
cargo run                    # debug build
cargo test                   # calculation engine, database and service tests
powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1
```

The installer script runs the tests, builds the release binary (static C
runtime, no console window) and produces `dist\VZFinance-Setup-<version>.exe`
with [Inno Setup 6](https://jrsoftware.org/isinfo.php)
(`winget install JRSoftware.InnoSetup`).

**Without Visual Studio.** If the Build Tools can't be installed (some
antivirus products block it), the app also builds with LLVM (`clang-cl`,
`lld-link`) and the Microsoft CRT and SDK libraries downloaded by
[xwin](https://github.com/Jake-Shadle/xwin). Point Cargo at them in your
user-level `~/.cargo/config.toml` (`linker`, `CC`, `AR`, `INCLUDE`, `LIB`,
`RC`). Release builds also need `fxc.exe` for GPUI's shaders: set
`GPUI_FXC_PATH` to it, or to any tool that accepts the same arguments.

### Demo data

```powershell
cargo run --example seed_demo -- C:\temp\demo.db
$env:VZ_FINANCE_DB = "C:\temp\demo.db"; cargo run
```

`VZ_FINANCE_DB` points the app at another database file, so your real data is
never touched while testing. The seeder refuses to overwrite an existing file.

## Project layout

```
src/
  main.rs          entry point: logging, assets, window
  lib.rs
  domain/          pure financial logic (money, schedules, accrual, portfolio)
  database/        SQLite setup, migrations, repositories, backup/restore
  services/        validated, transactional operations; settings; CSV export
  app/             root view, navigation, shortcuts, sidebar, dialogs
  ui/              design tokens, components, forms, screens
  charts/          line and column charts drawn with GPUI primitives
  utils/           dates
installer/         Inno Setup script
docs/              accounting rules
examples/          demo data seeder
```

## Licenses

Inter typeface: SIL Open Font License 1.1 (`assets/fonts/Inter-LICENSE.txt`).
Icons: Lucide (ISC), bundled through GPUI Kit.
