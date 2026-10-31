---
name: Acervus Finance
description: A private, offline lending book for Windows. Calm, exact, discreet.
colors:
  ground: "#F6F7FB"
  surface: "#FFFFFF"
  raised: "#FAFAFC"
  hover: "#F3F3F8"
  active: "#EEEDF6"
  border: "#E6E7EE"
  rule: "#EEEFF4"
  neutral-tint: "#F1F2F6"
  control: "#DCDDE6"
  ink: "#17181C"
  muted: "#5E6270"
  faint: "#6B6F7C"
  ledger-violet: "#5B47E8"
  ledger-violet-deep: "#4B37D6"
  ledger-violet-tint: "#EEEBFF"
  ledger-violet-text: "#4B37D6"
  on-accent: "#FFFFFF"
  positive: "#047857"
  positive-tint: "#E3F7EE"
  warning: "#8A5300"
  warning-tint: "#FFF4DC"
  negative: "#B42323"
  negative-tint: "#FDECEC"
  info: "#2459C9"
  info-tint: "#E6F0FF"
  dark-ground: "#16171B"
  dark-sidebar: "#121316"
  dark-surface: "#1B1C21"
  dark-surface-top: "#23242A"
  dark-ink: "#F1F2F5"
  dark-ledger-violet: "#6E5BF6"
typography:
  display:
    fontFamily: "Inter Display, Inter, sans-serif"
    fontSize: "30px"
    fontWeight: 600
    lineHeight: "36px"
    fontFeature: "'tnum' 1, 'cv11' 1"
  title:
    fontFamily: "Inter Display, Inter, sans-serif"
    fontSize: "24px"
    fontWeight: 600
    lineHeight: "30px"
  figure:
    fontFamily: "Inter Display, Inter, sans-serif"
    fontSize: "22px"
    fontWeight: 500
    lineHeight: "28px"
    fontFeature: "'tnum' 1, 'cv11' 1"
  heading:
    fontFamily: "Inter, sans-serif"
    fontSize: "15px"
    fontWeight: 600
    lineHeight: "20px"
  body:
    fontFamily: "Inter, sans-serif"
    fontSize: "13.5px"
    fontWeight: 400
    lineHeight: "20px"
  body-strong:
    fontFamily: "Inter, sans-serif"
    fontSize: "13.5px"
    fontWeight: 500
    lineHeight: "20px"
  label:
    fontFamily: "Inter, sans-serif"
    fontSize: "13px"
    fontWeight: 500
    lineHeight: "18px"
  caption:
    fontFamily: "Inter, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: "16px"
  micro:
    fontFamily: "Inter, sans-serif"
    fontSize: "11px"
    fontWeight: 500
    lineHeight: "14px"
rounded:
  chip: "6px"
  control: "8px"
  overlay: "10px"
  card: "14px"
spacing:
  xs: "4px"
  s: "8px"
  m: "12px"
  l: "16px"
  xl: "24px"
  xxl: "32px"
components:
  button-primary:
    backgroundColor: "{colors.ledger-violet}"
    textColor: "{colors.on-accent}"
    typography: "{typography.label}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "32px"
  button-primary-hover:
    backgroundColor: "{colors.ledger-violet-deep}"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    typography: "{typography.label}"
    rounded: "{rounded.control}"
    padding: "0 12px"
    height: "32px"
  button-secondary-hover:
    backgroundColor: "{colors.hover}"
  button-ghost-hover:
    backgroundColor: "{colors.hover}"
    textColor: "{colors.ink}"
  card:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.card}"
    padding: "0 24px 24px"
  menu:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.overlay}"
    padding: "4px"
  menu-item:
    typography: "{typography.label}"
    rounded: "{rounded.chip}"
    height: "30px"
    padding: "0 8px"
  segmented-track:
    backgroundColor: "{colors.neutral-tint}"
    rounded: "{rounded.control}"
    padding: "2px"
  segmented-thumb:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.chip}"
    height: "26px"
  pill:
    typography: "{typography.caption}"
    rounded: "{rounded.chip}"
    height: "22px"
    padding: "0 8px"
---

# Design System: Acervus Finance

## Overview

**Creative North Star: "The Private Ledger"**

Acervus is a well-kept book of accounts that lives on one PC. Figures lead;
the interface stays out of their way. Every screen answers "where is my money
and who owes me" before it offers anything to do, and every number on it can
be traced back to a recorded transaction. The tone is calm, exact and
discreet. Nothing shouts unless money is late.

The look (redesigned 2026-10-30, the "Dashboard style") is a calm
dashboard: a near-black ground in dark mode with the sign-in's night sky
behind every screen, a soft grey-blue ground in light mode, cards that each
hold one subject, and one violet accent.

Density is that of a working tool: tables at 13.5px, rows at 60px, cards
holding several facts each. Restraint carries the brand. One violet marks
actions and selection, and reds, ambers and greens mark the state of money
and nothing else. Depth is soft and real, so cards feel like sheets lying on a
desk, not stickers. Motion is quick and physical: springs that settle in about
200ms, never a performance.

**Key Characteristics:**
- Numbers first: tabular figures everywhere, amounts right-aligned in any
  list or column, never truncated.
- One accent (Ledger Violet) for action and selection only.
- Raised cards on a cool ground, with three-layer shadows; in dark mode a
  card's face shades from a lighter top down to its surface color.
- Spring motion on hover, press, selection and arrival; nothing loops.
- The whole window is used. The layout reflows across four widths instead of
  centering a fixed column.

## Colors

A cool, near-neutral ground with one confident violet and three status hues
that only ever describe money.

### Primary
- **Ledger Violet** (#5B47E8; #6E5BF6 in dark): the primary button, the
  selected segment and sidebar item, links, focus rings, the portfolio line
  in charts. At most one primary button per view.
- **Ledger Violet Deep** (#4B37D6): the primary button under the pointer.
- **Ledger Violet Tint** (#EEEBFF) and **Ledger Violet Text** (#4B37D6):
  accent pills, icon chips and text links.

### Neutral
- **Cool Ground** (#F7F8FA): the app background behind cards.
- **Sheet White** (#FFFFFF): cards, menus, secondary buttons.
- **Ink** (#17191C): primary text and figures.
- **Muted Slate** (#666C77) / **Faint Slate** (#737985): secondary text and
  icons. Both stay at or above 4.5:1 on white and on Cool Ground.
- **Border** (#E9EBEF), **Rule** (#F0F1F4), **Control Edge** (#DCDFE5):
  card edges, row dividers, the edge of framed buttons.
- **Hover** (#F3F4F7) / **Active** (#EEF0F4): pointer and pressed fills.

### Icon chips
- **Info** (#2459C9 on #E6F0FF) joins the accent and the status tints to
  tell stat cards apart by the icon's square: violet for value and capital,
  blue for what is owed or expected, green for money earned, red only when
  money is late. Info never describes a money state.

### Status
- **Positive** (#137A60 on #E7F5F0): money received, gains, paid off.
- **Warning** (#9A5F0E on #FDF3E3): due soon.
- **Negative** (#C2353D on #FDECEC): overdue, defaulted, losses, destructive
  actions.

Dark theme mirrors every role (ground #16171B, sidebar #121316, surface
#1B1C21 shading up to #23242A, ink #F1F2F5, violet #6E5BF6). The full set
is in `src/ui/theme.rs`. The chart's "capital invested" line is orange
(`chart_reference`).

### Sky
In dark mode the sign-in screen's night sky (gradient, two faint arcs, soft
clouds along the bottom) sits behind every screen's content. Light mode keeps
its plain ground; the owner tried and declined the blue day sky there.

### Named Rules
**The Money-Only Color Rule.** Green, amber and red describe the state of
money. They are never decoration, category colors or brand accents.

**The One Accent Rule.** Ledger Violet marks the thing you can act on or the
thing that is selected. If two violet buttons compete on one screen, one of
them is wrong.

## Typography

**Display Font:** Inter Display (with Inter)
**Body Font:** Inter (with the system sans)

**Character:** One family in two optical cuts. Inter sets everything up to
18px; Inter Display, drawn tighter for large sizes, sets titles and headline
figures. GPUI has no letter-spacing control, so the display cut is how large
type gets its tighter tracking.

### Hierarchy
- **Display** (600, 30/36px, Inter Display): the single figure a screen is
  about (total portfolio value).
- **Title** (600, 24/30px, Inter Display): page titles.
- **Figure** (500, 22/28px, Inter Display): KPI values.
- **Heading** (600, 15/20px): card and section headings.
- **Body** (400, 13.5/20px) and **Body Strong** (500): running text, table
  cells, the leading name or amount of a row.
- **Label** (500, 13/18px): buttons, segments, menu items.
- **Caption** (400, 12/16px): secondary lines, field labels, table headers.
- **Micro** (500, 11/14px): keyboard hints.

Roles are applied with `.typeset(text::ROLE)`, which sets family, size,
line height and weight together.

### Named Rules
**The Tabular Rule.** Every number uses tabular figures (`tnum`, plus `cv11`
for the single-storey a), so columns of money line up digit for digit.

**The Right Edge Rule.** Amounts in lists, tables and side-by-side columns
are right-aligned. Labels stay left. A KPI's figure stays left-aligned under
its own label, because it reads as one unit with it.

## Layout

The content column fills the window beside the sidebar; there is no maximum
width. Screens reflow on `Width`, measured from the content column:

- **Compact** (under 760px): one column, KPI figures two to a row, the chart
  range becomes a dropdown, secondary page actions fold into a "More" menu.
- **Medium** (760–1240px): cards stack, KPIs three to a row.
- **Wide** (1240–1680px): growth and allocation side by side (2 : 1, with the
  allocation card held between 360 and 520px), recent payments beside needs
  attention, KPIs three to a row.
- **Ultra** (1680px and up): all six KPIs in one row, taller chart.

Gutters follow the window: 16px under 900px, 24px under 1400px, 36px above.
Card gaps are 24px (16px when Compact). Below a 1024px window the sidebar
collapses to icons on its own. The window can shrink to 720 × 560.

Spacing runs on 4 and 8: 4, 8, 12, 16, 24, 32. Card content is inset 24px.

## Elevation & Depth

Layered and soft. Every elevation is the same three shadows (contact, key and
ambient) cut from one cool ink, so one level can spring into another.
Dark mode multiplies their strength by four and adds a hairline highlight
along the top edge, since dark grounds swallow shadows.

### Shadow Vocabulary
- **Raised** (`0 1px 2px ink/5%, 0 2px 6px -1px ink/4%, 0 8px 24px -6px ink/7%`):
  cards at rest.
- **Lifted** (`0 1px 2px ink/6%, 0 4px 10px -2px ink/6%, 0 16px 36px -10px ink/12%`):
  an interactive surface under the pointer.
- **Floating** (`0 1px 2px ink/8%, 0 8px 20px -4px ink/12%, 0 24px 56px -12px ink/20%`):
  menus, popovers, anything over content.
- **Primary glow** (`0 6px 14px -6px violet/45%` plus a 1px inset highlight):
  only under the primary (and danger) button.

### Named Rules
**The Sheet Rule.** Cards are sheets on a desk: raised, never floating, never
nested. Floating is reserved for layers that cover content.

## Shapes

Gently rounded and consistent: chips and pills 6px, controls 8px, floating
layers 10px, boxed rows 12px, cards 14px. One-pixel edges in Border; no colored side stripes.
Avatars and progress bars are fully round.

## Components

All live in `src/ui/kit/`, take their values from `ui::theme`, and are tried
on Overview first.

### Buttons
- **Shape:** gently rounded (8px), 32px tall (28px small, 22px mini).
- **Primary:** Ledger Violet fill, white label, a soft violet glow beneath and a
  light inset along the top edge.
- **Secondary:** white, Control Edge border, a contact shadow.
- **Ghost:** no frame until hovered; used inside rows and next to labels.
- **Danger:** Negative fill, same anatomy as primary; for the main action
  of a destructive confirmation.
- **Danger ghost:** red ink, a pale red fill on hover; for a destructive
  action that sits among others (Reset in Settings).
- **States:** the hover fill springs in (`CONTROL` spring); pressing darkens
  the fill instantly and nudges the face down 1px; keyboard focus draws a
  3px violet ring; Enter and Space activate; selected (a dropdown that is open)
  holds the hover fill; loading swaps the icon for a spinner and keeps the
  width; disabled is 50% opacity, keeps its tooltip so it can say why.

### Cards / Containers
- **Corner Style:** 14px.
- **Background:** Sheet White with a Border edge (dark: the shaded face).
- **Shadow Strategy:** Raised (see Elevation).
- **Header:** Heading title left, one element right (a link, a control, a
  count), inset 24px.

### Overlay and Dropdown
- **Overlay:** a Floating layer hung from its trigger. It rises 6px into
  place while fading in (`RISE` spring), draws above every card, stays 8px
  inside the window, and closes on Escape or a click outside.
- **Dropdown:** a button plus an Overlay menu. Works as a select (check on the
  chosen item, the trigger shows it) or an action menu (icon trigger,
  shortcut hints). Arrow keys move the highlight, Enter picks, and the
  chevron turns as it opens. Destructive items are red; unavailable items
  stay visible, dimmed, with the reason as a hint.

### Segmented control
- A 2px-inset track in the Neutral tint (#F1F2F5); a white thumb glides between
  equal-width cells (`GLIDE` spring), so labels never shift.

### List rows
- Avatar, name over a caption line, right-aligned trailing figures and
  actions. The hover fill fades in. The row is 60px with a 24px inset. Rows
  that no longer count (a reversed payment) grey their title.
- **Boxed** (`.boxed(true)`): each row is its own 12px-rounded raised card,
  56px, for lists that stand on the page under a section heading (a heading,
  an optional count pill, one action on the right) instead of inside a card:
  Overview's Needs attention and Recent payments.

### Stats
- An icon and label, then the figure (Figure role), then a caption. With
  `.chip(color, fill)` the icon sits in a 30px rounded square and the label
  reads in ink.
- **Stat cards:** each stat on its own card, a set number to a row, the last
  row keeping the same cell width. Rows are added before a figure is ever
  shrunk.

### Pills
- 22px, 6px radius, Caption at 500, a status tint behind status text.

## Do's and Don'ts

### Do:
- **Do** take every color, size, radius, shadow and spring from `ui::theme`.
- **Do** right-align amounts in any list or column, with tabular figures.
- **Do** move secondary actions into a Dropdown before letting the header
  wrap.
- **Do** branch layouts on `Width`, never on raw pixel checks.
- **Do** use springs from `theme::motion` for any state change that moves or
  fades.

### Don't:
- **Don't** truncate money to fit a row; change the layout instead.
- **Don't** nest cards or float a card; Floating belongs to overlays.
- **Don't** use status colors for decoration.
- **Don't** add looping or page-load animation; motion only answers input or
  state.
- **Don't** put more than one primary button in a view.
