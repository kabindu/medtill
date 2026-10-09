use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{CartItem, Cents, PaymentMethod, Product, Sale, SaleItem};
use crate::ui::notify::Notice;
use crate::ui::scan::{self, CameraScan};
use crate::ui::theme;

#[derive(Default)]
pub struct PosState {
    search: String,
    cart: Vec<CartItem>,
    payment_method: PaymentMethod,
    status: Notice,
    /// The "Scan" box: a USB scanner types here; the camera fills it too.
    barcode: String,
    camera: CameraScan,
}

/// What a click on a cart line asked for. The cart is only *read* while it's
/// being drawn, so clicks are recorded here and applied once drawing is done.
/// `Copy` because it's just a tag plus an index.
#[derive(Clone, Copy)]
enum CartAction {
    Decrease(usize),
    Increase(usize),
    Remove(usize),
}

pub fn show(ui: &mut egui::Ui, conn: &mut Connection, state: &mut PosState) {
    ui.heading("Checkout");
    ui.add_space(4.0);

    let products = match db::list_products(conn) {
        Ok(products) => products,
        Err(e) => {
            Notice::error(format!("Failed to load products: {e}")).show(ui);
            return;
        }
    };

    ui.columns(2, |columns| {
        render_product_picker(&mut columns[0], state, &products);
        render_cart(&mut columns[1], conn, state, &products);
    });
}

// ---------- Product picker (left) ----------

fn render_product_picker(ui: &mut egui::Ui, state: &mut PosState, products: &[Product]) {
    // Scanning is the fast path: each scan adds one unit to the cart.
    theme::section_title(ui, "Scan");
    if let Some(code) = scan::barcode_box(
        ui,
        &mut state.barcode,
        &mut state.camera,
        "checkout_barcode",
        "Scan a barcode (or type it + Enter)",
        true,
    ) {
        state.barcode.clear();
        on_barcode_scanned(state, products, &code);
    }
    ui.add_space(10.0);

    theme::section_title(ui, "Products");
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

    if products.is_empty() {
        ui.label("No products yet. Add some in Inventory first.");
        return;
    }
    if visible.is_empty() {
        ui.label("No products match your search.");
        return;
    }

    theme::card(ui, |ui| {
        theme::compact_list(ui);
        egui::ScrollArea::vertical().id_salt("pos_products_scroll").show(ui, |ui| {
            egui::Grid::new("pos_product_table")
                .num_columns(4)
                .spacing(egui::vec2(14.0, 6.0))
                .with_row_color(theme::stripe_rows)
                .show(ui, |ui| {
                    theme::column_header(ui, "Name");
                    theme::column_header(ui, "Price");
                    theme::column_header(ui, "Stock");
                    theme::column_header(ui, "");
                    ui.end_row();

                    for product in &visible {
                        ui.label(&product.name);
                        theme::number_cell(ui, format!("{:.2}", product.price.as_major()));
                        theme::number_cell(ui, product.stock_quantity.to_string());

                        if product.stock_quantity == 0 {
                            ui.colored_label(theme::STATUS_CRITICAL, "Out of stock");
                        } else if ui.button("Add").clicked() {
                            add_to_cart(state, product);
                        }

                        ui.end_row();
                    }
                });
        });
    });
}

/// A barcode was scanned at the till: find that product and add one unit.
fn on_barcode_scanned(state: &mut PosState, products: &[Product], code: &str) {
    match products.iter().find(|p| p.barcode.as_deref() == Some(code)) {
        Some(product) if product.stock_quantity == 0 => {
            state.status = Notice::error(format!("\"{}\" is out of stock.", product.name));
        }
        Some(product) => {
            add_to_cart(state, product);
            // `add_to_cart` leaves an error if the stock limit was reached.
            if matches!(state.status, Notice::None) {
                state.status = Notice::success(format!("Added \"{}\".", product.name));
            }
        }
        None => {
            state.status = Notice::error(format!(
                "No product has barcode {code}. Add it in Inventory first (scan it there)."
            ));
        }
    }
}

/// Adds one unit of `product`, or bumps the quantity if it's already in the
/// cart. Never lets the cart ask for more than is on the shelf.
fn add_to_cart(state: &mut PosState, product: &Product) {
    if let Some(existing) = state.cart.iter_mut().find(|item| item.product_id == product.id) {
        if existing.quantity < product.stock_quantity {
            existing.quantity += 1;
            state.status = Notice::None;
        } else {
            state.status = Notice::error(format!(
                "Only {} of \"{}\" in stock.",
                product.stock_quantity, product.name
            ));
        }
    } else {
        state.cart.push(CartItem { product_id: product.id, quantity: 1 });
        state.status = Notice::None;
    }
}

// ---------- Cart (right) ----------

fn render_cart(ui: &mut egui::Ui, conn: &mut Connection, state: &mut PosState, products: &[Product]) {
    theme::section_title(ui, "Cart");

    let mut action: Option<CartAction> = None;
    let mut total = Cents(0);

    theme::card(ui, |ui| {
        if state.cart.is_empty() {
            ui.label("Cart is empty. Add products from the left.");
            return;
        }

        theme::compact_list(ui);
        egui::ScrollArea::vertical().id_salt("cart_scroll").max_height(360.0).show(ui, |ui| {
            egui::Grid::new("cart_table")
                .num_columns(5)
                .spacing(egui::vec2(14.0, 6.0))
                .with_row_color(theme::stripe_rows)
                .show(ui, |ui| {
                    theme::column_header(ui, "Item");
                    theme::column_header(ui, "Price");
                    theme::column_header(ui, "Qty");
                    theme::column_header(ui, "Subtotal");
                    theme::column_header(ui, "");
                    ui.end_row();

                    for (index, item) in state.cart.iter().enumerate() {
                        let Some(product) = products.iter().find(|p| p.id == item.product_id) else {
                            continue;
                        };

                        let subtotal = item.subtotal(product);
                        total = total + subtotal;

                        ui.vertical(|ui| {
                            ui.label(&product.name);
                            if item.quantity > product.stock_quantity {
                                ui.colored_label(
                                    theme::STATUS_CRITICAL,
                                    format!("Only {} in stock", product.stock_quantity),
                                );
                            }
                        });

                        theme::number_cell(ui, format!("{:.2}", product.price.as_major()));

                        // − qty +   (going below 1 removes the line)
                        ui.horizontal(|ui| {
                            if ui.button("−").clicked() {
                                action = Some(CartAction::Decrease(index));
                            }
                            ui.label(egui::RichText::new(item.quantity.to_string()).strong());
                            let can_add = item.quantity < product.stock_quantity;
                            if ui.add_enabled(can_add, egui::Button::new("+")).clicked() {
                                action = Some(CartAction::Increase(index));
                            }
                        });

                        theme::number_cell(ui, format!("{:.2}", subtotal.as_major()));

                        if ui.button("Remove").clicked() {
                            action = Some(CartAction::Remove(index));
                        }

                        ui.end_row();
                    }
                });
        });
    });

    if let Some(action) = action {
        match action {
            CartAction::Decrease(index) => {
                if state.cart[index].quantity > 1 {
                    state.cart[index].quantity -= 1;
                } else {
                    state.cart.remove(index);
                }
            }
            CartAction::Increase(index) => {
                // The "+" button is only enabled below the stock level.
                state.cart[index].quantity += 1;
            }
            CartAction::Remove(index) => {
                state.cart.remove(index);
            }
        }
        state.status = Notice::None;
    }

    ui.add_space(10.0);
    ui.label(egui::RichText::new(format!("Total: {:.2}", total.as_major())).strong().size(28.0));
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.label("Payment:");
        ui.selectable_value(&mut state.payment_method, PaymentMethod::Cash, "Cash");
        ui.selectable_value(&mut state.payment_method, PaymentMethod::Card, "Card");
        ui.selectable_value(&mut state.payment_method, PaymentMethod::MobileMoney, "Mobile Money");
    });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if theme::primary_button(ui, "Complete sale").clicked() {
            match validate_cart(state, products) {
                Ok(()) => match checkout(conn, state, products) {
                    Ok(sale_id) => {
                        state.status = Notice::success(format!("Sale #{sale_id} recorded."));
                        state.cart.clear();
                    }
                    Err(e) => state.status = Notice::error(format!("Checkout failed: {e}")),
                },
                Err(msg) => state.status = Notice::error(msg),
            }
        }

        if !state.cart.is_empty() && ui.button("Clear cart").clicked() {
            state.cart.clear();
            state.status = Notice::None;
        }
    });

    state.status.show(ui);
}

/// Checks the cart is non-empty and every line has enough stock, before
/// touching the database at all. Pure read-only check — borrows both
/// arguments, mutates nothing.
fn validate_cart(state: &PosState, products: &[Product]) -> Result<(), String> {
    if state.cart.is_empty() {
        return Err("Cart is empty. Add at least one product before checking out.".to_string());
    }

    for item in &state.cart {
        if let Some(product) = products.iter().find(|p| p.id == item.product_id) {
            if item.quantity > product.stock_quantity {
                return Err(format!(
                    "Not enough stock of \"{}\": only {} available.",
                    product.name, product.stock_quantity
                ));
            }
        }
    }

    Ok(())
}

fn checkout(conn: &mut Connection, state: &PosState, products: &[Product]) -> rusqlite::Result<i64> {
    let items: Vec<SaleItem> = state
        .cart
        .iter()
        .filter_map(|cart_item| {
            products.iter().find(|p| p.id == cart_item.product_id).map(|product| SaleItem {
                product_id: product.id,
                product_name: product.name.clone(),
                quantity: cart_item.quantity,
                unit_price: product.price,
            })
        })
        .collect();

    let sale = Sale {
        id: 0,
        sale_date: chrono::Local::now().date_naive(),
        items,
        payment_method: state.payment_method,
    };

    db::record_sale(conn, &sale)
}