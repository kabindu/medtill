use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{CartItem, Cents, PaymentMethod, Product, Sale, SaleItem};
use crate::ui::notify::Notice;

#[derive(Default)]
pub struct PosState {
    search: String,
    cart: Vec<CartItem>,
    payment_method: PaymentMethod,
    status: Notice,
}

pub fn show(ui: &mut egui::Ui, conn: &mut Connection, state: &mut PosState) {
    ui.heading("Checkout");

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

fn render_product_picker(ui: &mut egui::Ui, state: &mut PosState, products: &[Product]) {
    ui.heading("Products");
    ui.add(egui::TextEdit::singleline(&mut state.search).hint_text("Search products..."));
    ui.separator();

    let query = state.search.to_lowercase();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for product in products {
            if !query.is_empty() && !product.name.to_lowercase().contains(&query) {
                continue;
            }

            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} — {:.2} (stock: {})",
                    product.name,
                    product.price.as_major(),
                    product.stock_quantity
                ));

                let out_of_stock = product.stock_quantity == 0;
                ui.add_enabled_ui(!out_of_stock, |ui| {
                    if ui.button("Add").clicked() {
                        add_to_cart(state, product);
                    }
                });
                if out_of_stock {
                    ui.colored_label(egui::Color32::from_rgb(198, 40, 40), "Out of stock");
                }
            });
        }
    });
}

fn add_to_cart(state: &mut PosState, product: &Product) {
    if let Some(existing) = state.cart.iter_mut().find(|item| item.product_id == product.id) {
        existing.quantity += 1;
    } else {
        state.cart.push(CartItem { product_id: product.id, quantity: 1 });
    }
    state.status = Notice::None;
}

fn render_cart(ui: &mut egui::Ui, conn: &mut Connection, state: &mut PosState, products: &[Product]) {
    ui.heading("Cart");

    if state.cart.is_empty() {
        ui.label("Cart is empty — add products from the left.");
    }

    let mut remove_index: Option<usize> = None;
    let mut total = Cents(0);

    for (index, item) in state.cart.iter().enumerate() {
        if let Some(product) = products.iter().find(|p| p.id == item.product_id) {
            let subtotal = item.subtotal(product);
            total = total + subtotal;

            ui.horizontal(|ui| {
                ui.label(format!("{} x{}", product.name, item.quantity));
                ui.label(format!("{:.2}", subtotal.as_major()));

                if item.quantity > product.stock_quantity {
                    ui.colored_label(
                        egui::Color32::from_rgb(198, 40, 40),
                        format!("Only {} in stock", product.stock_quantity),
                    );
                }

                if ui.button("Remove").clicked() {
                    remove_index = Some(index);
                }
            });
        }
    }

    if let Some(index) = remove_index {
        state.cart.remove(index);
    }

    ui.separator();
    ui.strong(format!("Total: {:.2}", total.as_major()));

    ui.horizontal(|ui| {
        ui.label("Payment:");
        ui.selectable_value(&mut state.payment_method, PaymentMethod::Cash, "Cash");
        ui.selectable_value(&mut state.payment_method, PaymentMethod::Card, "Card");
        ui.selectable_value(&mut state.payment_method, PaymentMethod::MobileMoney, "Mobile Money");
    });

    if ui.button("Complete Sale").clicked() {
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

    state.status.show(ui);
}

/// Checks the cart is non-empty and every line has enough stock, before
/// touching the database at all. Pure read-only check — borrows both
/// arguments, mutates nothing.
fn validate_cart(state: &PosState, products: &[Product]) -> Result<(), String> {
    if state.cart.is_empty() {
        return Err("Cart is empty — add at least one product before checking out.".to_string());
    }

    for item in &state.cart {
        if let Some(product) = products.iter().find(|p| p.id == item.product_id) {
            if item.quantity > product.stock_quantity {
                return Err(format!(
                    "Not enough stock of \"{}\" — only {} available.",
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