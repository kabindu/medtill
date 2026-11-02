//! Medicines: the catalogue. Stock is not edited here — it is checked in
//! under Stock, batch by batch.

use chrono::NaiveDate;
use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{Product, Session};
use crate::scanner::Found;
use crate::ui::scan::{self, BoxScan};
use crate::ui::theme::{self, Notice, RowClick};
use crate::validate;

const FORM_WIDTH: f32 = 470.0;

#[derive(Default)]
pub struct ProductsState {
    /// Id of the medicine being edited (empty: adding a new one).
    editing: String,
    /// Kept as it was when editing an older medicine that has one.
    barcode: Option<String>,
    code: String,
    scan: BoxScan,
    /// What was read off the box; the reader learns from it when the medicine is saved.
    scan_found: Option<Found>,
    name: String,
    category: String,
    price: String,
    search: String,
    confirm_delete: Option<String>,
    notice: Notice,
}

pub fn show(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut ProductsState) {
    ui.heading("Medicines");
    match db::list_products(conn, &s.branch_id) {
        Ok(products) => theme::two_columns(
            ui,
            "medicines",
            FORM_WIDTH,
            st,
            |ui, st| medicine_form(ui, conn, s, st, &products),
            |ui, st| medicine_list(ui, conn, s, st, &products),
        ),
        Err(e) => Notice::error(e).show(ui),
    }
}

// ---------- Medicines ----------

fn medicine_form(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut ProductsState, products: &[Product]) {
    let editing = !st.editing.is_empty();
    theme::section_title(ui, if editing { "Edit medicine" } else { "Add medicine" });
    theme::panel(ui, |ui| {
        theme::step(ui, 1, "Photograph the box (optional)");
        if let Some(found) = scan::box_scan(ui, conn, &mut st.scan, products) {
            match found.product_id.as_deref().and_then(|id| products.iter().find(|p| p.id == id)) {
                Some(p) => {
                    load(st, p);
                    st.notice = Notice::success(format!("{} is already listed — editing it.", p.name));
                }
                None => {
                    (st.name, st.category) = (found.name.clone(), found.category.clone());
                    st.notice = Notice::success("Read from the box: check the name and category, add the price, save.");
                }
            }
            st.scan_found = Some(found);
        }
        theme::step(ui, 2, "Details");
        theme::form(ui, "medicine_form", |ui| {
            theme::field(ui, "Name", &mut st.name, "e.g. Panadol 500mg");
            theme::field(ui, "Code", &mut st.code, "typed at the till, e.g. 1001 (empty = next number)");
            theme::field(ui, "Category", &mut st.category, "e.g. Painkiller");
            theme::field(ui, "Price (KSh)", &mut st.price, "selling price, e.g. 250.00");
        });
        theme::actions(ui, |ui| {
            if theme::primary_button(ui, if editing { "Save changes" } else { "Add medicine" }).clicked() {
                st.notice = save_medicine(conn, s, st).into();
                if st.notice.is_success() {
                    clear(st);
                }
            }
            if editing && theme::quiet_button(ui, "Cancel").clicked() {
                clear(st);
                st.notice = Notice::None;
            }
        });
        st.notice.show(ui);
        theme::muted(ui, "Batches, expiry dates and stock are added under Stock.");
        theme::muted(ui, "Stock level: red below 10 units, orange below 50, green from 50.");
    });
}

fn save_medicine(conn: &Connection, s: &Session, st: &ProductsState) -> Result<String, String> {
    let p = Product {
        id: st.editing.clone(),
        name: validate::parse_name("Name", &st.name)?,
        category: st.category.trim().to_string(),
        barcode: st.barcode.clone(),
        code: validate::parse_code(&st.code)?.unwrap_or_default(),
        price: validate::parse_price("Price", &st.price)?,
        ..Product::default()
    };
    let id = db::save_product(conn, s, &p)?;
    if let Some(found) = &st.scan_found
        && let Err(e) = scan::learn(conn, found, &id, &p.name, &p.category)
    {
        return Ok(format!("Saved {} ({e}).", p.name));
    }
    Ok(format!("Saved {}.", p.name))
}

fn load(st: &mut ProductsState, p: &Product) {
    st.editing = p.id.clone();
    st.barcode = p.barcode.clone();
    st.code = p.code.clone();
    st.name = p.name.clone();
    st.category = p.category.clone();
    st.price = p.price.to_string().replace(',', "");
}

fn clear(st: &mut ProductsState) {
    load(st, &Product::default());
    st.price.clear();
    st.scan_found = None;
}

fn medicine_list(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut ProductsState, products: &[Product]) {
    theme::section_title(ui, &format!("Medicines ({})", products.len()));
    theme::search_box(ui, &mut st.search, "Search by name, code or category");
    let today = chrono::Local::now().date_naive();
    let query = st.search.clone();
    if products.is_empty() {
        return theme::muted(ui, "No medicines yet. Add the first one on the left.");
    }
    let shown: Vec<&Product> = products.iter().filter(|p| p.matches(&query)).collect();
    let headers = ["Code", "Medicine", "Price", "In stock", "Stock level", "Expiry", ""];
    let widths = [70.0, 0.0, 90.0, 80.0, 150.0, 120.0, 170.0];
    let mut clicked = None;
    theme::long_list(ui, "medicine_list", &headers, &widths, shown.len(), |ui, i, w| {
        let (p, h) = (shown[i], theme::ROW_HEIGHT);
        theme::cell(ui, w[0], h, |ui| theme::cell_text(ui, p.code.as_str()));
        theme::cell(ui, w[1], h, |ui| theme::cell_text(ui, egui::RichText::new(&p.name).strong()));
        theme::cell(ui, w[2], h, |ui| theme::number_cell(ui, p.price.to_string()));
        theme::cell(ui, w[3], h, |ui| theme::number_cell(ui, (p.stock + p.held).to_string()));
        theme::cell(ui, w[4], h, |ui| theme::stock_level(ui, p.stock + p.held));
        theme::cell(ui, w[5], h, |ui| expiry_note(ui, p, today));
        theme::cell(ui, w[6], h, |ui| match theme::edit_delete(ui, &p.id, &mut st.confirm_delete) {
            RowClick::None => {}
            click => clicked = Some((i, click)),
        });
    });
    match clicked {
        Some((i, RowClick::Edit)) => load(st, shown[i]),
        Some((i, RowClick::Delete)) => {
            let p = shown[i];
            st.notice =
                db::delete_product(conn, s, p).map(|()| format!("Deleted {}.", p.name)).map_err(String::from).into();
            if st.editing == p.id {
                clear(st);
            }
        }
        _ => {}
    }
}

/// Units past their expiry (red), or "expiring soon" within 90 days (orange).
fn expiry_note(ui: &mut egui::Ui, p: &Product, today: NaiveDate) {
    if p.expired > 0 {
        ui.colored_label(theme::STATUS_CRITICAL, format!("{} expired", p.expired));
    } else if p.expires_within(today, 90) {
        ui.colored_label(theme::STATUS_SERIOUS, "Expiring soon");
    }
}
