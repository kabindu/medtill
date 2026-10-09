use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{Cents, Product};
use crate::ui::notify::Notice;
use crate::ui::scan::{self, CameraScan};
use crate::ui::theme;
use crate::validate;

#[derive(Default)]
pub struct InventoryState {
    name: String,
    category: String,
    batch_number: String,
    expiry_date: String,
    price: String,
    stock_quantity: String,
    reorder_level: String,
    status: Notice,
    editing_id: Option<i64>,
    /// Product the user clicked "Delete" on, waiting for "Yes". A two-step
    /// delete, so a single stray click can't remove a medicine.
    confirm_delete_id: Option<i64>,
    search: String,
    /// The Barcode field of the form (filled by a USB scanner, the camera,
    /// or typing).
    barcode: String,
    /// The webcam behind the Barcode field's "Camera" button.
    camera: CameraScan,
}

pub fn show(ui: &mut egui::Ui, conn: &mut Connection, state: &mut InventoryState) {
    ui.heading("Inventory");
    ui.add_space(4.0);

    let products = match db::list_products(conn) {
        Ok(products) => products,
        Err(e) => {
            Notice::error(format!("Failed to load products: {e}")).show(ui);
            return;
        }
    };

    // Shared borrow from here on: both columns only need `&Connection`
    // (add/update/delete all take a shared reference).
    let conn: &Connection = conn;

    // Form on the left at a fixed width, product list on the right taking
    // all the remaining width. Both sides scroll on their own, so nothing
    // spills off the edge of the window however many products there are.
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(FORM_WIDTH);
            egui::ScrollArea::vertical()
                .id_salt("inventory_form_scroll")
                .show(ui, |ui| {
                    render_form(ui, conn, state, &products);
                });
        });

        ui.separator();

        ui.vertical(|ui| {
            render_product_list(ui, conn, state, &products);
        });
    });
}

/// Width of the Add/Edit product column. The list gets everything else.
const FORM_WIDTH: f32 = 470.0;

fn render_form(ui: &mut egui::Ui, conn: &Connection, state: &mut InventoryState, products: &[Product]) {
    let heading = if state.editing_id.is_some() {
        "Edit product"
    } else {
        "Add product"
    };
    ui.label(
        egui::RichText::new(heading)
            .strong()
            .size(20.0)
            .color(theme::TEXT_PRIMARY),
    );
    ui.add_space(4.0);

    theme::card(ui, |ui| {
        let field_label = |ui: &mut egui::Ui, text: &str| {
            ui.label(egui::RichText::new(text).color(theme::TEXT_SECONDARY));
        };

        // Barcode first: scanning it either finds an existing product (and
        // opens it for editing) or starts a new one with the code filled in.
        field_label(ui, "Barcode");
        if let Some(code) = scan::barcode_box(
            ui,
            &mut state.barcode,
            &mut state.camera,
            "inventory_barcode",
            "Scan with the scanner or camera, or type + Enter",
            false,
        ) {
            on_barcode_scanned(state, products, &code);
        }
        ui.add_space(6.0);

        egui::Grid::new("add_product_grid")
            .num_columns(2)
            .spacing(egui::vec2(16.0, 10.0))
            .show(ui, |ui| {
                field_label(ui, "Name");
                ui.add(
                    egui::TextEdit::singleline(&mut state.name).hint_text("e.g. Paracetamol 500mg"),
                );
                ui.end_row();

                field_label(ui, "Category");
                ui.add(
                    egui::TextEdit::singleline(&mut state.category).hint_text("e.g. Painkiller"),
                );
                ui.end_row();

                field_label(ui, "Batch number");
                ui.add(
                    egui::TextEdit::singleline(&mut state.batch_number)
                        .hint_text("e.g. BN-2026-014"),
                );
                ui.end_row();

                field_label(ui, "Expiry date");
                ui.add(
                    egui::TextEdit::singleline(&mut state.expiry_date)
                        .hint_text("YYYY-MM-DD, e.g. 2027-06-30"),
                );
                ui.end_row();

                field_label(ui, "Price");
                ui.add(egui::TextEdit::singleline(&mut state.price).hint_text("e.g. 250.00"));
                ui.end_row();

                field_label(ui, "Stock quantity");
                ui.add(egui::TextEdit::singleline(&mut state.stock_quantity).hint_text("e.g. 100"));
                ui.end_row();

                field_label(ui, "Reorder level");
                ui.add(egui::TextEdit::singleline(&mut state.reorder_level).hint_text("e.g. 10"));
                ui.end_row();
            });

        ui.add_space(8.0);

        let button_label = if state.editing_id.is_some() {
            "Save changes"
        } else {
            "Add product"
        };

        ui.horizontal(|ui| {
            // The primary action always wears the accent fill, so it's
            // obvious which button commits the form.
            let primary =
                egui::Button::new(egui::RichText::new(button_label).color(egui::Color32::WHITE))
                    .fill(theme::ACCENT);
            if ui.add(primary).clicked() {
                match build_product(state).and_then(|product| check_barcode_free(&product, products).map(|_| product)) {
                    Ok(product) => {
                        let result = if state.editing_id.is_some() {
                            db::update_product(conn, &product)
                        } else {
                            db::add_product(conn, &product).map(|_| ())
                        };

                        match result {
                            Ok(()) => {
                                let verb = if state.editing_id.is_some() {
                                    "Updated"
                                } else {
                                    "Added"
                                };
                                state.status =
                                    Notice::success(format!("{verb} \"{}\".", product.name));
                                clear_form(state);
                            }
                            Err(e) => state.status = Notice::error(format!("Database error: {e}")),
                        }
                    }
                    Err(msg) => state.status = Notice::error(msg),
                }
            }

            if state.editing_id.is_some() && ui.button("Cancel").clicked() {
                clear_form(state);
                state.status = Notice::None;
            }
        });

        state.status.show(ui);
    });
}

fn render_product_list(
    ui: &mut egui::Ui,
    conn: &Connection,
    state: &mut InventoryState,
    products: &[Product],
) {
    ui.label(
        egui::RichText::new(format!("Products ({})", products.len()))
            .strong()
            .size(20.0),
    );
    ui.add_space(4.0);

    if products.is_empty() {
        ui.label("No products yet. Add one on the left.");
        return;
    }

    ui.add(egui::TextEdit::singleline(&mut state.search).hint_text("Search by name, category or barcode"));
    ui.add_space(6.0);

    let query = state.search.trim().to_lowercase();
    let visible: Vec<&Product> = products
        .iter()
        .filter(|p| {
            query.is_empty()
                || p.name.to_lowercase().contains(&query)
                || p.category.to_lowercase().contains(&query)
                || p.barcode.as_deref().is_some_and(|code| code.contains(&query))
        })
        .collect();

    if visible.is_empty() {
        ui.label("No products match your search.");
        return;
    }

    let today = chrono::Local::now().date_naive();

    theme::card(ui, |ui| {
        theme::compact_list(ui);
        egui::ScrollArea::both()
            .id_salt("inventory_list_scroll")
            .show(ui, |ui| {
                egui::Grid::new("product_table")
                    .num_columns(6)
                    .spacing(egui::vec2(16.0, 8.0))
                    .with_row_color(theme::stripe_rows)
                    .show(ui, |ui| {
                        let header = |ui: &mut egui::Ui, text: &str| {
                            ui.label(
                                egui::RichText::new(text)
                                    .strong()
                                    .color(theme::TEXT_SECONDARY),
                            );
                        };
                        header(ui, "Name");
                        header(ui, "Category");
                        header(ui, "Stock");
                        header(ui, "Price");
                        header(ui, "Status");
                        header(ui, "");
                        ui.end_row();

                        for product in &visible {
                            ui.vertical(|ui| {
                                ui.label(&product.name);
                                if let Some(code) = &product.barcode {
                                    ui.label(
                                        egui::RichText::new(code)
                                            .small()
                                            .color(theme::TEXT_SECONDARY),
                                    );
                                }
                            });
                            ui.label(&product.category);

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(product.stock_quantity.to_string());
                                },
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(format!("{:.2}", product.price.as_major()));
                                },
                            );

                            ui.vertical(|ui| {
                                let expired = product.is_expired(today);
                                let expiring_soon = product.is_expiring_soon(today, 30);
                                let low_stock = product.is_low_stock();

                                if expired {
                                    ui.colored_label(theme::STATUS_CRITICAL, "● Expired");
                                } else if expiring_soon {
                                    ui.colored_label(theme::STATUS_SERIOUS, "● Expiring soon");
                                }
                                if low_stock {
                                    ui.colored_label(theme::STATUS_WARNING, "● Low stock");
                                }
                                if !expired && !expiring_soon && !low_stock {
                                    ui.colored_label(theme::STATUS_GOOD, "● OK");
                                }
                            });

                            ui.horizontal(|ui| {
                                if state.confirm_delete_id == Some(product.id) {
                                    ui.colored_label(theme::STATUS_CRITICAL, "Delete?");
                                    let yes = egui::Button::new(
                                        egui::RichText::new("Yes").color(egui::Color32::WHITE),
                                    )
                                        .fill(theme::STATUS_CRITICAL);
                                    if ui.add(yes).clicked() {
                                        match db::deactivate_product(conn, product.id) {
                                            Ok(()) => {
                                                if state.editing_id == Some(product.id) {
                                                    clear_form(state);
                                                }
                                                state.status = Notice::success(format!(
                                                    "Deleted \"{}\".",
                                                    product.name
                                                ));
                                            }
                                            Err(e) => {
                                                state.status =
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
                                        load_into_form(state, product);
                                        state.confirm_delete_id = None;
                                    }
                                    if ui.button("Delete").clicked() {
                                        state.confirm_delete_id = Some(product.id);
                                    }
                                }
                            });

                            ui.end_row();
                        }
                    });
            });
    });
}

fn clear_form(state: &mut InventoryState) {
    state.name.clear();
    state.category.clear();
    state.batch_number.clear();
    state.expiry_date.clear();
    state.price.clear();
    state.stock_quantity.clear();
    state.reorder_level.clear();
    state.barcode.clear();
    state.editing_id = None;
}

fn load_into_form(state: &mut InventoryState, product: &Product) {
    state.name = product.name.clone();
    state.category = product.category.clone();
    state.batch_number = product.batch_number.clone();
    state.expiry_date = product.expiry_date.format("%Y-%m-%d").to_string();
    state.price = format!("{:.2}", product.price.as_major());
    state.stock_quantity = product.stock_quantity.to_string();
    state.reorder_level = product.reorder_level.to_string();
    state.barcode = product.barcode.clone().unwrap_or_default();
    state.editing_id = Some(product.id);
}

fn build_product(state: &InventoryState) -> Result<Product, String> {
    let name = validate::parse_name("Product name", &state.name)?;
    let category = state.category.trim().to_string();
    let batch_number = state.batch_number.trim().to_string();

    let today = chrono::Local::now().date_naive();
    let expiry_date = validate::parse_expiry_date(&state.expiry_date, today, false)?;

    let price = validate::parse_price("Price", &state.price)?;
    let stock_quantity = validate::parse_quantity("Stock quantity", &state.stock_quantity)?;
    let reorder_level = validate::parse_quantity("Reorder level", &state.reorder_level)?;
    let barcode = validate::parse_barcode(&state.barcode)?;

    Ok(Product {
        id: state.editing_id.unwrap_or(0),
        name,
        category,
        batch_number,
        expiry_date,
        price: Cents::from_major(price),
        cost_price: Cents(0),
        stock_quantity,
        supplier_id: None,
        reorder_level,
        barcode,
    })
}

/// A barcode was scanned into the form's Barcode box. If a product already
/// has that code, open it for editing (e.g. to restock or reprice it);
/// otherwise keep the code for the new product being entered.
fn on_barcode_scanned(state: &mut InventoryState, products: &[Product], code: &str) {
    if let Err(msg) = validate::parse_barcode(code) {
        state.status = Notice::error(msg);
        return;
    }

    match products.iter().find(|p| p.barcode.as_deref() == Some(code)) {
        Some(product) => {
            load_into_form(state, product);
            state.status = Notice::success(format!(
                "Found \"{}\" — it's open for editing below.",
                product.name
            ));
        }
        None => {
            if state.editing_id.is_some() {
                // Scanning a new code while editing: keep editing, just
                // attach the code to this product.
                state.status = Notice::success(format!(
                    "Barcode {code} will be saved on this product when you click Save changes."
                ));
            } else {
                state.status = Notice::success(format!(
                    "New barcode {code}. Fill in the details and click Add product."
                ));
            }
        }
    }
}

/// Refuses to save if another product already uses this barcode, so one
/// scan can never match two different medicines.
fn check_barcode_free(product: &Product, products: &[Product]) -> Result<(), String> {
    let Some(code) = &product.barcode else {
        return Ok(());
    };
    match products
        .iter()
        .find(|other| other.id != product.id && other.barcode.as_ref() == Some(code))
    {
        Some(other) => Err(format!(
            "Barcode {code} already belongs to \"{}\". Scan it to open that product instead.",
            other.name
        )),
        None => Ok(()),
    }
}