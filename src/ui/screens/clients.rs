use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{crumb, ncolumn, status_detail_pill};
use crate::app::{Route, VzApp};
use crate::database::repositories::clients::Client;
use crate::domain::ledger::{ContractStatus, DisplayStatus};
use crate::domain::money::Cents;
use crate::i18n::{count, t};
use crate::tr;
use crate::ui::components::{
    Tone, avatar, card, card_header, empty_state, figure, icon, label, page_header, pill, sel, table_header, table_row, th, vrule,
};
use crate::ui::forms::reason::ReasonKind;
use crate::ui::theme::{Palette, palette, size, space, tabular};

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
        let narrow = self.content_width(window) < 1000.;
        let money = self.settings().money();
        let query = self.client_search.read(cx).value().to_lowercase();
        let archived_count = self.snap.clients.iter().filter(|c| c.is_archived()).count();
        let with_loans = self.snap.clients.iter().filter(|c| self.client_stats(c.id).active_loans > 0).count();
        let visible: Vec<(&Client, ClientStats)> = self
            .snap
            .clients
            .iter()
            .filter(|c| self.show_archived || !c.is_archived())
            .filter(|c| {
                query.trim().is_empty() || {
                    let q = query.trim();
                    c.full_name.to_lowercase().contains(q)
                        || c.phone.as_deref().is_some_and(|x| x.to_lowercase().contains(q))
                        || c.email.as_deref().is_some_and(|x| x.to_lowercase().contains(q))
                }
            })
            .map(|c| (c, self.client_stats(c.id)))
            .collect();
        let total = self.snap.clients.len() - archived_count;
        let subtitle = {
            let n = count(total, "client", "clients", "cliente", "clientes");
            tr!("{n} · {with_loans} with active loans", "{n} · {with_loans} con préstamos activos")
        };

        let header = page_header(
            &p,
            t("Clients", "Clientes"),
            Some(sel(subtitle).into_any_element()),
            vec![
                Button::new("new-client")
                    .primary()
                    .icon(IconName::UserPlus)
                    .label(t("New client", "Nuevo cliente"))
                    .tooltip("Ctrl+N")
                    .on_click(cx.listener(|this, _, w, cx| this.open_client_form(None, w, cx)))
                    .into_any_element(),
            ],
        );

        let toolbar = h_flex()
            .justify_between()
            .pb(px(space::L))
            .child(div().w(px(380.)).child(Input::new(&self.client_search).prefix(icon(IconName::Search, p.faint, 14.)).cleanable(true)))
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

        let body = if self.snap.clients.is_empty() {
            empty_state(
                &p,
                IconName::Users,
                t("No clients yet", "Aún no hay clientes"),
                t(
                    "Add the people you lend to. Each client keeps their loans, payments and notes together.",
                    "Agrega a las personas a quienes prestas. Cada cliente guarda juntos sus préstamos, pagos y notas.",
                ),
                Some(
                    Button::new("empty-new-client")
                        .primary()
                        .label(t("Add your first client", "Agrega tu primer cliente"))
                        .on_click(cx.listener(|this, _, w, cx| this.open_client_form(None, w, cx)))
                        .into_any_element(),
                ),
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
            let rows = visible.iter().map(|(c, st)| {
                let id = c.id;
                let contact = [c.phone.clone(), c.email.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                table_row(&p, ("client", id as u64), true)
                    .child(
                        h_flex().flex_1().min_w(px(200.)).gap(px(space::M)).child(avatar(&p, &c.full_name, c.id, 32.)).child(
                            v_flex()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_color(if c.is_archived() { p.muted } else { p.text })
                                        .truncate()
                                        .child(sel(c.full_name.clone())),
                                )
                                .child(div().text_size(px(size::META)).text_color(p.muted).truncate().child(sel(if contact.is_empty() {
                                    t("No contact details", "Sin datos de contacto").to_string()
                                } else {
                                    contact
                                }))),
                        ),
                    )
                    .child(ncolumn(90.).font_features(tabular()).child(sel(st.active_loans.to_string())))
                    .when(!narrow, |d| {
                        d.child(ncolumn(140.).font_features(tabular()).text_color(p.muted).child(sel(money.format(st.borrowed))))
                    })
                    .child(ncolumn(140.).font_features(tabular()).child(sel(money.format(st.outstanding))))
                    .when(!narrow, |d| {
                        d.child(ncolumn(130.).font_features(tabular()).text_color(p.muted).child(sel(money.format(st.interest_paid))))
                    })
                    .child(div().w(px(130.)).flex().justify_end().child(self.client_status_pill(&p, c, st)))
                    .on_click(cx.listener(move |this, _, w, cx| this.open_row(Route::Client(id), w, cx)))
            });
            v_flex()
                .child(
                    table_header(&p)
                        .child(div().flex_1().min_w(px(200.)).child(th(&p, t("Client", "Cliente"))))
                        .child(ncolumn(90.).child(th(&p, t("Active loans", "Préstamos"))))
                        .when(!narrow, |d| d.child(ncolumn(140.).child(th(&p, t("Total borrowed", "Total prestado")))))
                        .child(ncolumn(140.).child(th(&p, t("Owes", "Debe"))))
                        .when(!narrow, |d| d.child(ncolumn(130.).child(th(&p, t("Interest paid", "Interés pagado")))))
                        .child(ncolumn(130.).child(th(&p, t("Status", "Estado")))),
                )
                .children(rows)
                .child(div().px(px(space::XL)).py(px(space::M)).text_size(px(size::META)).text_color(p.faint).child(sel({
                    let n = visible.len();
                    tr!("{n} shown", "{n} en la lista")
                })))
                .into_any_element()
        };

        v_flex().child(header).child(toolbar).child(card(&p).overflow_hidden().child(body)).into_any_element()
    }

    pub(crate) fn render_client(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let narrow = self.content_width(window) < 1000.;
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

        let mut actions = vec![
            Button::new("edit-client")
                .ghost()
                .icon(IconName::Pencil)
                .label(t("Edit", "Editar"))
                .on_click(cx.listener(move |this, _, w, cx| this.open_client_form(Some(id), w, cx)))
                .into_any_element(),
            Button::new("archive-client")
                .ghost()
                .icon(if archived { IconName::ArchiveRestore } else { IconName::Archive })
                .label(if archived { t("Restore", "Restaurar") } else { t("Archive", "Archivar") })
                .on_click(cx.listener(move |this, _, w, cx| this.confirm_archive(id, w, cx)))
                .into_any_element(),
            Button::new("delete-client")
                .ghost()
                .icon(IconName::Trash)
                .disabled(st.total_loans > 0)
                .tooltip(if st.total_loans > 0 {
                    t(
                        "Clients with loans can't be deleted. Delete the loans or move them to another client first.",
                        "No se puede eliminar un cliente con préstamos. Elimínalos o pásalos a otro cliente primero.",
                    )
                } else {
                    t("Delete client", "Eliminar cliente")
                })
                .on_click(cx.listener(move |this, _, w, cx| this.open_reason_form(ReasonKind::DeleteClient(id), w, cx)))
                .into_any_element(),
        ];
        if !archived {
            actions.push(
                Button::new("client-new-loan")
                    .label(t("New loan", "Nuevo préstamo"))
                    .on_click(cx.listener(move |this, _, w, cx| this.open_loan_form(Some(id), w, cx)))
                    .into_any_element(),
            );
        }
        let first_active = self.snap.loans_for_client(id).find(|l| l.status == ContractStatus::Active).map(|l| l.id);
        if st.active_loans > 0 {
            actions.push(
                Button::new("client-pay")
                    .primary()
                    .icon(IconName::Plus)
                    .label(t("Record payment", "Registrar pago"))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        let only = if this.client_stats(id).active_loans == 1 { first_active } else { None };
                        this.open_payment_form(only, w, cx)
                    }))
                    .into_any_element(),
            );
        }

        let title = h_flex()
            .gap(px(space::M))
            .child(avatar(&p, &client.full_name, client.id, 40.))
            .child(sel(client.full_name.clone()))
            .child(self.client_status_pill(&p, &client, &st));
        let header = h_flex()
            .justify_between()
            .items_end()
            .gap(px(space::L))
            .pb(px(space::XL))
            .child(v_flex().gap(px(8.)).child(div().text_size(px(size::TITLE)).font_weight(FontWeight::SEMIBOLD).child(title)).child(
                div().pl(px(52.)).text_color(p.muted).child(sel(if contact.is_empty() {
                    t("No contact details", "Sin datos de contacto").to_string()
                } else {
                    contact
                })),
            ))
            .child(h_flex().gap(px(space::S)).children(actions));

        let cell = |label_text: &str, value: String, sub: String| {
            v_flex()
                .flex_1()
                .flex_basis(px(0.))
                .min_w_0()
                .px(px(space::XL))
                .py(px(space::L + 2.))
                .gap(px(6.))
                .child(label(&p, label_text.to_string()).truncate())
                .child(figure(&p, value, size::FIGURE))
                .child(div().text_size(px(size::META)).text_color(p.muted).truncate().child(sel(sub)))
        };
        let strip = card(&p)
            .flex_row()
            .child(cell(
                t("Owes", "Debe"),
                money.format(st.outstanding),
                count(st.active_loans, "active loan", "active loans", "préstamo activo", "préstamos activos"),
            ))
            .child(vrule(&p))
            .child(cell(
                t("Total paid", "Total pagado"),
                money.format(st.paid),
                t("Capital, interest and fees", "Capital, interés y cargos").into(),
            ))
            .child(vrule(&p))
            .child(cell(
                t("Interest paid", "Interés pagado"),
                money.format(st.interest_paid),
                t("Income from this client", "Ganancia con este cliente").into(),
            ))
            .child(vrule(&p))
            .child(cell(t("Total borrowed", "Total prestado"), money.format(st.borrowed), {
                let n = count(st.total_loans, "loan", "loans", "préstamo", "préstamos");
                tr!("{n} in total", "{n} en total")
            }));

        // Loans, active first.
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
                        .child(div().child(sel(tr!("Loan #{lid}", "Préstamo #{lid}"))))
                        .child(div().text_size(px(size::META)).text_color(p.muted).truncate().child(sel(desc))),
                )
                .child(ncolumn(130.).font_features(tabular()).text_color(p.muted).child(sel(money.format(l.facts.principal))))
                .child(ncolumn(130.).font_features(tabular()).child(sel(money.format(pos.payoff()))))
                .child(
                    ncolumn(120.)
                        .text_color(p.muted)
                        .child(sel(q.next_due.map(|d| ds.format_short(d, self.today)).unwrap_or_else(|| "—".into()))),
                )
                .child(div().w(px(130.)).flex().justify_end().child(status_detail_pill(&p, status, &q, self.today)))
                .on_click(cx.listener(move |this, _, w, cx| this.open_row(Route::Loan(lid), w, cx)))
        });
        let loans_card = card(&p).overflow_hidden().child(card_header(&p, t("Loans", "Préstamos"), None)).child(if loans.is_empty() {
            empty_state(
                &p,
                IconName::HandCoins,
                t("No loans yet", "Aún no hay préstamos"),
                t("Create a loan to track its interest and payments here.", "Crea un préstamo para seguir aquí sus intereses y pagos."),
                (!archived).then(|| {
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
                        .child(div().flex_1().child(th(&p, t("Loan", "Préstamo"))))
                        .child(ncolumn(130.).child(th(&p, t("Lent", "Prestado"))))
                        .child(ncolumn(130.).child(th(&p, t("Owes today", "Debe hoy"))))
                        .child(ncolumn(120.).child(th(&p, t("Next payment", "Próximo pago"))))
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
            h_flex()
                .min_h(px(48.))
                .px(px(space::XL))
                .gap(px(space::M))
                .border_b_1()
                .border_color(p.rule)
                .child(div().size(px(8.)).rounded_full().bg(if reversed { p.control } else { p.positive }))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_color(if reversed { p.muted } else { p.text }).child(sel({
                            let (when, lid) = (ds.format(x.paid_on), x.loan_id);
                            tr!("{when} · Loan #{lid}", "{when} · Préstamo #{lid}")
                        })))
                        .child(div().text_size(px(size::META)).text_color(p.muted).truncate().child(sel(if reversed {
                            {
                                let why = x.reversal_reason.clone().unwrap_or_default();
                                tr!("Reversed: {why}", "Anulado: {why}")
                            }
                        } else {
                            {
                                let (i, c) = (money.format(x.interest), money.format(x.principal));
                                tr!("Interest {i} · Capital {c}", "Interés {i} · Capital {c}")
                            }
                        }))),
                )
                .child(
                    div()
                        .font_features(tabular())
                        .text_color(if reversed { p.muted } else { p.positive })
                        .when(reversed, |d| d.line_through())
                        .child(sel(format!("+{}", money.format(x.amount)))),
                )
        });
        let timeline_card =
            card(&p).overflow_hidden().child(card_header(&p, t("Payment history", "Historial de pagos"), None)).child(if pays.is_empty() {
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
                v_flex().children(timeline).into_any_element()
            });

        let notes_card =
            card(&p)
                .child(card_header(
                    &p,
                    t("Notes", "Notas"),
                    Some(
                        Button::new("edit-notes")
                            .ghost()
                            .small()
                            .label(t("Edit", "Editar"))
                            .on_click(cx.listener(move |this, _, w, cx| this.open_client_form(Some(id), w, cx)))
                            .into_any_element(),
                    ),
                ))
                .child(
                    div().px(px(space::XL)).pb(px(space::XL)).text_color(if client.notes.is_empty() { p.muted } else { p.text }).child(
                        sel(if client.notes.is_empty() { t("No notes yet.", "Sin notas.").to_string() } else { client.notes.clone() }),
                    ),
                );

        v_flex()
            .child(crumb(&p, t("Clients", "Clientes"), Route::Clients, cx))
            .child(header)
            .child(v_flex().gap(px(space::XL)).child(strip).child(if narrow {
                v_flex().gap(px(space::XL)).child(loans_card).child(notes_card).child(timeline_card).into_any_element()
            } else {
                h_flex()
                    .items_start()
                    .gap(px(space::XL))
                    .child(v_flex().flex_1().min_w_0().gap(px(space::XL)).child(loans_card).child(timeline_card))
                    .child(v_flex().w(px(340.)).flex_none().gap(px(space::XL)).child(notes_card))
                    .into_any_element()
            }))
            .into_any_element()
    }
}
