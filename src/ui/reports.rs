//! Reports: today's numbers at the top, then any report for a period (this
//! week, last 3 months, ... or your own dates), saved as CSV (opens in
//! Excel) or PDF in Documents\MedTill reports. Receipts can be voided here.

use std::path::PathBuf;

use chrono::{Datelike, NaiveDate};
use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{Action, Report, Session, Table};
use crate::ui::theme::{self, Notice};
use crate::validate;

#[derive(Default)]
pub struct ReportsState {
    kind: Report,
    period: Period,
    from: String,
    to: String,
    /// The report on screen, and what it was run for (so it only reloads
    /// when something changes).
    table: Option<Result<Table, String>>,
    loaded_for: Option<(Report, String, String)>,
    void_row: Option<usize>,
    void_reason: String,
    notice: Notice,
    /// The last file saved, for "Open folder".
    saved: Option<PathBuf>,
}

/// The date ranges offered; each ends today, except your own dates.
#[derive(Clone, Copy, PartialEq, Default)]
enum Period {
    Today,
    ThisWeek,
    Last7Days,
    #[default]
    ThisMonth,
    Last3Months,
    Last6Months,
    Last12Months,
    Custom,
}

impl Period {
    const ALL: [Period; 8] = [
        Period::Today,
        Period::ThisWeek,
        Period::Last7Days,
        Period::ThisMonth,
        Period::Last3Months,
        Period::Last6Months,
        Period::Last12Months,
        Period::Custom,
    ];

    fn label(self) -> &'static str {
        match self {
            Period::Today => "Today",
            Period::ThisWeek => "This week",
            Period::Last7Days => "Last 7 days",
            Period::ThisMonth => "This month",
            Period::Last3Months => "Last 3 months",
            Period::Last6Months => "Last 6 months",
            Period::Last12Months => "Last 12 months",
            Period::Custom => "Choose dates",
        }
    }

    /// First and last day (`None` for your own dates). Weeks start on Monday.
    fn range(self, today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
        let months = |n| today.checked_sub_months(chrono::Months::new(n)).and_then(|d| d.succ_opt());
        let from = match self {
            Period::Today => Some(today),
            Period::ThisWeek => Some(today - chrono::Days::new(today.weekday().num_days_from_monday().into())),
            Period::Last7Days => Some(today - chrono::Days::new(6)),
            Period::ThisMonth => today.with_day(1),
            Period::Last3Months => months(3),
            Period::Last6Months => months(6),
            Period::Last12Months => months(12),
            Period::Custom => None,
        };
        from.map(|from| (from, today))
    }
}

impl ReportsState {
    /// Reload on the next frame (after a sale, a void, or switching screens).
    pub fn refresh(&mut self) {
        self.loaded_for = None;
    }
}

pub fn show(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut ReportsState) {
    // Recomputed every time, so "Today" is still right after midnight.
    if let Some((from, to)) = st.period.range(chrono::Local::now().date_naive()) {
        (st.from, st.to) = (from.to_string(), to.to_string());
    }
    ui.heading("Reports");
    ui.add_space(4.0);
    match db::stats(conn, &s.branch_id) {
        Ok(x) => {
            let warn = |bad: bool, colour| if bad { colour } else { theme::STATUS_GOOD };
            theme::stat_row(
                ui,
                &[
                    ("Sales today (KSh)", x.today_sales.to_string(), theme::ACCENT),
                    ("Profit today (KSh)", x.today_profit.to_string(), theme::STATUS_GOOD),
                    (
                        "Medicines below 10 units",
                        x.low_stock.to_string(),
                        warn(x.low_stock > 0, theme::STATUS_CRITICAL),
                    ),
                    (
                        "Batches expiring in 90 days",
                        x.expiring_soon.to_string(),
                        warn(x.expiring_soon > 0, theme::STATUS_SERIOUS),
                    ),
                    (
                        "Expired stock (KSh)",
                        x.expired_value.to_string(),
                        warn(x.expired_value.0 > 0, theme::STATUS_CRITICAL),
                    ),
                ],
            );
        }
        Err(e) => Notice::error(e).show(ui),
    }
    ui.add_space(12.0);
    theme::two_columns(ui, "reports", 250.0, st, picker, |ui, st| report(ui, conn, s, st));
}

/// The reports, grouped the way people look for them.
const GROUPS: [(&str, &[Report]); 3] = [
    (
        "Sales",
        &[
            Report::SalesByDay,
            Report::Sales,
            Report::SalesByMedicine,
            Report::SalesByPayment,
            Report::SalesByCashier,
            Report::SellersPerShift,
            Report::CollectedByCashier,
            Report::UnpaidOrders,
        ],
    ),
    ("Stock", &[Report::StockOnHand, Report::Reorder, Report::ExpiryRisk, Report::Expiry, Report::Deliveries]),
    ("Shifts & audit", &[Report::Shifts, Report::Audit]),
];

fn picker(ui: &mut egui::Ui, st: &mut ReportsState) {
    theme::panel(ui, |ui| {
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (i, (group, kinds)) in GROUPS.iter().enumerate() {
                ui.add_space(if i == 0 { 0.0 } else { 10.0 });
                ui.label(egui::RichText::new(group.to_uppercase()).size(12.0).strong().color(theme::TEXT_SECONDARY));
                ui.add_space(2.0);
                for &kind in *kinds {
                    if theme::tab(ui, kind.label(), st.kind == kind).clicked() {
                        st.kind = kind;
                        st.void_row = None;
                    }
                }
            }
        });
    });
}

/// "1 Oct 2026", or the text as typed if it isn't a date yet.
fn nice_date(text: &str) -> String {
    validate::parse_date("", text).map_or(text.to_string(), |d| d.format("%-d %b %Y").to_string())
}

fn report(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut ReportsState) {
    let key = (st.kind, st.from.clone(), st.to.clone());
    if st.loaded_for.as_ref() != Some(&key) {
        st.table = Some(load(conn, s, st));
        st.loaded_for = Some(key);
    }
    theme::panel(ui, |ui| {
        // The title, with Refresh and the save buttons on the right.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(st.kind.label()).size(22.0).strong().color(theme::TEXT_PRIMARY));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| toolbar(ui, conn, s, st));
        });
        ui.add_space(6.0);
        if !st.kind.uses_dates() {
            return theme::muted(ui, "As things stand right now (no dates needed).");
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            for period in Period::ALL {
                if theme::chip(ui, period.label(), st.period == period).clicked() {
                    st.period = period; // "Choose dates" starts from the range shown
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if st.period == Period::Custom {
                for (label, text) in [("From", &mut st.from), ("to", &mut st.to)] {
                    theme::muted(ui, label);
                    ui.add(egui::TextEdit::singleline(text).hint_text("01/10/2026").desired_width(130.0));
                }
            } else {
                theme::muted(ui, format!("{}  –  {}", nice_date(&st.from), nice_date(&st.to)));
            }
        });
    });
    ui.add_space(4.0);
    st.notice.show(ui);
    void_panel(ui, conn, s, st);

    // On the receipts report each row can be reprinted, and voided by a manager.
    let receipts = st.kind == Report::Sales;
    let can_void = s.can(Action::VoidSale);
    let (mut void_clicked, mut reprint) = (None, None);
    match &st.table {
        Some(Ok(table)) => {
            ui.add_space(4.0);
            theme::report_table(ui, "report_table", table, if receipts { 160.0 } else { 0.0 }, |ui, i| {
                if ui.button("Reprint").clicked() {
                    reprint = Some(table.keys[i].clone());
                }
                let open = table.rows[i].last().is_some_and(|status| status == "done");
                if can_void && open && theme::quiet_button(ui, "Void").clicked() {
                    void_clicked = Some(i);
                }
            });
        }
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

/// Right to left: Save PDF, Save CSV, Refresh, and Open folder after a save.
fn toolbar(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut ReportsState) {
    let ready = matches!(st.table, Some(Ok(_)));
    for (label, pdf) in [("Save PDF", true), ("Save CSV", false)] {
        let save = ui.add_enabled_ui(ready, |ui| ui.button(label)).inner;
        if save.on_hover_text("Saved in Documents\\MedTill reports").clicked() {
            st.notice = match export(conn, s, st, pdf) {
                Ok(path) => {
                    let msg = format!("Saved {}", path.display());
                    st.saved = Some(path);
                    Notice::success(msg)
                }
                Err(e) => Notice::error(e),
            };
        }
    }
    if theme::quiet_button(ui, "Refresh").clicked() {
        st.refresh();
    }
    if let Some(dir) = st.saved.as_ref().and_then(|p| p.parent())
        && theme::quiet_button(ui, "Open folder").clicked()
    {
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
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

/// Saves the report on screen as CSV or PDF in Documents\MedTill reports.
/// A PDF's verification code goes in the audit log, so a printed copy can be
/// checked later.
fn export(conn: &Connection, s: &Session, st: &ReportsState, pdf: bool) -> Result<PathBuf, String> {
    let Some(Ok(table)) = &st.table else { return Err("Nothing to save.".into()) };
    let name = file_name(st);
    let (ext, bytes, code) = match pdf {
        true => {
            let shop = db::branch_name(conn)?;
            let now = chrono::Local::now();
            let code = verification_code(&shop, &name, &now.to_rfc3339(), table);
            let period = match st.kind.uses_dates() {
                true => format!("{}  –  {}", nice_date(&st.from), nice_date(&st.to)),
                false => format!("As at {}", now.format("%-d %b %Y")),
            };
            let header = crate::pdf::Header {
                shop: &shop,
                title: st.kind.label(),
                period: &period,
                made_by: &format!("{} ({})", s.user.name, s.user.role.label()),
                made_at: &now.format("%-d %b %Y, %H:%M").to_string(),
                code: &code,
            };
            ("pdf", crate::pdf::report(&header, table), Some(code))
        }
        false => ("csv", to_csv(table).into_bytes(), None),
    };
    let dir = reports_dir();
    let path = dir.join(format!("{name}.{ext}"));
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&path, bytes))
        .map_err(|e| format!("Could not save the report (is it open in another program?): {e}"))?;
    if let Some(code) = code {
        db::audit(
            conn,
            s,
            "report",
            &format!("Saved PDF \"{name}\" ({} rows), verification code {code}", table.rows.len()),
        )?;
    }
    Ok(path)
}

/// A code unique to this PDF ("7F3A-91C2-0B4D-E8A6"), from its contents and
/// the moment it was made.
fn verification_code(shop: &str, name: &str, made_at: &str, table: &Table) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (shop, name, made_at, &table.rows).hash(&mut hasher);
    let n = hasher.finish();
    format!("{:04X}-{:04X}-{:04X}-{:04X}", n >> 48, (n >> 32) & 0xFFFF, (n >> 16) & 0xFFFF, n & 0xFFFF)
}

/// Documents\MedTill reports.
fn reports_dir() -> PathBuf {
    crate::storage::documents_dir().join("MedTill reports")
}

/// "MedTill report - Sales by day - This week (2026-10-12 to 2026-10-16)".
fn file_name(st: &ReportsState) -> String {
    let kind = st.kind.label();
    let name = match (st.kind.uses_dates(), st.period) {
        (false, _) => format!("{kind} - {}", chrono::Local::now().date_naive()),
        (true, Period::Custom) => format!("{kind} - {} to {}", st.from.trim(), st.to.trim()),
        (true, period) => format!("{kind} - {} ({} to {})", period.label(), st.from, st.to),
    };
    format!("MedTill report - {name}").replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "-")
}

fn to_csv(table: &Table) -> String {
    let cell = |text: &String| {
        if text.contains([',', '"', '\n']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text.clone() }
    };
    // The BOM tells Excel the file is UTF-8 (so "KSh" and "—" show correctly).
    let mut csv = String::from("\u{feff}");
    for row in std::iter::once(&table.headers).chain(&table.rows) {
        csv += &row.iter().map(cell).collect::<Vec<_>>().join(",");
        csv += "\r\n";
    }
    csv
}
