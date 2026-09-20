use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{Cents, Product};
use crate::ui::notify::Notice;
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
}

pub fn show(ui: &mut egui::Ui, conn: &mut Connection, state: &mut InventoryState) {
    ui.heading("Inventory");
    ui.add_space(4.0);

    let heading = if state.editing_id.is_some() { "Edit product" } else { "Add product" };
    // Force the section open whenever we're mid-edit — otherwise the
    // heading text changing (see below) makes egui treat it as a
    // never-before-seen section, which defaults to closed.
    let force_open = if state.editing_id.is_some() { Some(true) } else { None };
    egui::CollapsingHeader::new(egui::RichText::new(heading).strong().size(16.0))
        .id_salt("product_form") // fixed identity, independent of the displayed heading text
        .open(force_open)
        .show(ui, |ui| {
            theme::card(ui, |ui| {
                let field_label = |ui: &mut egui::Ui, text: &str| {
                    ui.label(egui::RichText::new(text).color(theme::TEXT_SECONDARY));
                };

                egui::Grid::new("add_product_grid")
                    .num_columns(2)
                    .spacing(egui::vec2(16.0, 10.0))
                    .show(ui, |ui| {
                        field_label(ui, "Name");
                        ui.add(egui::TextEdit::singleline(&mut state.name).hint_text("e.g. Paracetamol 500mg"));
                        ui.end_row();

                        field_label(ui, "Category");
                        ui.add(egui::TextEdit::singleline(&mut state.category).hint_text("e.g. Painkiller"));
                        ui.end_row();

                        field_label(ui, "Batch number");
                        ui.add(egui::TextEdit::singleline(&mut state.batch_number).hint_text("e.g. BN-2026-014"));
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

                let button_label = if state.editing_id.is_some() { "Save changes" } else { "Add" };

                ui.horizontal(|ui| {
                    // The primary action always wears the accent fill, so it's
                    // visually obvious which button actually commits the form —
                    // "Cancel" stays in the default (secondary) button style.
                    let primary = egui::Button::new(egui::RichText::new(button_label).color(egui::Color32::WHITE))
                        .fill(theme::ACCENT);
                    if ui.add(primary).clicked() {
                        match build_product(state) {
                            Ok(product) => {
                                let result = if state.editing_id.is_some() {
                                    db::update_product(conn, &product)
                                } else {
                                    db::add_product(conn, &product).map(|_| ())
                                };

                                match result {
                                    Ok(()) => {
                                        let verb = if state.editing_id.is_some() { "Updated" } else { "Added" };
                                        state.status = Notice::success(format!("{verb} \"{}\".", product.name));
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
        });

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(8.0);

    match db::list_products(conn) {
        Ok(products) => {
            if products.is_empty() {
                ui.label("No products yet — add one above.");
            } else {
                render_product_table(ui, state, &products);
            }
        }
        Err(e) => Notice::error(format!("Failed to load products: {e}")).show(ui),
    }
}

fn clear_form(state: &mut InventoryState) {
    state.name.clear();
    state.category.clear();
    state.batch_number.clear();
    state.expiry_date.clear();
    state.price.clear();
    state.stock_quantity.clear();
    state.reorder_level.clear();
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
    })
}

fn render_product_table(ui: &mut egui::Ui, state: &mut InventoryState, products: &[Product]) {
    let today = chrono::Local::now().date_naive();

    theme::card(ui, |ui| {
        egui::Grid::new("product_table")
            .num_columns(6)
            .spacing(egui::vec2(16.0, 8.0))
            .with_row_color(|row, _style| {
                // Skip the header row (0); tint every other data row.
                if row > 0 && row % 2 == 0 {
                    Some(theme::STRIPE_BG)
                } else {
                    None
                }
            })
            .show(ui, |ui| {
                let header = |ui: &mut egui::Ui, text: &str| {
                    ui.label(egui::RichText::new(text).strong().color(theme::TEXT_SECONDARY));
                };
                header(ui, "Name");
                header(ui, "Category");
                header(ui, "Stock");
                header(ui, "Price");
                header(ui, "Status");
                header(ui, "");
                ui.end_row();

                for product in products {
                    ui.label(&product.name);
                    ui.label(&product.category);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(product.stock_quantity.to_string());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("{:.2}", product.price.as_major()));
                    });

                    ui.horizontal(|ui| {
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

                    if ui.button("Edit").clicked() {
                        load_into_form(state, product);
                    }

                    ui.end_row();
                }
            });
    });
}