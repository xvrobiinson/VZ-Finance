use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{column, crumb, ncolumn, status_detail_pill};
use crate::app::{Route, VzApp};
use crate::database::repositories::clients::Client;
use crate::domain::ledger::{ContractStatus, DisplayStatus};
use crate::domain::money::Cents;
use crate::i18n::{count, t};
use crate::sync::wire::Need;
use crate::tr;
use crate::ui::components::{Tone, avatar, empty_state, icon, kv, page_header, pill, sel, table_header, table_row, th};
use crate::ui::forms::reason::ReasonKind;
use crate::ui::kit::{Button, Card, Dropdown, DropdownItem, ListRow, SortHeader, Stat, StatCards};
use crate::ui::lists::{self, Key};
use crate::ui::theme::{Palette, RADIUS_CONTROL, Typeset as _, Width, palette, space, tabular, text};

/// Diameter of the avatar beside the profile title.
const PROFILE_AVATAR: f32 = 40.;
/// Height of a shown client photo; it keeps its shape inside.
const PHOTO_HEIGHT: f32 = 220.;

/// Columns the client list can be sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientColumn {
    Name,
    ActiveLoans,
    Borrowed,
    Owes,
    InterestPaid,
    Status,
}

/// Status order for sorting: what needs attention first, archived last.
fn status_rank(c: &Client, st: &ClientStats) -> i64 {
    if c.is_archived() {
        return 6;
    }
    match st.worst {
        Some(DisplayStatus::Overdue) => 0,
        Some(DisplayStatus::Defaulted) => 1,
        Some(DisplayStatus::DueSoon) => 2,
        _ if st.active_loans > 0 => 3,
        _ => 4,
    }
}

/// Per-client figures derived from the ledger.
#[derive(Default, Clone)]
pub struct ClientStats {
    pub active_loans: usize,
    pub total_loans: usize,
    pub borrowed: Cents,
    pub outstanding: Cents,
    pub paid: Cents,
    pub interest_paid: Cents,
    pub worst: Option<DisplayStatus>,
}

impl VzApp {
    pub(crate) fn client_stats(&self, client_id: i64) -> ClientStats {
        let mut s = ClientStats::default();
        for l in self.snap.book.loans.iter().filter(|l| l.client_id == client_id) {
            let pos = l.position(self.today);
            s.total_loans += 1;
            s.borrowed += l.facts.principal;
            s.paid += pos.paid_total;
            s.interest_paid += pos.interest_paid;
            let st = l.display_status(self.today);
            if l.facts.status == ContractStatus::Active {
                s.active_loans += 1;
                s.outstanding += pos.payoff();
            }
            if l.facts.status == ContractStatus::Defaulted {
                s.outstanding += pos.payoff();
            }
            s.worst = Some(match s.worst {
                Some(w) if w <= st => w,
                _ => st,
            });
        }
        s
    }

    fn client_status_pill(&self, p: &Palette, c: &Client, stats: &ClientStats) -> Div {
        if c.is_archived() {
            return pill(p, t("Archived", "Archivado"), Tone::Neutral);
        }
        match stats.worst {
            Some(DisplayStatus::Overdue) => pill(p, DisplayStatus::Overdue.label(), Tone::Negative),
            Some(DisplayStatus::Defaulted) => pill(p, DisplayStatus::Defaulted.label(), Tone::Negative),
            Some(DisplayStatus::DueSoon) => pill(p, DisplayStatus::DueSoon.label(), Tone::Warning),
            _ if stats.active_loans > 0 => pill(p, DisplayStatus::Active.label(), Tone::Positive),
            _ => pill(p, t("No active loans", "Sin préstamos activos"), Tone::Neutral),
        }
    }

    pub(crate) fn render_clients(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let w = Width::of(self.content_width(window));
        // Compact keeps name, balance and status; Medium adds the loan count.
        let show_count = w >= Width::Medium;
        let show_history = w >= Width::Wide;
        let money = self.settings().money();
        let query = self.client_search.read(cx).value().to_lowercase();
        let archived_count = self.snap.clients.iter().filter(|c| c.is_archived()).count();
        let with_loans = self.snap.clients.iter().filter(|c| self.client_stats(c.id).active_loans > 0).count();
        let mut visible: Vec<(&Client, ClientStats)> = self
            .snap
            .clients
            .iter()
            .filter(|c| self.show_archived || !c.is_archived())
            .filter(|c| {
                let opt = |v: &Option<String>| v.clone().unwrap_or_default();
                lists::matches(&query, &[&c.full_name, &opt(&c.phone), &opt(&c.email), &opt(&c.id_number), &opt(&c.address), &c.notes])
            })
            .map(|c| (c, self.client_stats(c.id)))
            .collect();
        lists::sort_rows(&mut visible, self.client_sort, |(c, st), col| match col {
            ClientColumn::Name => Key::text(&c.full_name),
            ClientColumn::ActiveLoans => Key::Number(st.active_loans as i64),
            ClientColumn::Borrowed => Key::Number(st.borrowed),
            ClientColumn::Owes => Key::Number(st.outstanding),
            ClientColumn::InterestPaid => Key::Number(st.interest_paid),
            ClientColumn::Status => Key::Number(status_rank(c, st)),
        });
        let sort = self.client_sort;
        let header_cell = |id: &'static str, label: &'static str, col: ClientColumn, cx: &mut Context<Self>| {
            SortHeader::new(id, label, col, sort).on_click(cx.listener(move |this, _, _, cx| {
                this.client_sort = this.client_sort.toggle(col);
                cx.notify();
            }))
        };
        let total = self.snap.clients.len() - archived_count;
        let subtitle = {
            let n = count(total, "client", "clients", "cliente", "clientes");
            tr!("{n} · {with_loans} with active loans", "{n} · {with_loans} con préstamos activos")
        };

        let header = page_header(
            &p,
            t("Clients", "Clientes"),
            Some(sel(subtitle).into_any_element()),
            self.can(Need::Record)
                .then(|| {
                    Button::new("new-client")
                        .primary()
                        .icon(IconName::UserPlus)
                        .label(t("New client", "Nuevo cliente"))
                        .tooltip("Ctrl+N")
                        .on_click(cx.listener(|this, _, w, cx| this.open_client_form(None, w, cx)))
                        .into_any_element()
                })
                .into_iter()
                .collect(),
        );

        let toolbar = h_flex()
            .justify_between()
            .gap(px(space::M))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .max_w(px(380.))
                    .child(Input::new(&self.client_search).prefix(icon(IconName::Search, p.faint, 14.)).cleanable(true)),
            )
            .when(archived_count > 0, |d| {
                d.child(
                    Button::new("toggle-archived")
                        .ghost()
                        .small()
                        .label(if self.show_archived {
                            tr!("Hide archived ({archived_count})", "Ocultar archivados ({archived_count})")
                        } else {
                            tr!("Show archived ({archived_count})", "Ver archivados ({archived_count})")
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_archived = !this.show_archived;
                            cx.notify();
                        })),
                )
            });

        let body =
            if self.snap.clients.is_empty() {
                empty_state(
                    &p,
                    IconName::Users,
                    t("No clients yet", "Aún no hay clientes"),
                    t(
                        "Add the people you lend to. Each client keeps their loans, payments and notes together.",
                        "Agrega a las personas a quienes prestas. Cada cliente guarda juntos sus préstamos, pagos y notas.",
                    ),
                    self.can(Need::Record).then(|| {
                        Button::new("empty-new-client")
                            .primary()
                            .label(t("Add your first client", "Agrega tu primer cliente"))
                            .on_click(cx.listener(|this, _, w, cx| this.open_client_form(None, w, cx)))
                            .into_any_element()
                    }),
                )
                .into_any_element()
            } else if visible.is_empty() {
                empty_state(
                    &p,
                    IconName::SearchX,
                    t("No matching clients", "Ningún cliente coincide"),
                    &{
                        let q = query.trim();
                        tr!("Nothing matches “{q}”.", "Nada coincide con “{q}”.")
                    },
                    None,
                )
                .into_any_element()
            } else {
                let head =
                    table_header(&p)
                        .child(div().flex_1().min_w_0().child(header_cell("sort-name", t("Client", "Cliente"), ClientColumn::Name, cx)))
                        .when(show_count, |d| {
                            d.child(
                                ncolumn(90.).flex().justify_end().child(
                                    header_cell("sort-active", t("Active loans", "Préstamos"), ClientColumn::ActiveLoans, cx).right(),
                                ),
                            )
                        })
                        .when(show_history, |d| {
                            d.child(ncolumn(140.).flex().justify_end().child(
                                header_cell("sort-borrowed", t("Total borrowed", "Total prestado"), ClientColumn::Borrowed, cx).right(),
                            ))
                        })
                        .child(
                            ncolumn(140.)
                                .flex()
                                .justify_end()
                                .child(header_cell("sort-owes", t("Owes", "Debe"), ClientColumn::Owes, cx).right()),
                        )
                        .when(show_history, |d| {
                            d.child(ncolumn(130.).flex().justify_end().child(
                                header_cell("sort-interest", t("Interest paid", "Interés pagado"), ClientColumn::InterestPaid, cx).right(),
                            ))
                        })
                        .child(
                            ncolumn(130.)
                                .flex()
                                .justify_end()
                                .child(header_cell("sort-status", t("Status", "Estado"), ClientColumn::Status, cx).right()),
                        );
                let rows = visible.iter().map(|(c, st)| {
                    let id = c.id;
                    let contact = [c.phone.clone(), c.email.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                    table_row(&p, ("client", id as u64), true)
                        .child(
                            h_flex().flex_1().min_w_0().gap(px(space::M)).child(avatar(&p, &c.full_name, c.id, 32.)).child(
                                v_flex()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .typeset(text::BODY)
                                            .text_color(if c.is_archived() { p.muted } else { p.text })
                                            .truncate()
                                            .child(sel(c.full_name.clone())),
                                    )
                                    .child(div().typeset(text::CAPTION).text_color(p.muted).truncate().child(sel(if contact.is_empty() {
                                        t("No contact details", "Sin datos de contacto").to_string()
                                    } else {
                                        contact
                                    }))),
                            ),
                        )
                        .when(show_count, |d| d.child(ncolumn(90.).child(sel(st.active_loans.to_string()))))
                        .when(show_history, |d| d.child(ncolumn(140.).text_color(p.muted).child(sel(money.format(st.borrowed)))))
                        .child(ncolumn(140.).child(sel(money.format(st.outstanding))))
                        .when(show_history, |d| d.child(ncolumn(130.).text_color(p.muted).child(sel(money.format(st.interest_paid)))))
                        .child(column(130.).flex().justify_end().child(self.client_status_pill(&p, c, st)))
                        .on_click(cx.listener(move |this, _, w, cx| this.open_row(Route::Client(id), w, cx)))
                });
                v_flex()
                    .child(head)
                    .children(rows)
                    .child(div().px(px(space::XL)).py(px(space::M)).typeset(text::CAPTION).text_color(p.faint).child(sel({
                        let n = visible.len();
                        tr!("{n} shown", "{n} en la lista")
                    })))
                    .into_any_element()
            };

        v_flex().child(header).child(toolbar.pb(px(space::L))).child(Card::new().overflow_hidden().child(body)).into_any_element()
    }

    pub(crate) fn render_client(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let w = Width::of(self.content_width(window));
        let gap = px(w.gap());
        let side_by_side = w >= Width::Wide;
        let Some(client) = self.snap.client(id).cloned() else {
            return div().child(t("Client not found", "Cliente no encontrado")).into_any_element();
        };
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let st = self.client_stats(id);
        let since = chrono::NaiveDate::parse_from_str(client.created_at.get(..10).unwrap_or(""), "%Y-%m-%d").ok();
        let contact = [
            client.phone.clone(),
            client.email.clone(),
            since.map(|d| {
                let when = crate::utils::dates::short_month_year(d);
                tr!("Client since {when}", "Cliente desde {when}")
            }),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("  ·  ");
        let archived = client.is_archived();

        let header = h_flex()
            .flex_wrap()
            .justify_between()
            .items_end()
            .gap(px(space::L))
            .child(
                v_flex()
                    .min_w_0()
                    .gap(px(space::S))
                    .child(
                        h_flex()
                            .gap(px(space::M))
                            .child(avatar(&p, &client.full_name, client.id, PROFILE_AVATAR))
                            .child(div().min_w_0().truncate().typeset(text::TITLE).text_color(p.text).child(sel(client.full_name.clone())))
                            .child(self.client_status_pill(&p, &client, &st)),
                    )
                    .child(div().pl(px(PROFILE_AVATAR + space::M)).text_color(p.muted).child(sel(if contact.is_empty() {
                        t("No contact details", "Sin datos de contacto").to_string()
                    } else {
                        contact
                    }))),
            )
            .child(h_flex().flex_none().gap(px(space::S)).children(self.client_actions(id, archived, &st, w, cx)));

        // Four figures: one row when wide, two by two below that.
        let per_row = if side_by_side { 4 } else { 2 };
        let cells = vec![
            Stat::new(t("Owes", "Debe"), money.format(st.outstanding))
                .icon(IconName::HandCoins)
                .chip(p.info, p.info_tint)
                .caption(sel(count(st.active_loans, "active loan", "active loans", "préstamo activo", "préstamos activos"))),
            Stat::new(t("Total paid", "Total pagado"), money.format(st.paid))
                .icon(IconName::Wallet)
                .chip(p.accent_text, p.accent_tint)
                .caption(sel(t("Capital, interest and fees", "Capital, interés y cargos"))),
            Stat::new(t("Interest paid", "Interés pagado"), money.format(st.interest_paid))
                .icon(IconName::TrendingUp)
                .chip(p.positive, p.positive_tint)
                .caption(sel(t("Income from this client", "Ganancia con este cliente"))),
            Stat::new(t("Total borrowed", "Total prestado"), money.format(st.borrowed))
                .icon(IconName::Landmark)
                .chip(p.accent_text, p.accent_tint)
                .caption(sel({
                    let n = count(st.total_loans, "loan", "loans", "préstamo", "préstamos");
                    tr!("{n} in total", "{n} en total")
                })),
        ];
        let strip = StatCards::new(per_row).spacing(w.gap()).stats(cells);

        // Loans, active first. Compact drops the amount lent and the next
        // due date; balance and status stay.
        let full_loans = w >= Width::Medium;
        let mut loans: Vec<_> = self.snap.book.loans.iter().filter(|l| l.client_id == id).collect();
        loans.sort_by_key(|l| (l.facts.status != ContractStatus::Active, std::cmp::Reverse(l.facts.start_date)));
        let loan_rows = loans.iter().map(|l| {
            let lid = l.id;
            let pos = l.position(self.today);
            let q = l.delinquency(self.today);
            let status = l.display_status(self.today);
            let terms = self.snap.loan(lid).map(|x| x.terms.clone());
            let desc = terms.map(|lt| format!("{} · {}", lt.rate_line(), ds.format(lt.start_date))).unwrap_or_default();
            table_row(&p, ("cl-loan", lid as u64), true)
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().typeset(text::BODY).truncate().child(sel(tr!("Loan #{lid}", "Préstamo #{lid}"))))
                        .child(div().typeset(text::CAPTION).font_features(tabular()).text_color(p.muted).truncate().child(sel(desc))),
                )
                .when(full_loans, |d| d.child(ncolumn(130.).text_color(p.muted).child(sel(money.format(l.facts.principal)))))
                .child(ncolumn(130.).child(sel(money.format(pos.payoff()))))
                .when(full_loans, |d| {
                    d.child(
                        ncolumn(120.)
                            .text_color(p.muted)
                            .child(sel(q.next_due.map(|d| ds.format_short(d, self.today)).unwrap_or_else(|| "—".into()))),
                    )
                })
                .child(column(130.).flex().justify_end().child(status_detail_pill(&p, status, &q, self.today)))
                .on_click(cx.listener(move |this, _, w, cx| this.open_row(Route::Loan(lid), w, cx)))
        });
        let loans_card = Card::new().overflow_hidden().title(t("Loans", "Préstamos")).child(if loans.is_empty() {
            empty_state(
                &p,
                IconName::HandCoins,
                t("No loans yet", "Aún no hay préstamos"),
                t("Create a loan to track its interest and payments here.", "Crea un préstamo para seguir aquí sus intereses y pagos."),
                (!archived && self.can(Need::Manage)).then(|| {
                    Button::new("cl-empty-loan")
                        .primary()
                        .label(t("New loan", "Nuevo préstamo"))
                        .on_click(cx.listener(move |this, _, w, cx| this.open_loan_form(Some(id), w, cx)))
                        .into_any_element()
                }),
            )
            .into_any_element()
        } else {
            v_flex()
                .child(
                    table_header(&p)
                        .child(div().flex_1().min_w_0().child(th(&p, t("Loan", "Préstamo"))))
                        .when(full_loans, |d| d.child(ncolumn(130.).child(th(&p, t("Lent", "Prestado")))))
                        .child(ncolumn(130.).child(th(&p, t("Owes today", "Debe hoy"))))
                        .when(full_loans, |d| d.child(ncolumn(120.).child(th(&p, t("Next payment", "Próximo pago")))))
                        .child(ncolumn(130.).child(th(&p, t("Status", "Estado")))),
                )
                .children(loan_rows)
                .into_any_element()
        });

        // Payment timeline across all of this client's loans.
        let loan_ids: Vec<i64> = loans.iter().map(|l| l.id).collect();
        let pays: Vec<_> = self.snap.payments.iter().rev().filter(|x| loan_ids.contains(&x.loan_id)).take(12).collect();
        let timeline = pays.iter().map(|x| {
            let reversed = x.is_reversed();
            ListRow::new(("cl-pay", x.id as u64))
                .muted(reversed)
                .leading(div().flex_none().size(px(space::S)).rounded_full().bg(if reversed { p.control } else { p.positive }))
                .title({
                    let (when, lid) = (ds.format(x.paid_on), x.loan_id);
                    tr!("{when} · Loan #{lid}", "{when} · Préstamo #{lid}")
                })
                .subtitle(if reversed {
                    let why = x.reversal_reason.clone().unwrap_or_default();
                    tr!("Reversed: {why}", "Anulado: {why}")
                } else {
                    let (i, c) = (money.format(x.interest), money.format(x.principal));
                    tr!("Interest {i} · Capital {c}", "Interés {i} · Capital {c}")
                })
                .trailing(
                    div()
                        .flex_none()
                        .typeset(text::BODY_STRONG)
                        .font_features(tabular())
                        .whitespace_nowrap()
                        .text_color(if reversed { p.muted } else { p.positive })
                        .when(reversed, |d| d.line_through())
                        .child(sel(format!("+{}", money.format(x.amount)))),
                )
        });
        let timeline_card = Card::new().overflow_hidden().title(t("Payment history", "Historial de pagos")).child(if pays.is_empty() {
            empty_state(
                &p,
                IconName::Receipt,
                t("No payments yet", "Aún no hay pagos"),
                t(
                    "Payments from this client appear here, newest first.",
                    "Los pagos de este cliente aparecen aquí, del más reciente al más antiguo.",
                ),
                None,
            )
            .into_any_element()
        } else {
            v_flex().pb(px(space::S)).children(timeline).into_any_element()
        });

        let notes_card = Card::new()
            .title(t("Notes", "Notas"))
            .when(self.can(Need::Manage), |c| {
                c.action(
                    Button::new("edit-notes")
                        .ghost()
                        .small()
                        .label(t("Edit", "Editar"))
                        .on_click(cx.listener(move |this, _, w, cx| this.open_client_form(Some(id), w, cx))),
                )
            })
            .child(
                div()
                    .px(px(space::XL))
                    .pb(px(space::XL))
                    .typeset(text::BODY)
                    .text_color(if client.notes.is_empty() { p.muted } else { p.text })
                    .child(sel(if client.notes.is_empty() { t("No notes yet.", "Sin notas.").to_string() } else { client.notes.clone() })),
            );

        let details_card = self.client_details(&p, &client, cx);

        // Loans and history beside the details and notes when wide (2 : 1,
        // that column held between 340 and 480px); stacked below that.
        let body = if side_by_side {
            h_flex()
                .items_start()
                .gap(gap)
                .child(v_flex().flex_grow(2.).flex_basis(px(0.)).min_w_0().gap(gap).child(loans_card).child(timeline_card))
                .child(
                    v_flex()
                        .flex_grow(1.)
                        .flex_basis(px(0.))
                        .min_w(px(340.))
                        .max_w(px(480.))
                        .gap(gap)
                        .children(details_card)
                        .child(notes_card),
                )
                .into_any_element()
        } else {
            v_flex().gap(gap).child(loans_card).children(details_card).child(notes_card).child(timeline_card).into_any_element()
        };

        v_flex()
            .child(crumb(&p, t("Clients", "Clientes"), Route::Clients, cx))
            .child(v_flex().gap(gap).child(header).child(strip).child(body))
            .into_any_element()
    }

    /// ID number, address and photo, each only when the client has it; no
    /// card at all when they have none. The photo may show an identity
    /// document, so it stays hidden until asked for.
    fn client_details(&self, p: &Palette, client: &Client, cx: &mut Context<Self>) -> Option<Card> {
        let lines: Vec<(&str, String)> =
            [(t("ID number", "Identificación"), &client.id_number), (t("Address", "Dirección"), &client.address)]
                .into_iter()
                .filter_map(|(label, v)| v.clone().map(|v| (label, v)))
                .collect();
        if lines.is_empty() && !client.has_photo {
            return None;
        }
        let id = client.id;
        let shown = self.shown_photo.as_ref().filter(|(cid, _)| *cid == id).map(|(_, image)| image.clone());
        let photo = client.has_photo.then(|| {
            v_flex()
                .gap(px(space::S))
                .pt(px(space::S))
                .child(
                    h_flex()
                        .justify_between()
                        .gap(px(space::L))
                        .child(div().text_color(p.muted).child(t("Photo or ID", "Foto o identificación")))
                        .child(
                            Button::new("toggle-photo")
                                .small()
                                .ghost()
                                .icon(if shown.is_some() { IconName::EyeOff } else { IconName::Eye })
                                .label(if shown.is_some() { t("Hide", "Ocultar") } else { t("Show", "Mostrar") })
                                .on_click(cx.listener(move |this, _, w, cx| this.toggle_client_photo(id, w, cx))),
                        ),
                )
                .children(shown.map(|image| {
                    div()
                        .w_full()
                        .h(px(PHOTO_HEIGHT))
                        .rounded(px(RADIUS_CONTROL))
                        .border_1()
                        .border_color(p.rule)
                        .bg(p.raised)
                        .overflow_hidden()
                        .child(img(image).size_full().object_fit(ObjectFit::Contain))
                }))
        });
        Some(Card::new().title(t("Identification", "Identificación")).child(
            v_flex().px(px(space::XL)).pb(px(space::L)).children(lines.into_iter().map(|(label, v)| kv(p, label, sel(v)))).children(photo),
        ))
    }

    /// Shows the client's photo, read from the database only now, or hides it.
    fn toggle_client_photo(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if self.shown_photo.as_ref().is_some_and(|(cid, _)| *cid == id) {
            self.shown_photo = None;
        } else {
            match self.store.client_photo(id) {
                Ok(Some(bytes)) => self.shown_photo = Some((id, Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes)))),
                Ok(None) => self.notify_error(
                    t("This client's photo is no longer on file.", "La foto de este cliente ya no está guardada."),
                    window,
                    cx,
                ),
                Err(e) => {
                    log::warn!("client photo not loaded: {e:#}");
                    self.notify_error(
                        t("The photo couldn't be read. Try again.", "No se pudo leer la foto. Inténtalo de nuevo."),
                        window,
                        cx,
                    )
                }
            }
        }
        cx.notify();
    }

    /// Profile actions. Wide windows show them all; narrower ones keep the
    /// main action and fold the rest into a "More" menu so the header never
    /// wraps its buttons.
    fn client_actions(&self, id: i64, archived: bool, st: &ClientStats, w: Width, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let has_loans = st.total_loans > 0;
        let first_active = self.snap.loans_for_client(id).find(|l| l.status == ContractStatus::Active).map(|l| l.id);
        let pay = (st.active_loans > 0 && self.can(Need::Record)).then(|| {
            Button::new("client-pay")
                .primary()
                .icon(IconName::Plus)
                .label(t("Record payment", "Registrar pago"))
                .on_click(cx.listener(move |this, _, w, cx| {
                    let only = if this.client_stats(id).active_loans == 1 { first_active } else { None };
                    this.open_payment_form(only, w, cx)
                }))
                .into_any_element()
        });
        let edit = cx.listener(move |this, _, w, cx| this.open_client_form(Some(id), w, cx));
        let archive = cx.listener(move |this, _, w, cx| this.confirm_archive(id, w, cx));
        let delete = cx.listener(move |this, _, w, cx| this.open_reason_form(ReasonKind::DeleteClient(id), w, cx));
        let new_loan = cx.listener(move |this, _, w, cx| this.open_loan_form(Some(id), w, cx));
        let archive_icon = if archived { IconName::ArchiveRestore } else { IconName::Archive };
        let archive_label = if archived { t("Restore", "Restaurar") } else { t("Archive", "Archivar") };
        if !self.can(Need::Manage) {
            return pay.into_iter().collect();
        }

        if w >= Width::Wide {
            let mut actions = vec![
                Button::new("edit-client").ghost().icon(IconName::Pencil).label(t("Edit", "Editar")).on_click(edit).into_any_element(),
                Button::new("archive-client").ghost().icon(archive_icon).label(archive_label).on_click(archive).into_any_element(),
                Button::new("delete-client")
                    .ghost()
                    .icon(IconName::Trash)
                    .disabled(has_loans)
                    .tooltip(if has_loans {
                        t(
                            "Clients with loans can't be deleted. Delete the loans or move them to another client first.",
                            "No se puede eliminar un cliente con préstamos. Elimínalos o pásalos a otro cliente primero.",
                        )
                    } else {
                        t("Delete client", "Eliminar cliente")
                    })
                    .on_click(delete)
                    .into_any_element(),
            ];
            if !archived {
                actions.push(Button::new("client-new-loan").label(t("New loan", "Nuevo préstamo")).on_click(new_loan).into_any_element());
            }
            actions.extend(pay);
            return actions;
        }

        // With no payment to record, "New loan" stays out as the main action.
        let mut more = Dropdown::new("client-more").icon(IconName::Ellipsis).tooltip(t("More actions", "Más acciones")).align_end();
        let mut actions = Vec::new();
        match (archived, pay.is_some()) {
            (true, _) => {}
            (false, true) => more = more.item(DropdownItem::new(t("New loan", "Nuevo préstamo"), new_loan).icon(IconName::HandCoins)),
            (false, false) => {
                actions.push(Button::new("client-new-loan").label(t("New loan", "Nuevo préstamo")).on_click(new_loan).into_any_element())
            }
        }
        more = more
            .item(DropdownItem::new(t("Edit", "Editar"), edit).icon(IconName::Pencil))
            .item(DropdownItem::new(archive_label, archive).icon(archive_icon));
        // A client with loans can't be deleted; the item stays, disabled, so
        // the menu can say why.
        more = more.item(
            DropdownItem::new(t("Delete client", "Eliminar cliente"), delete)
                .icon(IconName::Trash)
                .danger()
                .disabled(has_loans)
                .when(has_loans, |i| i.hint(t("Has loans", "Tiene préstamos"))),
        );
        actions.insert(0, more.into_any_element());
        actions.extend(pay);
        actions
    }
}
