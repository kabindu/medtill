use eframe::egui;
use egui::Panel;
use rusqlite::Connection;

use crate::db;
use crate::ui;

#[derive(Default, PartialEq, Clone, Copy)]
pub enum Screen {
    #[default]
    Dashboard,
    Pos,
    Inventory,
    Suppliers,
    History,
}

pub struct MedTillApp {
    pub screen: Screen,
    pub conn: Connection,
    pub dashboard: ui::dashboard::DashboardState,
    pub pos: ui::pos::PosState,
    pub inventory: ui::inventory::InventoryState,
    pub suppliers: ui::suppliers::SuppliersState,
    pub history: ui::history::HistoryState,
}

impl MedTillApp {
    pub fn new() -> rusqlite::Result<Self> {
        let conn = db::init_db("medtill.sqlite")?;
        Ok(Self {
            screen: Screen::default(),
            conn,
            dashboard: ui::dashboard::DashboardState::default(),
            pos: ui::pos::PosState::default(),
            inventory: ui::inventory::InventoryState::default(),
            suppliers: ui::suppliers::SuppliersState::default(),
            history: ui::history::HistoryState::default(),
        })
    }
}

/// Breathing room around the nav bar and every screen, so nothing sits
/// hard against the window edge.
const NAV_MARGIN: egui::Margin = egui::Margin { left: 20, right: 20, top: 10, bottom: 10 };
const PAGE_MARGIN: egui::Margin = egui::Margin { left: 24, right: 24, top: 16, bottom: 16 };

impl eframe::App for MedTillApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        Panel::top("nav").show(ui, |ui| {
            egui::Frame::NONE.inner_margin(NAV_MARGIN).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new("MedTill")
                            .heading()
                            .strong()
                            .color(ui::theme::ACCENT),
                    );
                    ui.label(egui::RichText::new("pharmacy point of sale").color(ui::theme::TEXT_SECONDARY));
                    ui.separator();
                    ui.selectable_value(&mut self.screen, Screen::Dashboard, "Dashboard");
                    ui.selectable_value(&mut self.screen, Screen::Pos, "Checkout");
                    ui.selectable_value(&mut self.screen, Screen::Inventory, "Inventory");
                    ui.selectable_value(&mut self.screen, Screen::Suppliers, "Suppliers");
                    ui.selectable_value(&mut self.screen, Screen::History, "History");
                });
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::Frame::NONE.inner_margin(PAGE_MARGIN).show(ui, |ui| match self.screen {
                Screen::Dashboard => crate::ui::dashboard::show(ui, &self.conn, &mut self.dashboard),
                Screen::Pos => crate::ui::pos::show(ui, &mut self.conn, &mut self.pos),
                Screen::Inventory => crate::ui::inventory::show(ui, &mut self.conn, &mut self.inventory),
                Screen::Suppliers => crate::ui::suppliers::show(ui, &mut self.conn, &mut self.suppliers),
                Screen::History => crate::ui::history::show(ui, &mut self.conn, &mut self.history),
            });
        });
    }
}