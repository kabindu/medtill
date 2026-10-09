use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{Cents, Delivery, DeliveryItem, Product, Supplier};
use crate::ui::notify::Notice;
use crate::ui::scan::{self, CameraScan};
use crate::ui::theme;
use crate::validate;

#[derive(Default)]
pub struct SuppliersState {
    // Add / edit supplier form
    name: String,
    contact_phone: String,
    contact_email: String,
    address: String,
    notes: String,
    supplier_status: Notice,
    editing_supplier_id: Option<i64>,
    /// Supplier the user clicked "Delete" on, waiting for "Yes".
    confirm_delete_id: Option<i64>,

    // Log a delivery
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
    /// Barcode box for picking the product of a delivery line by scanning.
    line_barcode: String,
    camera: CameraScan,
}

pub fn show(ui: &mut egui::Ui, conn: &mut Connection, state: &mut SuppliersState) {
    ui.heading("Suppliers");
    ui.add_space(8.0);

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

    // Suppliers (form + list) on the left, the delivery log beside them on
    // the right. Each side scrolls on its own if the window is short.
    ui.columns(2, |columns| {
        egui::ScrollArea::vertical()
            .id_salt("suppliers_left_scroll")
            .show(&mut columns[0], |ui| {
                render_supplier_form(ui, conn, state);
                ui.add_space(16.0);
                render_supplier_list(ui, conn, state, &suppliers);
            });

        egui::ScrollArea::vertical()
            .id_salt("suppliers_right_scroll")
            .show(&mut columns[1], |ui| {
                render_delivery_form(ui, conn, state, &suppliers, &products);
            });
    });
}

// ---------- Add / edit supplier ----------

fn render_supplier_form(ui: &mut egui::Ui, conn: &Connection, state: &mut SuppliersState) {
    let heading = if state.editing_supplier_id.is_some() { "Edit supplier" } else { "Add supplier" };
    theme::section_title(ui, heading);

    theme::panel(ui, |ui| {
        egui::Grid::new("add_supplier_grid")
            .num_columns(2)
            .min_col_width(theme::LABEL_COL)
            .spacing(egui::vec2(12.0, 10.0))
            .show(ui, |ui| {
                theme::form_label(ui, "Name");
                theme::wide_text_edit(ui, &mut state.name, "e.g. Cosmos Pharmaceuticals");
                ui.end_row();

                theme::form_label(ui, "Phone");
                theme::wide_text_edit(ui, &mut state.contact_phone, "e.g. 0712 345 678");
                ui.end_row();

                theme::form_label(ui, "Email");
                theme::wide_text_edit(ui, &mut state.contact_email, "e.g. orders@supplier.com");
                ui.end_row();

                theme::form_label(ui, "Address");
                theme::wide_text_edit(ui, &mut state.address, "optional");
                ui.end_row();

                theme::form_label(ui, "Notes");
                theme::wide_text_edit(ui, &mut state.notes, "optional");
                ui.end_row();
            });

        ui.add_space(10.0);

        let button_label = if state.editing_supplier_id.is_some() { "Save changes" } else { "Add supplier" };

        ui.horizontal(|ui| {
            if theme::primary_button(ui, button_label).clicked() {
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
                                state.supplier_status =
                                    Notice::success(format!("{verb} \"{}\".", supplier.name));
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
    });
}

// ---------- Supplier list ----------

fn render_supplier_list(ui: &mut egui::Ui, conn: &Connection, state: &mut SuppliersState, suppliers: &[Supplier]) {
    theme::section_title(ui, &format!("Supplier list ({})", suppliers.len()));

    theme::panel(ui, |ui| {
        if suppliers.is_empty() {
            ui.label("No suppliers yet. Add one above.");
            return;
        }

        theme::compact_list(ui);
        egui::ScrollArea::both()
            .id_salt("supplier_list_scroll")
            .max_height(320.0)
            .show(ui, |ui| {
                egui::Grid::new("supplier_table")
                    .num_columns(4)
                    .spacing(egui::vec2(16.0, 6.0))
                    .with_row_color(theme::stripe_rows)
                    .show(ui, |ui| {
                        theme::column_header(ui, "Name");
                        theme::column_header(ui, "Phone");
                        theme::column_header(ui, "Email");
                        theme::column_header(ui, "");
                        ui.end_row();

                        for supplier in suppliers {
                            ui.label(&supplier.name);
                            ui.label(&supplier.contact_phone);
                            ui.label(&supplier.contact_email);

                            ui.horizontal(|ui| {
                                if state.confirm_delete_id == Some(supplier.id) {
                                    ui.colored_label(theme::STATUS_CRITICAL, "Delete?");
                                    if theme::danger_button(ui, "Yes").clicked() {
                                        match db::deactivate_supplier(conn, supplier.id) {
                                            Ok(()) => {
                                                if state.editing_supplier_id == Some(supplier.id) {
                                                    clear_supplier_form(state);
                                                }
                                                if state.selected_supplier == Some(supplier.id) {
                                                    state.selected_supplier = None;
                                                }
                                                state.supplier_status =
                                                    Notice::success(format!("Deleted \"{}\".", supplier.name));
                                            }
                                            Err(e) => {
                                                state.supplier_status =
                                                    Notice::error(format!("Database error: {e}"))
                                            }
                                        }
                                        state.confirm_delete_id = None;
                                    }
                                    if ui.button("No").clicked() {
                                        state.confirm_delete_id = None;
                                    }
                                } else {
                                    if ui.button("Edit").clicked() {
                                        load_supplier_into_form(state, supplier);
                                        state.confirm_delete_id = None;
                                    }
                                    if ui.button("Delete").clicked() {
                                        state.confirm_delete_id = Some(supplier.id);
                                    }
                                }
                            });

                            ui.end_row();
                        }
                    });
            });
    });
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

// ---------- Log a delivery ----------

fn render_delivery_form(
    ui: &mut egui::Ui,
    conn: &mut Connection,
    state: &mut SuppliersState,
    suppliers: &[Supplier],
    products: &[Product],
) {
    theme::section_title(ui, "Log a delivery");

    theme::panel(ui, |ui| {
        if suppliers.is_empty() {
            ui.label("Add a supplier first before logging a delivery.");
            return;
        }
        if products.is_empty() {
            ui.label("Add at least one product in Inventory first before logging a delivery.");
            return;
        }

        // ---- 1. Delivery details ----
        theme::subheading(ui, "1. Delivery details");

        let supplier_text = state
            .selected_supplier
            .and_then(|id| suppliers.iter().find(|s| s.id == id))
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "Choose a supplier".to_string());

        egui::Grid::new("delivery_header_grid")
            .num_columns(2)
            .min_col_width(theme::LABEL_COL)
            .spacing(egui::vec2(12.0, 10.0))
            .show(ui, |ui| {
                theme::form_label(ui, "Supplier");
                let width = ui.available_width();
                egui::ComboBox::from_id_salt("delivery_supplier")
                    .width(width)
                    .selected_text(supplier_text)
                    .show_ui(ui, |ui| {
                        for supplier in suppliers {
                            ui.selectable_value(&mut state.selected_supplier, Some(supplier.id), &supplier.name);
                        }
                    });
                ui.end_row();

                theme::form_label(ui, "Invoice #");
                theme::wide_text_edit(ui, &mut state.invoice_number, "e.g. INV-2026-0091");
                ui.end_row();

                theme::form_label(ui, "Notes");
                theme::wide_text_edit(ui, &mut state.delivery_notes, "optional");
                ui.end_row();
            });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // ---- 2. Add a product line ----
        theme::subheading(ui, "2. Add a product line");

        // Scanning the box picks the product below (no need to scroll the list).
        theme::form_label(ui, "Scan the box");
        if let Some(code) = scan::barcode_box(
            ui,
            &mut state.line_barcode,
            &mut state.camera,
            "delivery_barcode",
            "Scan the barcode (or type it + Enter)",
            false,
        ) {
            match products.iter().find(|p| p.barcode.as_deref() == Some(code.as_str())) {
                Some(product) => {
                    state.line_product = Some(product.id);
                    state.delivery_status = Notice::success(format!("Selected \"{}\".", product.name));
                }
                None => {
                    state.delivery_status = Notice::error(format!(
                        "No product has barcode {code}. Add it in Inventory first (scan it there)."
                    ));
                }
            }
        }
        ui.add_space(6.0);

        let product_text = state
            .line_product
            .and_then(|id| products.iter().find(|p| p.id == id))
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Choose a product".to_string());

        egui::Grid::new("delivery_line_grid")
            .num_columns(2)
            .min_col_width(theme::LABEL_COL)
            .spacing(egui::vec2(12.0, 10.0))
            .show(ui, |ui| {
                theme::form_label(ui, "Product");
                let width = ui.available_width();
                egui::ComboBox::from_id_salt("delivery_product")
                    .width(width)
                    .selected_text(product_text)
                    .show_ui(ui, |ui| {
                        for product in products {
                            ui.selectable_value(&mut state.line_product, Some(product.id), &product.name);
                        }
                    });
                ui.end_row();

                theme::form_label(ui, "Quantity");
                theme::wide_text_edit(ui, &mut state.line_quantity, "e.g. 50");
                ui.end_row();

                theme::form_label(ui, "Unit cost");
                theme::wide_text_edit(ui, &mut state.line_unit_cost, "e.g. 120.00");
                ui.end_row();

                theme::form_label(ui, "Batch number");
                theme::wide_text_edit(ui, &mut state.line_batch, "e.g. BN-2026-014");
                ui.end_row();

                theme::form_label(ui, "Expiry date");
                theme::wide_text_edit(ui, &mut state.line_expiry, "YYYY-MM-DD, e.g. 2027-06-30");
                ui.end_row();
            });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("+ Add line").clicked() {
                match build_delivery_item(state) {
                    Ok(item) => {
                        state.items.push(item);
                        state.line_product = None;
                        state.line_quantity.clear();
                        state.line_unit_cost.clear();
                        state.line_batch.clear();
                        state.line_expiry.clear();
                        state.line_barcode.clear();
                        state.delivery_status = Notice::None;
                    }
                    Err(msg) => state.delivery_status = Notice::error(msg),
                }
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);

        // ---- 3. Lines in this delivery ----
        theme::subheading(ui, &format!("3. Lines in this delivery ({})", state.items.len()));

        if state.items.is_empty() {
            ui.label(egui::RichText::new("No lines added yet.").color(theme::TEXT_SECONDARY));
        } else {
            let mut total = Cents(0);
            let mut remove_index: Option<usize> = None;

            ui.scope(|ui| {
                theme::compact_list(ui);
                egui::ScrollArea::both()
                    .id_salt("delivery_lines_scroll")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        egui::Grid::new("delivery_items_table")
                            .num_columns(7)
                            .spacing(egui::vec2(12.0, 6.0))
                            .with_row_color(theme::stripe_rows)
                            .show(ui, |ui| {
                                theme::column_header(ui, "Product");
                                theme::column_header(ui, "Qty");
                                theme::column_header(ui, "Unit cost");
                                theme::column_header(ui, "Batch");
                                theme::column_header(ui, "Expiry");
                                theme::column_header(ui, "Line total");
                                theme::column_header(ui, "");
                                ui.end_row();

                                for (index, item) in state.items.iter().enumerate() {
                                    let product_name = products
                                        .iter()
                                        .find(|p| p.id == item.product_id)
                                        .map(|p| p.name.as_str())
                                        .unwrap_or("Unknown product");

                                    total = total + item.line_cost();

                                    ui.label(product_name);
                                    theme::number_cell(ui, item.quantity.to_string());
                                    theme::number_cell(ui, format!("{:.2}", item.unit_cost.as_major()));
                                    ui.label(&item.batch_number);
                                    ui.label(item.expiry_date.to_string());
                                    theme::number_cell(ui, format!("{:.2}", item.line_cost().as_major()));
                                    if ui.button("Remove").clicked() {
                                        remove_index = Some(index);
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });

            if let Some(index) = remove_index {
                state.items.remove(index);
            }

            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!("Total cost: {:.2}", total.as_major()))
                    .strong()
                    .size(20.0)
                    .color(theme::TEXT_PRIMARY),
            );
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if theme::primary_button(ui, "Record delivery").clicked() {
                if state.selected_supplier.is_none() {
                    state.delivery_status = Notice::error("Choose a supplier first.");
                } else if state.items.is_empty() {
                    state.delivery_status = Notice::error("Add at least one product line first.");
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
        });

        state.delivery_status.show(ui);
    });
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