use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{Cents, Delivery, DeliveryItem, Product, Supplier};
use crate::ui::notify::Notice;
use crate::validate;

#[derive(Default)]
pub struct SuppliersState {
    name: String,
    contact_phone: String,
    contact_email: String,
    address: String,
    notes: String,
    supplier_status: Notice,
    editing_supplier_id: Option<i64>,

    selected_supplier: Option<i64>,
    invoice_number: String,
    delivery_notes: String,
    items: Vec<DeliveryItem>,
    line_product: Option<i64>,
    line_quantity: String,
    line_unit_cost: String,
    line_batch: String,
    line_expiry: String,
    delivery_status: Notice,
}

pub fn show(ui: &mut egui::Ui, conn: &mut Connection, state: &mut SuppliersState) {
    ui.heading("Suppliers");

    let suppliers = match db::list_suppliers(conn) {
        Ok(s) => s,
        Err(e) => {
            Notice::error(format!("Failed to load suppliers: {e}")).show(ui);
            return;
        }
    };

    let products = match db::list_products(conn) {
        Ok(p) => p,
        Err(e) => {
            Notice::error(format!("Failed to load products: {e}")).show(ui);
            return;
        }
    };

    let supplier_heading = if state.editing_supplier_id.is_some() { "Edit supplier" } else { "Add supplier" };
    let force_open = if state.editing_supplier_id.is_some() { Some(true) } else { None };
    egui::CollapsingHeader::new(supplier_heading)
        .id_salt("supplier_form")
        .open(force_open)
        .show(ui, |ui| {
            render_add_supplier_form(ui, conn, state);
        });

    ui.separator();
    ui.heading("Supplier list");
    if suppliers.is_empty() {
        ui.label("No suppliers yet — add one above.");
    } else {
        egui::Grid::new("supplier_table").striped(true).num_columns(4).show(ui, |ui| {
            for supplier in &suppliers {
                ui.label(&supplier.name);
                ui.label(&supplier.contact_phone);
                ui.label(&supplier.contact_email);
                if ui.button("Edit").clicked() {
                    load_supplier_into_form(state, supplier);
                }
                ui.end_row();
            }
        });
    }

    ui.separator();
    ui.collapsing("Log a delivery", |ui| {
        render_delivery_form(ui, conn, state, &suppliers, &products);
    });
}

fn render_add_supplier_form(ui: &mut egui::Ui, conn: &mut Connection, state: &mut SuppliersState) {
    egui::Grid::new("add_supplier_grid").num_columns(2).show(ui, |ui| {
        ui.label("Name");
        ui.add(egui::TextEdit::singleline(&mut state.name).hint_text("e.g. Cosmos Pharmaceuticals"));
        ui.end_row();

        ui.label("Phone");
        ui.add(egui::TextEdit::singleline(&mut state.contact_phone).hint_text("e.g. 0712 345 678"));
        ui.end_row();

        ui.label("Email");
        ui.add(egui::TextEdit::singleline(&mut state.contact_email).hint_text("e.g. orders@supplier.com"));
        ui.end_row();

        ui.label("Address");
        ui.add(egui::TextEdit::singleline(&mut state.address).hint_text("optional"));
        ui.end_row();

        ui.label("Notes");
        ui.add(egui::TextEdit::singleline(&mut state.notes).hint_text("optional"));
        ui.end_row();
    });

    let button_label = if state.editing_supplier_id.is_some() { "Save changes" } else { "Add supplier" };

    ui.horizontal(|ui| {
        if ui.button(button_label).clicked() {
            match build_supplier(state) {
                Ok(supplier) => {
                    let result = if state.editing_supplier_id.is_some() {
                        db::update_supplier(conn, &supplier)
                    } else {
                        db::add_supplier(conn, &supplier).map(|_| ())
                    };

                    match result {
                        Ok(()) => {
                            let verb = if state.editing_supplier_id.is_some() { "Updated" } else { "Added" };
                            state.supplier_status = Notice::success(format!("{verb} \"{}\".", supplier.name));
                            clear_supplier_form(state);
                        }
                        Err(e) => state.supplier_status = Notice::error(format!("Database error: {e}")),
                    }
                }
                Err(msg) => state.supplier_status = Notice::error(msg),
            }
        }

        if state.editing_supplier_id.is_some() && ui.button("Cancel").clicked() {
            clear_supplier_form(state);
            state.supplier_status = Notice::None;
        }
    });

    state.supplier_status.show(ui);
}

fn clear_supplier_form(state: &mut SuppliersState) {
    state.name.clear();
    state.contact_phone.clear();
    state.contact_email.clear();
    state.address.clear();
    state.notes.clear();
    state.editing_supplier_id = None;
}

fn load_supplier_into_form(state: &mut SuppliersState, supplier: &Supplier) {
    state.name = supplier.name.clone();
    state.contact_phone = supplier.contact_phone.clone();
    state.contact_email = supplier.contact_email.clone();
    state.address = supplier.address.clone();
    state.notes = supplier.notes.clone();
    state.editing_supplier_id = Some(supplier.id);
}

fn build_supplier(state: &SuppliersState) -> Result<Supplier, String> {
    let name = validate::parse_name("Supplier name", &state.name)?;
    let contact_phone = validate::parse_phone(&state.contact_phone)?;
    let contact_email = validate::parse_email(&state.contact_email)?;

    Ok(Supplier {
        id: state.editing_supplier_id.unwrap_or(0),
        name,
        contact_phone,
        contact_email,
        address: state.address.trim().to_string(),
        notes: state.notes.trim().to_string(),
    })
}

fn render_delivery_form(
    ui: &mut egui::Ui,
    conn: &mut Connection,
    state: &mut SuppliersState,
    suppliers: &[Supplier],
    products: &[Product],
) {
    if suppliers.is_empty() {
        ui.label("Add a supplier first before logging a delivery.");
        return;
    }
    if products.is_empty() {
        ui.label("Add at least one product in Inventory first before logging a delivery.");
        return;
    }

    let selected_supplier_name = state
        .selected_supplier
        .and_then(|id| suppliers.iter().find(|s| s.id == id))
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "Choose a supplier".to_string());

    egui::ComboBox::from_label("Supplier")
        .selected_text(selected_supplier_name)
        .show_ui(ui, |ui| {
            for supplier in suppliers {
                ui.selectable_value(&mut state.selected_supplier, Some(supplier.id), &supplier.name);
            }
        });

    ui.horizontal(|ui| {
        ui.label("Invoice #");
        ui.add(egui::TextEdit::singleline(&mut state.invoice_number).hint_text("e.g. INV-2026-0091"));
    });
    ui.horizontal(|ui| {
        ui.label("Notes");
        ui.add(egui::TextEdit::singleline(&mut state.delivery_notes).hint_text("optional"));
    });

    ui.separator();
    ui.label("Add a product line:");

    let selected_product_name = state
        .line_product
        .and_then(|id| products.iter().find(|p| p.id == id))
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "Choose a product".to_string());

    egui::ComboBox::from_label("Product")
        .selected_text(selected_product_name)
        .show_ui(ui, |ui| {
            for product in products {
                ui.selectable_value(&mut state.line_product, Some(product.id), &product.name);
            }
        });

    egui::Grid::new("delivery_line_grid").num_columns(2).show(ui, |ui| {
        ui.label("Quantity");
        ui.add(egui::TextEdit::singleline(&mut state.line_quantity).hint_text("e.g. 50"));
        ui.end_row();

        ui.label("Unit cost");
        ui.add(egui::TextEdit::singleline(&mut state.line_unit_cost).hint_text("e.g. 120.00"));
        ui.end_row();

        ui.label("Batch number");
        ui.add(egui::TextEdit::singleline(&mut state.line_batch).hint_text("e.g. BN-2026-014"));
        ui.end_row();

        ui.label("Expiry date");
        ui.add(
            egui::TextEdit::singleline(&mut state.line_expiry)
                .hint_text("YYYY-MM-DD, e.g. 2027-06-30"),
        );
        ui.end_row();
    });

    if ui.button("Add line to delivery").clicked() {
        match build_delivery_item(state) {
            Ok(item) => {
                state.items.push(item);
                state.line_product = None;
                state.line_quantity.clear();
                state.line_unit_cost.clear();
                state.line_batch.clear();
                state.line_expiry.clear();
                state.delivery_status = Notice::None;
            }
            Err(msg) => state.delivery_status = Notice::error(msg),
        }
    }

    ui.separator();

    if state.items.is_empty() {
        ui.label("No lines added yet.");
    } else {
        let mut remove_index: Option<usize> = None;
        let mut total = Cents(0);

        for (index, item) in state.items.iter().enumerate() {
            let product_name = products
                .iter()
                .find(|p| p.id == item.product_id)
                .map(|p| p.name.as_str())
                .unwrap_or("Unknown product");

            total = total + item.line_cost();

            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} x{} @ {:.2} (batch {}, exp {})",
                    product_name,
                    item.quantity,
                    item.unit_cost.as_major(),
                    item.batch_number,
                    item.expiry_date
                ));
                if ui.button("Remove").clicked() {
                    remove_index = Some(index);
                }
            });
        }

        if let Some(index) = remove_index {
            state.items.remove(index);
        }

        ui.strong(format!("Total cost: {:.2}", total.as_major()));
    }

    if ui.button("Record delivery").clicked() {
        if state.selected_supplier.is_none() {
            state.delivery_status = Notice::error("Choose a supplier first.".to_string());
        } else if state.items.is_empty() {
            state.delivery_status = Notice::error("Add at least one product line first.".to_string());
        } else {
            match record_delivery(conn, state) {
                Ok(delivery_id) => {
                    state.delivery_status = Notice::success(format!(
                        "Delivery #{delivery_id} recorded and stock updated."
                    ));
                    state.items.clear();
                    state.invoice_number.clear();
                    state.delivery_notes.clear();
                    state.selected_supplier = None;
                }
                Err(e) => {
                    state.delivery_status = Notice::error(format!("Failed to record delivery: {e}"))
                }
            }
        }
    }

    state.delivery_status.show(ui);
}

fn build_delivery_item(state: &SuppliersState) -> Result<DeliveryItem, String> {
    let product_id = state.line_product.ok_or_else(|| "Choose a product first.".to_string())?;

    let quantity = validate::parse_positive_quantity("Quantity", &state.line_quantity)?;
    let unit_cost = validate::parse_price("Unit cost", &state.line_unit_cost)?;

    let batch_number = state.line_batch.trim().to_string();
    if batch_number.is_empty() {
        return Err("Batch number is required.".to_string());
    }

    let today = chrono::Local::now().date_naive();
    let expiry_date = validate::parse_expiry_date(&state.line_expiry, today, false)?;

    Ok(DeliveryItem {
        product_id,
        quantity,
        unit_cost: Cents::from_major(unit_cost),
        batch_number,
        expiry_date,
    })
}

fn record_delivery(conn: &mut Connection, state: &SuppliersState) -> rusqlite::Result<i64> {
    let delivery = Delivery {
        id: 0,
        supplier_id: state.selected_supplier.unwrap_or(0),
        delivery_date: chrono::Local::now().date_naive(),
        invoice_number: state.invoice_number.trim().to_string(),
        notes: state.delivery_notes.trim().to_string(),
        items: state.items.clone(),
    };

    db::record_delivery(conn, &delivery)
}