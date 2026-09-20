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

impl eframe::App for MedTillApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        Panel::top("nav").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("MedTill");
                ui.label(egui::RichText::new("pharmacy point of sale").color(ui::theme::TEXT_SECONDARY));
                ui.separator();
                ui.selectable_value(&mut self.screen, Screen::Dashboard, "Dashboard");
                ui.selectable_value(&mut self.screen, Screen::Pos, "Checkout");
                ui.selectable_value(&mut self.screen, Screen::Inventory, "Inventory");
                ui.selectable_value(&mut self.screen, Screen::Suppliers, "Suppliers");
                ui.selectable_value(&mut self.screen, Screen::History, "History");
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ui, |ui| match self.screen {
            Screen::Dashboard => crate::ui::dashboard::show(ui, &self.conn, &mut self.dashboard),
            Screen::Pos => crate::ui::pos::show(ui, &mut self.conn, &mut self.pos),
            Screen::Inventory => crate::ui::inventory::show(ui, &mut self.conn, &mut self.inventory),
            Screen::Suppliers => crate::ui::suppliers::show(ui, &mut self.conn, &mut self.suppliers),
            Screen::History => crate::ui::history::show(ui, &mut self.conn, &mut self.history),
        });
    }
}