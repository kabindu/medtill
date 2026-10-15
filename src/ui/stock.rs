//! Stock: check stock in as batches (find each medicine by scanning, its
//! code or its name, and add a new medicine on the spot), see every batch on
//! the shelf (earliest expiry first), top up batches on the shelf (a list of
//! lines saved together with "Update stock"), write off and count stock.

use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{Action, Batch, Cents, Delivery, DeliveryLine, Product, Session};
use crate::ui::scan::{self, CameraScan};
use crate::ui::theme::{self, Notice};
use crate::validate;

const FORM_WIDTH: f32 = 520.0;
const REASONS: [&str; 5] = ["New stock arrived", "Expired", "Damaged", "Returned to supplier", "Stock count"];
const NEW_STOCK: usize = 0;
const EXPIRED: usize = 1;
const STOCK_COUNT: usize = 4;

#[derive(Default)]
pub struct StockState {
    delivery: Delivery,
    // Finding the medicine for the next line.
    query: String,
    camera: CameraScan,
    product: Option<String>, // id
    new_medicine: Option<Product>,
    new_price: String,
    // The line being typed.
    qty: String,
    cost: String,
    batch_no: String,
    expiry: String,
    focus_qty: bool,
    notice: Notice,
    // The shelf.
    search: String,
    adjusting: Option<String>, // batch id
    reason: usize,
    adjust_qty: String,
    adjust_expiry: String,
    focus_adjust: bool,
    top_ups: Delivery,
    shelf_notice: Notice,
}

pub fn show(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut StockState) {
    ui.heading("Stock");
    let loaded = (|| -> db::Result<_> {
        Ok((
            db::list_products(conn, &s.branch_id)?,
            db::list_batches(conn, &s.branch_id)?,
            [db::recent_check_ins(conn, &s.branch_id, 8)?, db::quick_picks(conn, &s.branch_id, 8)?.0],
        ))
    })();
    let (products, batches, picks) = match loaded {
        Ok(data) => data,
        Err(e) => return Notice::error(e).show(ui),
    };
    theme::two_columns(
        ui,
        "stock",
        FORM_WIDTH,
        st,
        |ui, st| check_in_form(ui, conn, s, st, &products, &picks),
        |ui, st| shelf(ui, conn, s, st, &batches),
    );
}

// ---------- Receive ----------

fn check_in_form(
    ui: &mut egui::Ui,
    conn: &Connection,
    s: &Session,
    st: &mut StockState,
    products: &[Product],
    picks: &[Vec<String>; 2],
) {
    theme::section_title(ui, "Check in stock");
    theme::panel(ui, |ui| {
        theme::step(ui, 1, "Find the medicine");
        find_medicine(ui, conn, s, st, products, picks);

        theme::step(ui, 2, "Quantity, cost, batch and expiry");
        let mut add_line = false;
        theme::form(ui, "receive_line", |ui| {
            let qty = theme::field(ui, "Quantity", &mut st.qty, "units received, e.g. 50");
            if std::mem::take(&mut st.focus_qty) {
                qty.request_focus();
            }
            theme::field(ui, "Unit cost (KSh)", &mut st.cost, "what you paid per unit, e.g. 120.00");
            theme::field(ui, "Batch number", &mut st.batch_no, "printed on the box, e.g. BN-2026-014");
            let expiry = theme::field(ui, "Expiry", &mut st.expiry, "as on the box: 06/2027 or 2027-06-30");
            add_line = theme::enter_pressed(&expiry);
        });
        ui.add_space(6.0);
        if ui.button("+ Add line").clicked() || add_line {
            match build_line(st, products) {
                Ok(line) => {
                    st.delivery.lines.push(line);
                    (st.product, st.qty, st.cost, st.batch_no, st.expiry) = Default::default();
                    st.notice = Notice::None;
                }
                Err(e) => st.notice = Notice::error(e),
            }
        }

        theme::step(ui, 3, &format!("Lines to check in ({})", st.delivery.lines.len()));
        if st.delivery.lines.is_empty() {
            theme::muted(ui, "No lines added yet.");
        } else {
            let mut remove = None;
            theme::table(ui, "receive_lines", &["Medicine", "Batch", "Expiry", "Qty", "Cost (KSh)", ""], |ui| {
                for (i, line) in st.delivery.lines.iter().enumerate() {
                    ui.label(&line.product_name);
                    ui.label(&line.batch_no);
                    ui.label(line.expiry.to_string());
                    theme::number_cell(ui, line.qty.to_string());
                    theme::number_cell(ui, (line.cost * line.qty).to_string());
                    if theme::quiet_button(ui, "Remove").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = remove {
                st.delivery.lines.remove(i);
            }
            let total: Cents = st.delivery.lines.iter().map(|l| l.cost * l.qty).sum();
            theme::big_total(ui, format!("Total cost  KSh {total}"), theme::TEXT_PRIMARY);
        }
        theme::form(ui, "check_in_reference", |ui| {
            theme::field(ui, "Reference #", &mut st.delivery.invoice_no, "optional, e.g. invoice or note number");
        });
        if theme::big_button(ui, "Check in stock").clicked() {
            st.notice = record(conn, s, st).into();
        }
        st.notice.show(ui);
    });
}

/// Step 1: one box for everything (a scan, an exact code, or a name with a
/// single match picks the medicine on Enter), one-tap Recent and Popular
/// picks, the closest matches as buttons, and "+ New medicine" for one that
/// isn't listed yet.
fn find_medicine(
    ui: &mut egui::Ui,
    conn: &Connection,
    s: &Session,
    st: &mut StockState,
    products: &[Product],
    picks: &[Vec<String>; 2],
) {
    if let Some(p) = st.product.as_ref().and_then(|id| products.iter().find(|p| &p.id == id)) {
        let mut change = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(p.label()).strong().size(18.0).color(theme::ACCENT_STRONG));
            theme::muted(ui, format!("{} on the shelf", p.stock + p.held));
            change = theme::quiet_button(ui, "Change").clicked();
        });
        if change {
            st.product = None;
        }
        return;
    }
    let hint = "Scan, or type barcode / code / name, then Enter";
    let autofocus = st.new_medicine.is_none();
    if let Some(entered) = scan::barcode_box(ui, &mut st.query, &mut st.camera, "receive_find", hint, autofocus) {
        let hits: Vec<&Product> = products.iter().filter(|p| p.matches(&entered)).collect();
        match (products.iter().find(|p| p.is_exactly(&entered)), hits.as_slice()) {
            (Some(p), _) | (None, &[p]) => choose(st, p),
            (None, []) => {}
            (None, many) => st.notice = Notice::error(format!("{} medicines match — pick one below.", many.len())),
        }
    }
    let query = st.query.trim().to_string();
    if let Some(draft) = &mut st.new_medicine {
        let (mut saved, mut cancel) = (None, false);
        theme::panel(ui, |ui| {
            theme::subheading(ui, "New medicine");
            theme::form(ui, "receive_new_medicine", |ui| {
                theme::field(ui, "Name", &mut draft.name, "e.g. Panadol 500mg");
                theme::field(ui, "Selling price (KSh)", &mut st.new_price, "e.g. 250.00");
                theme::field(ui, "Code", &mut draft.code, "leave empty for the next number");
                theme::field(ui, "Barcode", draft.barcode.get_or_insert_default(), "optional");
            });
            theme::actions(ui, |ui| {
                if theme::primary_button(ui, "Save and use it").clicked() {
                    saved = Some(save_new_medicine(conn, s, draft, &st.new_price));
                }
                cancel = theme::quiet_button(ui, "Cancel").clicked();
            });
        });
        match saved {
            Some(Ok(id)) => {
                (st.new_medicine, st.query, st.new_price) = Default::default();
                st.product = Some(id);
                st.focus_qty = true;
                st.notice = Notice::success("Medicine added. Now enter the quantity.");
            }
            Some(Err(e)) => st.notice = Notice::error(e),
            None if cancel => st.new_medicine = None,
            None => {}
        }
        return;
    }
    if query.is_empty() {
        for (label, ids) in [("Recently checked in", &picks[0]), ("Popular", &picks[1])] {
            let found: Vec<&Product> = ids.iter().filter_map(|id| products.iter().find(|p| &p.id == id)).collect();
            if found.is_empty() {
                continue;
            }
            ui.horizontal_wrapped(|ui| {
                ui.add_sized([150.0, 24.0], egui::Label::new(egui::RichText::new(label).color(theme::TEXT_SECONDARY)));
                for p in found {
                    if theme::chip(ui, &p.name, false).on_hover_text(p.label()).clicked() {
                        choose(st, p);
                    }
                }
            });
        }
        if ui.button("+ New medicine").clicked() {
            st.new_medicine = Some(Product::default());
        }
        return;
    }
    let hits: Vec<&Product> = products.iter().filter(|p| p.matches(&query)).take(8).collect();
    ui.horizontal_wrapped(|ui| {
        for p in &hits {
            if theme::chip(ui, p.label(), false).clicked() {
                choose(st, p);
            }
        }
    });
    if hits.is_empty() {
        theme::muted(ui, format!("No medicine matches \"{query}\"."));
    }
    if ui.button(format!("+ Add \"{query}\" as a new medicine")).clicked() {
        let barcode = query.chars().all(|c| c.is_ascii_digit()) && query.len() >= 8;
        st.new_medicine = Some(Product {
            name: if barcode { String::new() } else { query.clone() },
            barcode: barcode.then_some(query),
            ..Product::default()
        });
    }
}

fn choose(st: &mut StockState, p: &Product) {
    st.product = Some(p.id.clone());
    st.query.clear();
    // Pre-fill from the last check-in of this medicine; both can be changed.
    st.cost = p.last_cost.filter(|c| c.0 > 0).map(|c| c.to_string().replace(',', "")).unwrap_or_default();
    st.qty = p.last_qty.map(|q| q.to_string()).unwrap_or_default();
    st.focus_qty = true;
    st.notice = Notice::None;
}

fn save_new_medicine(conn: &Connection, s: &Session, draft: &Product, price: &str) -> Result<String, String> {
    let medicine = Product {
        name: validate::parse_name("Name", &draft.name)?,
        code: validate::parse_code(&draft.code)?.unwrap_or_default(),
        barcode: validate::parse_barcode(draft.barcode.as_deref().unwrap_or_default())?,
        price: validate::parse_price("Selling price", price)?,
        ..Product::default()
    };
    Ok(db::save_product(conn, s, &medicine)?)
}

fn record(conn: &Connection, s: &Session, st: &mut StockState) -> Result<String, String> {
    db::receive(conn, s, &st.delivery)?;
    let lines = std::mem::take(&mut st.delivery).lines.len();
    Ok(format!("Checked in: {lines} lines are now on the shelf."))
}

fn build_line(st: &StockState, products: &[Product]) -> Result<DeliveryLine, String> {
    let product = st.product.as_ref().and_then(|id| products.iter().find(|p| &p.id == id));
    let Some(product) = product else { return Err("Find the medicine first (step 1).".into()) };
    let batch_no = st.batch_no.trim().to_string();
    if batch_no.is_empty() {
        return Err("Batch number is required (it's printed on the box).".into());
    }
    Ok(DeliveryLine {
        product_id: product.id.clone(),
        product_name: product.name.clone(),
        batch_no,
        expiry: validate::parse_expiry(&st.expiry, chrono::Local::now().date_naive())?,
        qty: validate::parse_positive_quantity("Quantity", &st.qty)?,
        cost: validate::parse_money("Unit cost", &st.cost)?,
        batch_id: None,
    })
}

// ---------- The shelf ----------

fn shelf(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut StockState, batches: &[Batch]) {
    theme::section_title(ui, "On the shelf — earliest expiry first");
    if let Some(b) = st.adjusting.as_ref().and_then(|id| batches.iter().find(|b| &b.id == id)) {
        adjust_panel(ui, conn, s, st, b);
    }
    top_up_list(ui, conn, s, st);
    st.shelf_notice.show(ui);
    theme::search_box(ui, &mut st.search, "Search by medicine or batch");
    if batches.is_empty() {
        return theme::muted(ui, "Nothing on the shelf yet. Check stock in on the left.");
    }
    let today = chrono::Local::now().date_naive();
    let can_adjust = s.can(Action::AdjustStock);
    let shown: Vec<&Batch> =
        batches.iter().filter(|b| theme::matches(&st.search, &format!("{} {}", b.product_name, b.batch_no))).collect();
    // Units waiting in the "Stock to add" list, shown as "12 (+5)".
    let pending = |b: &Batch| -> i64 {
        st.top_ups.lines.iter().filter(|l| l.batch_id.as_deref() == Some(b.id.as_str())).map(|l| l.qty).sum()
    };
    let headers = ["Medicine", "Batch", "Expiry", "Days left", "Qty", "Value (KSh)", ""];
    let mut clicked = None;
    theme::long_list(ui, "shelf", &headers, &[0.0, 110.0, 100.0, 80.0, 80.0, 110.0, 90.0], shown.len(), |ui, i, w| {
        let (b, h) = (shown[i], theme::ROW_HEIGHT);
        let days = (b.expiry - today).num_days();
        let colour = match days {
            ..0 => theme::STATUS_CRITICAL,
            0..=90 => theme::STATUS_SERIOUS,
            _ => theme::TEXT_PRIMARY,
        };
        theme::cell(ui, w[0], h, |ui| theme::cell_text(ui, egui::RichText::new(&b.product_name).strong()));
        theme::cell(ui, w[1], h, |ui| theme::cell_text(ui, batch_no(b)));
        theme::cell(ui, w[2], h, |ui| {
            ui.colored_label(colour, b.expiry.to_string());
        });
        theme::cell(ui, w[3], h, |ui| {
            theme::number_cell(ui, if days < 0 { "expired".into() } else { days.to_string() })
        });
        theme::cell(ui, w[4], h, |ui| match pending(b) {
            0 => theme::number_cell(ui, b.qty.to_string()),
            more => theme::number_cell(ui, format!("{} (+{more})", b.qty)),
        });
        theme::cell(ui, w[5], h, |ui| theme::number_cell(ui, (b.cost * b.qty).to_string()));
        theme::cell(ui, w[6], h, |ui| {
            if ui.button("Adjust").on_hover_text("Add new stock, write off or count").clicked() {
                clicked = Some((b.id.clone(), days));
            }
        });
    });
    if let Some((id, days)) = clicked {
        st.adjusting = Some(id);
        st.reason = if days < 0 && can_adjust { EXPIRED } else { NEW_STOCK };
        (st.adjust_qty, st.adjust_expiry, st.focus_adjust) = (String::new(), String::new(), true);
        st.shelf_notice = Notice::None;
    }
}

fn batch_no(b: &Batch) -> &str {
    if b.batch_no.is_empty() { "—" } else { &b.batch_no }
}

/// Everyone who checks stock in can add new units to a batch on the shelf
/// (just the quantity, plus the expiry if it is different) as a line in the
/// "Stock to add" list; writing off and counting need Adjust stock.
fn adjust_panel(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut StockState, b: &Batch) {
    let can_adjust = s.can(Action::AdjustStock);
    let (mut apply, mut cancel) = (false, false);
    theme::panel(ui, |ui| {
        theme::subheading(ui, &format!("{} — batch {} ({} on the shelf)", b.product_name, batch_no(b), b.qty));
        ui.horizontal(|ui| {
            if can_adjust {
                egui::ComboBox::from_id_salt("adjust_reason").selected_text(REASONS[st.reason]).show_ui(ui, |ui| {
                    for (i, reason) in REASONS.iter().enumerate() {
                        ui.selectable_value(&mut st.reason, i, *reason);
                    }
                });
            }
            let hint = match st.reason {
                NEW_STOCK => "units that arrived",
                STOCK_COUNT => "units counted on the shelf",
                _ => "units to remove",
            };
            let units = ui.add(egui::TextEdit::singleline(&mut st.adjust_qty).hint_text(hint).desired_width(170.0));
            if std::mem::take(&mut st.focus_adjust) {
                units.request_focus();
            }
            apply = theme::enter_pressed(&units);
            if st.reason == NEW_STOCK {
                let hint = format!("expiry, if not {}", b.expiry.format("%m/%Y"));
                let expiry =
                    ui.add(egui::TextEdit::singleline(&mut st.adjust_expiry).hint_text(hint).desired_width(170.0));
                apply |= theme::enter_pressed(&expiry);
            }
        });
        theme::actions(ui, |ui| {
            apply |= match st.reason {
                NEW_STOCK => theme::primary_button(ui, "+ Add line"),
                STOCK_COUNT => theme::primary_button(ui, "Save count"),
                _ => theme::danger_button(ui, "Write off"),
            }
            .clicked();
            cancel = theme::quiet_button(ui, "Cancel").clicked();
        });
    });
    if cancel {
        st.adjusting = None;
        st.shelf_notice = Notice::None;
    } else if apply {
        st.shelf_notice = adjust(conn, s, st, b).into();
        if st.shelf_notice.is_success() {
            st.adjusting = None;
        }
    }
}

fn adjust(conn: &Connection, s: &Session, st: &mut StockState, b: &Batch) -> Result<String, String> {
    match st.reason {
        NEW_STOCK => {
            let qty = validate::parse_positive_quantity("Units", &st.adjust_qty)?;
            let today = chrono::Local::now().date_naive();
            let expiry = match st.adjust_expiry.trim() {
                "" if b.expiry < today => return Err("This batch has expired — type the new expiry.".into()),
                "" => b.expiry,
                typed => validate::parse_expiry(typed, today)?,
            };
            // Same expiry: more of this batch. A new expiry: a new batch.
            let same = expiry == b.expiry;
            let line = DeliveryLine {
                batch_id: same.then(|| b.id.clone()),
                product_id: b.product_id.clone(),
                product_name: b.product_name.clone(),
                batch_no: if same { b.batch_no.clone() } else { String::new() },
                expiry,
                qty,
                cost: b.cost,
            };
            let lines = &mut st.top_ups.lines;
            match lines
                .iter_mut()
                .find(|l| l.batch_id == line.batch_id && l.product_id == line.product_id && l.expiry == expiry)
            {
                Some(l) => l.qty += qty,
                None => lines.push(line),
            }
            Ok(format!("{} +{qty} is on the list. Press Update stock when you're done.", b.product_name))
        }
        STOCK_COUNT => {
            let counted = validate::parse_quantity("Counted", &st.adjust_qty)?;
            db::count_stock(conn, s, b, counted)?;
            Ok(format!("{} batch {} set to {counted}.", b.product_name, batch_no(b)))
        }
        _ => {
            let qty = validate::parse_positive_quantity("Quantity", &st.adjust_qty)?;
            db::write_off(conn, s, b, qty, REASONS[st.reason])?;
            Ok(format!("Removed {qty} from {} batch {}.", b.product_name, batch_no(b)))
        }
    }
}

/// The lines added with Adjust → New stock arrived, saved together.
fn top_up_list(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut StockState) {
    if st.top_ups.lines.is_empty() {
        return;
    }
    let units: i64 = st.top_ups.lines.iter().map(|l| l.qty).sum();
    let (mut remove, mut save, mut clear) = (None, false, false);
    theme::panel(ui, |ui| {
        theme::subheading(ui, &format!("Stock to add — {} lines, {units} units", st.top_ups.lines.len()));
        theme::table(ui, "top_ups", &["Medicine", "Batch", "Expiry", "Units", ""], |ui| {
            for (i, l) in st.top_ups.lines.iter().enumerate() {
                ui.label(&l.product_name);
                ui.label(if l.batch_id.is_some() { l.batch_no.as_str() } else { "new batch" });
                ui.label(l.expiry.to_string());
                theme::number_cell(ui, format!("+{}", l.qty));
                if theme::quiet_button(ui, "Remove").clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
        theme::actions(ui, |ui| {
            save = theme::primary_button(ui, "Update stock").clicked();
            clear = theme::quiet_button(ui, "Clear the list").clicked();
        });
    });
    ui.add_space(8.0);
    if let Some(i) = remove {
        st.top_ups.lines.remove(i);
    } else if clear {
        st.top_ups = Delivery::default();
    } else if save {
        st.shelf_notice = match db::receive(conn, s, &st.top_ups) {
            Ok(()) => {
                let lines = std::mem::take(&mut st.top_ups).lines.len();
                Notice::success(format!("Stock updated: {units} units added in {lines} lines."))
            }
            Err(e) => Notice::error(e),
        };
    }
}
