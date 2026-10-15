//! The window: first-time setup, the login (lock) screen, and — once
//! someone is logged in — the navigation bar and the screen they picked.

use std::time::{Duration, Instant};

use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{Action, Session};
use crate::ui::theme::{self, Notice};
use crate::ui::{admin, pos, products, reports, stock};
use crate::validate;

/// The till locks itself after this long without a key press or click.
const AUTO_LOCK: Duration = Duration::from_secs(3 * 60 * 60);
/// After this many wrong PINs in a row, logging in pauses for `PAUSE`.
const MAX_TRIES: u32 = 5;
const PAUSE: Duration = Duration::from_secs(30);

const NAV_MARGIN: egui::Margin = egui::Margin { left: 20, right: 20, top: 10, bottom: 10 };
const PAGE_MARGIN: egui::Margin = egui::Margin { left: 24, right: 24, top: 16, bottom: 16 };

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Checkout,
    Medicines,
    Stock,
    Reports,
    Admin,
}

impl Screen {
    const ALL: [Screen; 5] = [Screen::Checkout, Screen::Medicines, Screen::Stock, Screen::Reports, Screen::Admin];

    fn label(self) -> &'static str {
        match self {
            Screen::Checkout => "Checkout",
            Screen::Medicines => "Medicines",
            Screen::Stock => "Stock",
            Screen::Reports => "Reports",
            Screen::Admin => "Staff & settings",
        }
    }

    /// The permission a role needs to see this screen in the menu.
    fn needs(self) -> Action {
        match self {
            Screen::Checkout => Action::Sell,
            Screen::Medicines => Action::EditProducts,
            Screen::Stock => Action::ReceiveStock,
            Screen::Reports => Action::ViewReports,
            Screen::Admin => Action::ManageUsers,
        }
    }
}

/// The login and first-time setup forms.
#[derive(Default)]
struct Login {
    user_id: String,
    pin: String,
    wrong_tries: u32,
    paused_until: Option<Instant>,
    notice: Notice,
    // First-time setup only.
    branch: String,
    name: String,
    pin_again: String,
}

pub struct MedTillApp {
    conn: Connection,
    session: Option<Session>,
    /// Who was logged in last, so their screens survive a lock/unlock but
    /// are wiped when someone else logs in (never another person's cart).
    last_user: Option<String>,
    screen: Screen,
    login: Login,
    last_input: Instant,
    pos: pos::PosState,
    products: products::ProductsState,
    stock: stock::StockState,
    reports: reports::ReportsState,
    admin: admin::AdminState,
}

impl MedTillApp {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn,
            session: None,
            last_user: None,
            screen: Screen::Checkout,
            login: Login::default(),
            last_input: Instant::now(),
            pos: Default::default(),
            products: Default::default(),
            stock: Default::default(),
            reports: Default::default(),
            admin: Default::default(),
        }
    }

    fn start(&mut self, session: Session) {
        if self.last_user.as_deref() != Some(session.user.id.as_str()) {
            self.pos = Default::default();
            self.products = Default::default();
            self.stock = Default::default();
            self.reports = Default::default();
            self.admin = Default::default();
            self.screen = if session.can(Action::ViewReports) { Screen::Reports } else { Screen::Checkout };
        }
        self.last_user = Some(session.user.id.clone());
        self.session = Some(session);
        self.login = Login::default();
        self.last_input = Instant::now();
    }

    fn lock(&mut self) {
        if let Some(session) = self.session.take() {
            self.login.user_id = session.user.id;
        }
    }

    fn setup_screen(&mut self, ui: &mut egui::Ui) {
        let l = &mut self.login;
        theme::section_title(ui, "Welcome — let's set up MedTill");
        theme::muted(ui, "You'll be the owner. You can add your staff after this.");
        theme::form(ui, "setup", |ui| {
            theme::field(ui, "Branch name", &mut l.branch, "e.g. Kiambu Road Pharmacy");
            theme::field(ui, "Your name", &mut l.name, "e.g. Kelvin");
            theme::pin_field(ui, "PIN", &mut l.pin);
            theme::pin_field(ui, "Repeat PIN", &mut l.pin_again);
        });
        if theme::big_button(ui, "Start").clicked() {
            let result = (|| -> Result<Session, String> {
                let branch = validate::parse_name("Branch name", &l.branch)?;
                let name = validate::parse_name("Your name", &l.name)?;
                let pin = validate::parse_pin(&l.pin)?;
                if l.pin != l.pin_again {
                    return Err("The two PINs are different.".into());
                }
                Ok(db::setup(&self.conn, &branch, &name, &pin)?)
            })();
            match result {
                Ok(session) => self.start(session),
                Err(e) => self.login.notice = Notice::error(e),
            }
        }
    }

    fn login_screen(&mut self, ui: &mut egui::Ui) {
        let users = match db::list_users(&self.conn, false) {
            Ok(users) => users,
            Err(e) => return Notice::error(e).show(ui),
        };
        let l = &mut self.login;
        theme::section_title(ui, "Who is at the till?");
        ui.horizontal_wrapped(|ui| {
            for user in &users {
                if theme::chip(ui, &user.name, l.user_id == user.id).clicked() {
                    l.user_id = user.id.clone();
                    l.pin.clear();
                }
            }
        });
        if l.user_id.is_empty() {
            return;
        }
        ui.add_space(8.0);
        let paused = l.paused_until.is_some_and(|until| Instant::now() < until);
        let pin_box = ui.add_enabled(
            !paused,
            egui::TextEdit::singleline(&mut l.pin)
                .password(true)
                .hint_text("Your PIN, then Enter")
                .id_salt("login_pin"),
        );
        if !paused && ui.memory(|m| m.focused().is_none()) {
            pin_box.request_focus();
        }
        let entered = pin_box.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (theme::big_button(ui, "Log in").clicked() || entered) && !paused {
            match db::login(&self.conn, &l.user_id, l.pin.trim()) {
                Ok(Some(session)) => return self.start(session),
                Ok(None) => {
                    l.wrong_tries += 1;
                    l.notice = Notice::error("Wrong PIN.");
                    if l.wrong_tries >= MAX_TRIES {
                        l.wrong_tries = 0;
                        l.paused_until = Some(Instant::now() + PAUSE);
                    }
                }
                Err(e) => l.notice = Notice::error(e),
            }
            l.pin.clear();
        }
        if let Some(until) = l.paused_until.filter(|_| paused) {
            let secs = until.saturating_duration_since(Instant::now()).as_secs() + 1;
            ui.colored_label(theme::STATUS_CRITICAL, format!("Too many wrong PINs. Try again in {secs} s."));
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
    }

    fn main_screen(&mut self, ui: &mut egui::Ui, s: &Session) {
        let mut lock = false;
        egui::Panel::top("nav").show(ui, |ui| {
            egui::Frame::NONE.inner_margin(NAV_MARGIN).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("MedTill").heading().strong().color(theme::ACCENT));
                    theme::muted(ui, "pharmacy point of sale");
                    ui.separator();
                    for screen in Screen::ALL.into_iter().filter(|screen| s.can(screen.needs())) {
                        if theme::tab(ui, screen.label(), self.screen == screen).clicked() {
                            self.screen = screen;
                            self.reports.refresh();
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        lock = ui.button("Lock").clicked();
                        let branch = db::branch_name(&self.conn).unwrap_or_default();
                        theme::muted(ui, format!("{} · {} · {branch}", s.user.name, s.user.role.label()));
                    });
                });
            });
        });
        if lock {
            return self.lock();
        }
        if !s.can(self.screen.needs()) {
            self.screen = Screen::Checkout;
        }
        egui::CentralPanel::default().show(ui, |ui| {
            egui::Frame::NONE.inner_margin(PAGE_MARGIN).show(ui, |ui| {
                let conn = &self.conn;
                match self.screen {
                    Screen::Checkout => pos::show(ui, conn, s, &mut self.pos),
                    Screen::Medicines => products::show(ui, conn, s, &mut self.products),
                    Screen::Stock => stock::show(ui, conn, s, &mut self.stock),
                    Screen::Reports => reports::show(ui, conn, s, &mut self.reports),
                    Screen::Admin => admin::show(ui, conn, s, &mut self.admin),
                }
            });
        });
    }
}

impl eframe::App for MedTillApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.input(|i| !i.events.is_empty()) {
            self.last_input = Instant::now();
        }
        if self.session.is_some() && self.last_input.elapsed() > AUTO_LOCK {
            self.lock();
        }
        // Wake up now and then even when idle, so the auto-lock can fire.
        ui.ctx().request_repaint_after(Duration::from_secs(5));

        match self.session.clone() {
            Some(session) => self.main_screen(ui, &session),
            None => {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(60.0);
                        ui.label(egui::RichText::new("MedTill").size(40.0).strong().color(theme::ACCENT));
                        theme::muted(ui, db::branch_name(&self.conn).unwrap_or_default());
                        ui.add_space(16.0);
                        ui.scope(|ui| {
                            ui.set_max_width(560.0);
                            theme::panel(ui, |ui| {
                                match db::needs_setup(&self.conn) {
                                    Ok(true) => self.setup_screen(ui),
                                    Ok(false) => self.login_screen(ui),
                                    Err(e) => Notice::error(e).show(ui),
                                }
                                self.login.notice.show(ui);
                            });
                        });
                    });
                });
            }
        }
    }
}
