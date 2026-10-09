use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::ui::notify::Notice;
use crate::ui::theme;

#[derive(Default)]
pub struct DashboardState {
    pub status: Notice,
}

const LOW_STOCK_CRITICAL_AT: i64 = 3;
const EXPIRY_WINDOW_DAYS: i64 = 30;

pub fn show(ui: &mut egui::Ui, conn: &Connection, state: &mut DashboardState) {
    ui.heading("Dashboard");
    ui.add_space(6.0);

    render_stat_row(ui, conn, state);

    state.status.show(ui);
}

fn render_stat_row(ui: &mut egui::Ui, conn: &Connection, state: &mut DashboardState) {
    let today_total = match db::today_sales_total(conn) {
        Ok(total) => total,
        Err(err) => {
            state.status = Notice::error(format!("Could not load today's sales: {err}"));
            crate::models::Cents(0)
        }
    };
    let low_stock = db::count_low_stock(conn).unwrap_or(0);
    let expiring_soon = db::count_expiring_soon(conn, EXPIRY_WINDOW_DAYS).unwrap_or(0);
    let total_products = db::count_products(conn).unwrap_or(0);

    let low_stock_color = if low_stock == 0 {
        theme::STATUS_GOOD
    } else if low_stock >= LOW_STOCK_CRITICAL_AT {
        theme::STATUS_CRITICAL
    } else {
        theme::STATUS_WARNING
    };
    let expiring_color = if expiring_soon == 0 {
        theme::STATUS_GOOD
    } else {
        theme::STATUS_SERIOUS
    };

    // Wrapped so the four (now very large) tiles move onto a second row
    // instead of running off the edge of the window.
    ui.horizontal_wrapped(|ui| {
        theme::stat_tile(ui, "Today's sales", &format!("{:.2}", today_total.as_major()), theme::ACCENT);
        theme::stat_tile(ui, "Low stock", &low_stock.to_string(), low_stock_color);
        theme::stat_tile(
            ui,
            &format!("Expiring within {EXPIRY_WINDOW_DAYS}d"),
            &expiring_soon.to_string(),
            expiring_color,
        );
        theme::stat_tile(ui, "Total products", &total_products.to_string(), theme::TEXT_PRIMARY);
    });
}