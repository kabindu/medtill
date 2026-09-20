use eframe::egui;
use egui_plot::{Bar, BarChart, Plot};
use rusqlite::Connection;

use crate::db;
use crate::ui::notify::Notice;
use crate::ui::theme;

#[derive(Default)]
pub struct DashboardState {
    pub status: Notice,
}

const TREND_DAYS: i64 = 14;
const LOW_STOCK_CRITICAL_AT: i64 = 3;
const EXPIRY_WINDOW_DAYS: i64 = 30;
const TOP_PRODUCTS_LIMIT: i64 = 5;

pub fn show(ui: &mut egui::Ui, conn: &Connection, state: &mut DashboardState) {
    ui.heading("Dashboard");
    ui.add_space(6.0);

    render_stat_row(ui, conn, state);
    ui.add_space(16.0);

    ui.columns(2, |columns| {
        render_sales_trend(&mut columns[0], conn, state);
        render_top_products(&mut columns[1], conn, state);
    });

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

    ui.horizontal(|ui| {
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

fn render_sales_trend(ui: &mut egui::Ui, conn: &Connection, state: &mut DashboardState) {
    theme::card(ui, |ui| {
        ui.label(egui::RichText::new(format!("Sales — last {TREND_DAYS} days")).strong());

        let daily = match db::sales_totals_last_n_days(conn, TREND_DAYS) {
            Ok(daily) => daily,
            Err(err) => {
                state.status = Notice::error(format!("Could not load sales trend: {err}"));
                return;
            }
        };

        let bars: Vec<Bar> = daily
            .iter()
            .enumerate()
            .map(|(i, (date, total))| {
                Bar::new(i as f64, total.as_major())
                    .name(date.format("%b %d").to_string())
                    .width(0.6)
            })
            .collect();

        let chart = BarChart::new("daily_sales", bars).color(theme::ACCENT);

        Plot::new("sales_trend_plot")
            .height(220.0)
            .show_x(false)
            .allow_zoom(false)
            .allow_drag(false)
            .allow_scroll(false)
            .show(ui, |plot_ui| {
                plot_ui.bar_chart(chart);
            });
    });
}

fn render_top_products(ui: &mut egui::Ui, conn: &Connection, state: &mut DashboardState) {
    theme::card(ui, |ui| {
        ui.label(egui::RichText::new(format!("Top {TOP_PRODUCTS_LIMIT} products by units sold")).strong());

        let top = match db::top_products_by_quantity(conn, TOP_PRODUCTS_LIMIT) {
            Ok(top) => top,
            Err(err) => {
                state.status = Notice::error(format!("Could not load top products: {err}"));
                return;
            }
        };

        if top.is_empty() {
            ui.label("No sales recorded yet.");
            return;
        }

        let bars: Vec<Bar> = top
            .iter()
            .enumerate()
            .map(|(i, (name, qty))| Bar::new(i as f64, *qty as f64).name(name.clone()).width(0.6))
            .collect();

        let chart = BarChart::new("top_products", bars).color(theme::ACCENT);

        Plot::new("top_products_plot")
            .height(220.0)
            .show_x(false)
            .allow_zoom(false)
            .allow_drag(false)
            .allow_scroll(false)
            .show(ui, |plot_ui| {
                plot_ui.bar_chart(chart);
            });

        ui.add_space(6.0);
        for (i, (name, qty)) in top.iter().enumerate() {
            ui.label(format!("{}. {name} — {qty} units", i + 1));
        }
    });
}