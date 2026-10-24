# VZ Finance

A private, offline Windows desktop app for running a personal lending
portfolio: clients, loans, interest, repayments and the growth of your capital.
Built in Rust with [GPUI](https://gpui.rs) and SQLite.

![Overview](docs/screenshots/overview.png)

## What it does

- **Loans at a rate per period**: lend at 5%, 10%, 15% or any rate, paid
  monthly or every 15 days. Each period's interest is the rate times the
  capital still owed; paying more than the interest lowers the capital and so
  the next interest (1,000 at 10% is 100; paying 600 leaves 500, so next time
  it is 50).
- **Overview**: portfolio value, capital lent out, interest earned, available
  capital, active loans and overdue amount, with a growth chart, where your
  money is, recent payments and loans needing attention.
- **Clients**: searchable directory; each client has a profile with loans,
  payment history, totals and notes. Clients are archived, never deleted.
- **Payments**: record a payment in a few keystrokes with a live split between
  interest and capital.
- **Two accounts**: available capital and available interest. Loans use
  capital first, then interest; repaid capital goes back to capital, interest
  to interest. Record expenses from either account, move money between them,
  or adjust both to what you really have on hand.
- **Everything is editable**: loans, payments, capital, expenses and clients
  can be edited or deleted. Payment splits are recalculated automatically and
  every change is kept in a change history with the old and new values.
- **Printable receipts**: a payment receipt (what was paid, what is still
  owed, the percentage of capital paid, the next payment) and a loan receipt
  (amount, rate, payment dates, how payments work, signature lines), both with
  your logo.
- **Portfolio Analytics**: returns, capital ledger, monthly interest, capital
  lent out, loans by status and a monthly cash-flow table.
- **Settings**: business name and logo (shown in the app and on receipts), English or
  Spanish, currency (Dominican peso by default), number and date formats,
  default rate and payment frequency, light and dark theme, verified backup
  and restore, CSV export.

Every figure is derived from recorded transactions. The rules (interest,
rounding, payment order, overdue logic, portfolio value) are written down in
[docs/ACCOUNTING.md](docs/ACCOUNTING.md).

![Loan detail](docs/screenshots/loan.png)

![Receipt](docs/screenshots/receipt.png)

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
| New expense | Ctrl E |
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

### Releases

Every push to `main` runs [`.github/workflows/release.yml`](.github/workflows/release.yml),
which formats, audits dependencies (RustSec), lints with Clippy, tests, builds the
installer on Windows and publishes it as a GitHub Release with its SHA-256. A
newer push to `main` cancels the run in progress, so only the latest commit is
released. Commits that only change `README.md` or screenshots, or whose message
contains `[skip ci]`, don't release.

Versions: `Cargo.toml` sets `MAJOR.MINOR`; CI picks the next free patch from the
`v*` tags (0.2.0, 0.2.1, …) and stamps it into the build. Nothing is committed
back. To start a new line, raise the version in `Cargo.toml` (for example to
`0.3.0`).

Code signing is optional: add the repository secrets `WINDOWS_SIGNING_CERT`
(the `.pfx` file, base64-encoded) and `WINDOWS_SIGNING_PASSWORD`, and both the
app and the installer are signed and timestamped.

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
