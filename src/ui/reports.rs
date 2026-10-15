//! Reports: today's numbers at the top, then any report for a date range,
//! exportable to CSV (opens in Excel). Receipts can be voided from here.

use chrono::Datelike;
use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{Action, Report, Session, Table};
use crate::ui::theme::{self, Notice};
use crate::validate;

#[derive(Default)]
pub struct ReportsState {
    kind: Report,
    from: String,
    to: String,
    /// The report on screen, and what it was run for (so it only reloads
    /// when something changes).
    table: Option<Result<Table, String>>,
    loaded_for: Option<(Report, String, String)>,
    void_row: Option<usize>,
    void_reason: String,
    notice: Notice,
}

impl ReportsState {
    /// Reload on the next frame (after a sale, a void, or switching screens).
    pub fn refresh(&mut self) {
        self.loaded_for = None;
    }
}

pub fn show(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut ReportsState) {
    if st.from.is_empty() {
        set_range(st, true);
    }
    ui.heading("Reports");
    match db::stats(conn, &s.branch_id) {
        Ok(x) => {
            ui.horizontal_wrapped(|ui| {
                let warn = |n: bool, colour| if n { colour } else { theme::STATUS_GOOD };
                theme::stat_tile(ui, "Sales today (KSh)", &x.today_sales.to_string(), theme::ACCENT_STRONG);
                theme::stat_tile(ui, "Profit today (KSh)", &x.today_profit.to_string(), theme::STATUS_GOOD);
                theme::stat_tile(
                    ui,
                    "Medicines below 10 units",
                    &x.low_stock.to_string(),
                    warn(x.low_stock > 0, theme::STATUS_CRITICAL),
                );
                theme::stat_tile(
                    ui,
                    "Batches expiring in 90 days",
                    &x.expiring_soon.to_string(),
                    warn(x.expiring_soon > 0, theme::STATUS_SERIOUS),
                );
                theme::stat_tile(
                    ui,
                    "Expired stock (KSh)",
                    &x.expired_value.to_string(),
                    warn(x.expired_value.0 > 0, theme::STATUS_CRITICAL),
                );
            });
        }
        Err(e) => Notice::error(e).show(ui),
    }
    ui.add_space(8.0);
    theme::two_columns(ui, "reports", 300.0, st, picker, |ui, st| report(ui, conn, s, st));
}

fn picker(ui: &mut egui::Ui, st: &mut ReportsState) {
    theme::panel(ui, |ui| {
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for kind in Report::ALL {
                if theme::tab(ui, kind.label(), st.kind == kind).clicked() {
                    st.kind = kind;
                    st.void_row = None;
                }
            }
        });
    });
    ui.add_space(8.0);
    theme::panel(ui, |ui| {
        theme::subheading(ui, "Dates");
        theme::form(ui, "report_dates", |ui| {
            theme::field(ui, "From", &mut st.from, "YYYY-MM-DD");
            theme::field(ui, "To", &mut st.to, "YYYY-MM-DD");
        });
        theme::actions(ui, |ui| {
            if ui.button("Today").clicked() {
                set_range(st, false);
            }
            if ui.button("This month").clicked() {
                set_range(st, true);
            }
            if ui.button("Refresh").clicked() {
                st.refresh();
            }
        });
    });
}

fn set_range(st: &mut ReportsState, whole_month: bool) {
    let today = chrono::Local::now().date_naive();
    let from = if whole_month { today.with_day(1).unwrap_or(today) } else { today };
    st.from = from.to_string();
    st.to = today.to_string();
}

fn report(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut ReportsState) {
    let key = (st.kind, st.from.clone(), st.to.clone());
    if st.loaded_for.as_ref() != Some(&key) {
        st.table = Some(load(conn, s, st));
        st.loaded_for = Some(key);
    }
    ui.horizontal(|ui| {
        theme::section_title(ui, st.kind.label());
        if matches!(st.table, Some(Ok(_))) && ui.button("Export CSV").clicked() {
            st.notice = export(st).into();
        }
    });
    st.notice.show(ui);
    void_panel(ui, conn, s, st);

    // On the receipts report each row can be reprinted, and voided by a manager.
    let receipts = st.kind == Report::Sales;
    let can_void = s.can(Action::VoidSale);
    let (mut void_clicked, mut reprint) = (None, None);
    match &st.table {
        Some(Ok(table)) => theme::report_table(ui, "report_table", table, |ui, i| {
            if !receipts {
                ui.label("");
                return;
            }
            ui.horizontal(|ui| {
                if ui.button("Reprint").clicked() {
                    reprint = Some(table.keys[i].clone());
                }
                let open = table.rows[i].last().is_some_and(|status| status == "done");
                if can_void && open && theme::quiet_button(ui, "Void").clicked() {
                    void_clicked = Some(i);
                }
            });
        }),
        Some(Err(e)) => Notice::error(e).show(ui),
        None => {}
    }
    if void_clicked.is_some() {
        st.void_row = void_clicked;
        st.void_reason.clear();
    }
    if let Some(sale_id) = reprint {
        st.notice = crate::ui::pos::print_receipt(conn, &sale_id).into();
    }
}

fn load(conn: &Connection, s: &Session, st: &ReportsState) -> Result<Table, String> {
    let (from, to) = match st.kind.uses_dates() {
        true => (validate::parse_date("From", &st.from)?.to_string(), validate::parse_date("To", &st.to)?.to_string()),
        false => Default::default(),
    };
    Ok(db::report(conn, st.kind, &s.branch_id, &from, &to)?)
}

fn void_panel(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut ReportsState) {
    let Some(Ok(table)) = &st.table else { return };
    let Some(i) = st.void_row.filter(|&i| i < table.rows.len()) else { return };
    let (sale_id, receipt) = (table.keys[i].clone(), table.rows[i][1].clone());
    theme::panel(ui, |ui| {
        theme::subheading(ui, &format!("Void receipt {receipt}?"));
        theme::muted(ui, "Its medicines go back on the shelf. The void is recorded in the audit log.");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut st.void_reason).hint_text("Reason (required)").desired_width(320.0));
            if theme::danger_button(ui, "Void sale").clicked() {
                let reason = st.void_reason.trim();
                st.notice = if reason.is_empty() {
                    Notice::error("Give a reason for the void.")
                } else {
                    db::void_sale(conn, s, &sale_id, reason)
                        .map(|()| format!("Receipt {receipt} voided."))
                        .map_err(String::from)
                        .into()
                };
                if st.notice.is_success() {
                    st.void_row = None;
                    st.refresh();
                }
            }
            if theme::quiet_button(ui, "Cancel").clicked() {
                st.void_row = None;
            }
        });
    });
}

/// Saves the report on screen as `reports/<name>_<from>_<to>.csv`.
fn export(st: &ReportsState) -> Result<String, String> {
    let Some(Ok(table)) = &st.table else { return Err("Nothing to export.".into()) };
    let cell = |text: &String| {
        if text.contains([',', '"', '\n']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text.clone() }
    };
    // The BOM tells Excel the file is UTF-8 (so "KSh" and "—" show correctly).
    let mut csv = String::from("\u{feff}");
    for row in std::iter::once(&table.headers).chain(&table.rows) {
        csv += &row.iter().map(cell).collect::<Vec<_>>().join(",");
        csv += "\r\n";
    }
    // "Sales (receipts)" -> "sales-receipts"
    let label = st.kind.label().to_lowercase();
    let name = label.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-");
    let dir = std::path::Path::new("reports");
    let path = dir.join(format!("{name}_{}_{}.csv", st.from, st.to));
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&path, csv))
        .map_err(|e| format!("Could not save the CSV: {e}"))?;
    let full = std::env::current_dir().map(|dir| dir.join(&path)).unwrap_or(path);
    Ok(format!("Saved {}", full.display()))
}
