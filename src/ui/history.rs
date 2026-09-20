use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::ui::notify::Notice;

/// Which half of the history screen is showing — just a tag the UI flips
/// between frames, so `Copy` is free and correct (there's no owned data).
#[derive(Default, PartialEq, Clone, Copy)]
pub enum HistoryTab {
    #[default]
    Sales,
    Deliveries,
}

#[derive(Default)]
pub struct HistoryState {
    pub tab: HistoryTab,
    pub status: Notice,
}

pub fn show(ui: &mut egui::Ui, conn: &mut Connection, state: &mut HistoryState) {
    ui.heading("History");

    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.tab, HistoryTab::Sales, "Sales");
        ui.selectable_value(&mut state.tab, HistoryTab::Deliveries, "Deliveries");
    });

    ui.separator();

    match state.tab {
        HistoryTab::Sales => render_sales(ui, conn, state),
        HistoryTab::Deliveries => render_deliveries(ui, conn, state),
    }

    state.status.show(ui);
}

fn render_sales(ui: &mut egui::Ui, conn: &Connection, state: &mut HistoryState) {
    let sales = match db::list_sales(conn) {
        Ok(sales) => sales,
        Err(err) => {
            state.status = Notice::error(format!("Could not load sales: {err}"));
            return;
        }
    };

    if sales.is_empty() {
        ui.label("No sales recorded yet.");
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for sale in &sales {
            let heading = format!(
                "Sale #{}  —  {}  —  {}  —  Total {:.2}",
                sale.id,
                sale.sale_date,
                sale.payment_method,
                sale.total().as_major(),
            );
            egui::CollapsingHeader::new(heading)
                .id_salt(("sale_history", sale.id))
                .show(ui, |ui| {
                    egui::Grid::new(("sale_items", sale.id))
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Product");
                            ui.strong("Qty");
                            ui.strong("Unit price");
                            ui.strong("Subtotal");
                            ui.end_row();

                            for item in &sale.items {
                                ui.label(&item.product_name);
                                ui.label(item.quantity.to_string());
                                ui.label(format!("{:.2}", item.unit_price.as_major()));
                                ui.label(format!("{:.2}", item.subtotal().as_major()));
                                ui.end_row();
                            }
                        });
                });
        }
    });
}

fn render_deliveries(ui: &mut egui::Ui, conn: &Connection, state: &mut HistoryState) {
    let deliveries = match db::list_deliveries(conn) {
        Ok(deliveries) => deliveries,
        Err(err) => {
            state.status = Notice::error(format!("Could not load deliveries: {err}"));
            return;
        }
    };

    if deliveries.is_empty() {
        ui.label("No deliveries recorded yet.");
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for delivery in &deliveries {
            let invoice = if delivery.invoice_number.is_empty() {
                "—"
            } else {
                &delivery.invoice_number
            };
            let heading = format!(
                "Delivery #{}  —  {}  —  {}  —  Invoice {}  —  Total {:.2}",
                delivery.id,
                delivery.delivery_date,
                delivery.supplier_name,
                invoice,
                delivery.total_cost().as_major(),
            );
            egui::CollapsingHeader::new(heading)
                .id_salt(("delivery_history", delivery.id))
                .show(ui, |ui| {
                    if !delivery.notes.is_empty() {
                        ui.label(format!("Notes: {}", delivery.notes));
                    }
                    egui::Grid::new(("delivery_items", delivery.id))
                        .striped(true)
                        .show(ui, |ui| {
                            ui.strong("Product");
                            ui.strong("Qty");
                            ui.strong("Unit cost");
                            ui.strong("Batch");
                            ui.strong("Expiry");
                            ui.strong("Line cost");
                            ui.end_row();

                            for item in &delivery.items {
                                ui.label(&item.product_name);
                                ui.label(item.quantity.to_string());
                                ui.label(format!("{:.2}", item.unit_cost.as_major()));
                                ui.label(&item.batch_number);
                                ui.label(item.expiry_date.to_string());
                                ui.label(format!("{:.2}", item.line_cost().as_major()));
                                ui.end_row();
                            }
                        });
                });
        }
    });
}