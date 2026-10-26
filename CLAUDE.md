# Acervus Finance: working rules

Rust + GPUI (via `gpui-kit`) desktop app for Windows, SQLite storage. The
visual system is written down in `DESIGN.md`; read it before any UI work.

## UI components: build, test in Overview, then ask

1. New or changed reusable components go in `src/ui/kit/` (one component per
   file, re-exported from `kit/mod.rs`).
2. Every new component or change is tried out on the **Overview** screen
   (`src/ui/screens/overview.rs`) first.
3. When it works there, **stop and ask the user** whether to roll it out.
   Do not switch other screens, forms or dialogs to it until the user says
   yes. Ask again for each new component or change; one approval doesn't
   cover the next one.
4. After approval, migrate every screen to the component and delete the old
   one-off code it replaces.

Status (2026-10-26): the user approved the kit and every screen, the
sidebar and every form now use it. The old `card`, `card_header`,
`figure` and `segmented` helpers are gone, and no gpui-component `Button` is
left. Still on gpui-component on purpose: the form dialogs' engine, `Input`,
`Select`, `Switch`, `Checkbox` (the kit has no text field or switch), and the
date picker's hand-built toggle.

Kit contents: `Button` (Primary, Secondary, Ghost, Danger, DangerGhost;
Mini/Small/Medium; `.loading`, `.selected`, `.suffix`), `Card`, `Stat`,
`StatStrip`, `ListRow` (`.muted`), `Segmented` (`Segmented::pick`, cells
fit their labels), `Dropdown` + `DropdownItem` (`.danger`, `.disabled`,
`.hint`), `Overlay`, and `kit::interact` (hover state, color mixing) for
custom elements that need the same springs.

## Principles

- **SOLID**: a component does one job and is extended through builder
  methods (`.suffix()`, `.action()`), not by editing it for each caller.
  Screens depend on `ui::kit` and `ui::theme`, never on raw values.
- **DRY**: if a pattern appears a third time, it becomes a kit component or
  a token. No copy-pasted rows, cards or buttons.
- **KISS**: the smallest API that covers real uses. No speculative options.

## Styling rules

- Colors come from `Palette` (`theme::palette(cx)`). No hex values in screens.
- Type comes from `theme::text::*` roles via `.typeset(...)`. No raw
  `text_size` + `font_weight` pairs in new code.
- Depth comes from `theme::Elevation` (`Flat`, `Raised`, `Lifted`, `Floating`).
- Motion comes from `theme::motion` springs (`CONTROL`, `GLIDE`, `RISE`,
  `FADE`). No linear or fixed-duration transitions on controls.
- Numbers: tabular figures (`theme::tabular()`) always; amounts in lists,
  tables and columns are right-aligned. Money is never truncated.
- Layout: branch on `theme::Width` (`Compact`/`Medium`/`Wide`/`Ultra`) from
  `VzApp::content_width`. The content column uses the full window width.
- Text is bilingual: every string goes through `t(...)` / `tr!(...)`.

## Checks before handing back

- `cargo fmt`, `cargo clippy --all-targets`, `cargo test`.
- The user keeps the app open from `target/debug` or `target/release`.
  Windows locks a running exe, so close it, rebuild, and relaunch it after
  UI changes.
- Receipts (`src/services/receipt.rs`) are US Letter pages. `PAYMENT_PAGE_ROWS`
  and `LOAN_PAGE_ROWS` were measured by printing sample receipts to PDF with
  headless Edge (`msedge --headless=new --no-pdf-header-footer
  --print-to-pdf=<out.pdf> file:///<receipt.html>`) from a scratch database.
  After changing receipt CSS, measure again so the "continues on the next
  page" note matches the real page count.
- Form tests run in a GPUI test window (`#[gpui::test]`, dev-dependency
  `gpui-pre` with `test-support`). In a test module that does `use super::*`
  over the kit's glob import, add `use core::prelude::v1::test;` and
  `use ::gpui;`, or the macro recurses or is ambiguous. Never let a test print
  a receipt: an in-memory store writes files next to the working directory.
- For screenshots use a demo database, never the user's data:
  `cargo run --example seed_demo -- <new.db>` and launch with
  `VZ_FINANCE_DB=<new.db>`. Capture the window with `PrintWindow`, which works
  even when it is covered. Never send synthetic keystrokes or clicks: when the
  app isn't the foreground window they land in the user's other programs.
