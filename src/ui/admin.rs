//! Staff and settings (owner only): add staff, set their role and PIN,
//! block someone from logging in, and name the branch.

use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{Role, Session, User};
use crate::ui::theme::{self, Notice};
use crate::validate;

const FORM_WIDTH: f32 = 470.0;

#[derive(Default)]
pub struct AdminState {
    /// Id of the person being edited (empty: adding someone new).
    id: String,
    name: String,
    role: Role,
    pin: String,
    blocked: bool,
    // Branch and receipt settings, loaded from the database once.
    loaded: bool,
    branch: String,
    header: String,
    footer: String,
    print: bool,
    notice: Notice,
}

pub fn show(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut AdminState) {
    ui.heading("Staff & settings");
    let users = match db::list_users(conn, true) {
        Ok(users) => users,
        Err(e) => return Notice::error(e).show(ui),
    };
    if !st.loaded {
        st.loaded = true;
        st.branch = db::branch_name(conn).unwrap_or_default();
        st.header = db::setting(conn, "receipt_header").unwrap_or_default();
        st.footer = db::setting(conn, "receipt_footer").unwrap_or_default();
        st.print = db::setting(conn, "print_receipts").is_ok_and(|v| v == "1");
    }
    theme::two_columns(
        ui,
        "admin",
        FORM_WIDTH,
        st,
        |ui, st| {
            staff_form(ui, conn, s, st);
            ui.add_space(16.0);
            branch_form(ui, conn, s, st);
        },
        |ui, st| staff_list(ui, st, &users),
    );
}

fn staff_form(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut AdminState) {
    let editing = !st.id.is_empty();
    theme::section_title(ui, if editing { "Edit staff member" } else { "Add staff member" });
    theme::panel(ui, |ui| {
        theme::form(ui, "staff_form", |ui| {
            theme::field(ui, "Name", &mut st.name, "e.g. Ann Wanjiru");
            theme::choice(ui, "Role", "staff_role", &mut st.role, &Role::ALL.map(|r| (r, r.label())), "");
            theme::pin_field(ui, if editing { "New PIN" } else { "PIN" }, &mut st.pin);
            ui.label("");
            ui.checkbox(&mut st.blocked, "Blocked (can't log in)");
            ui.end_row();
        });
        theme::muted(ui, format!("A {} can: {}", st.role.label().to_lowercase(), can_do(st.role)));
        if editing {
            theme::muted(ui, "Leave the PIN empty to keep their current one.");
        }
        theme::actions(ui, |ui| {
            if theme::primary_button(ui, if editing { "Save changes" } else { "Add staff member" }).clicked() {
                st.notice = save(conn, s, st).into();
                if st.notice.is_success() {
                    *st = AdminState { notice: std::mem::take(&mut st.notice), ..Default::default() };
                }
            }
            if editing && theme::quiet_button(ui, "Cancel").clicked() {
                *st = AdminState::default();
            }
        });
        st.notice.show(ui);
    });
}

fn can_do(role: Role) -> &'static str {
    match role {
        Role::Owner => "do everything, including managing staff.",
        Role::Manager => "sell, void sales, receive and adjust stock, edit medicines and prices, see reports.",
        Role::Pharmacist => "sell, send orders to the cashier, take payment and check stock in.",
        Role::Cashier => "sell, send orders and take payment.",
    }
}

fn save(conn: &Connection, s: &Session, st: &AdminState) -> Result<String, String> {
    let pin = match st.pin.trim() {
        "" => None,
        pin => Some(validate::parse_pin(pin)?),
    };
    let user =
        User { id: st.id.clone(), name: validate::parse_name("Name", &st.name)?, role: st.role, active: !st.blocked };
    db::save_user(conn, s, &user, pin.as_deref())?;
    Ok(format!("Saved {}.", user.name))
}

fn branch_form(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut AdminState) {
    theme::section_title(ui, "This branch and its receipts");
    theme::panel(ui, |ui| {
        theme::form(ui, "branch_form", |ui| {
            theme::field(ui, "Branch name", &mut st.branch, "printed at the top of receipts");
            theme::field(ui, "Receipt header", &mut st.header, "e.g. Tel 0712 345 678 · Kiambu Road");
            theme::field(ui, "Receipt footer", &mut st.footer, "e.g. Thank you. Get well soon!");
            ui.label("");
            ui.checkbox(&mut st.print, "Print a receipt after every sale");
            ui.end_row();
        });
        theme::actions(ui, |ui| {
            if theme::primary_button(ui, "Save settings").clicked() {
                st.notice = save_branch(conn, s, st).into();
            }
        });
    });
}

fn save_branch(conn: &Connection, s: &Session, st: &AdminState) -> Result<String, String> {
    let name = validate::parse_name("Branch name", &st.branch)?;
    db::rename_branch(conn, s, &name)?;
    let print = if st.print { "1" } else { "0" };
    let settings =
        [("receipt_header", st.header.trim()), ("receipt_footer", st.footer.trim()), ("print_receipts", print)];
    db::save_settings(conn, s, &settings)?;
    Ok("Branch and receipt settings saved.".into())
}

fn staff_list(ui: &mut egui::Ui, st: &mut AdminState, users: &[User]) {
    theme::section_title(ui, &format!("Staff ({})", users.len()));
    theme::list(ui, "staff_list", &["Name", "Role", "Status", ""], |ui| {
        for user in users {
            ui.label(&user.name);
            ui.label(user.role.label());
            if user.active {
                ui.colored_label(theme::STATUS_GOOD, "Active");
            } else {
                ui.colored_label(theme::STATUS_CRITICAL, "Blocked");
            }
            if ui.button("Edit").clicked() {
                *st = AdminState {
                    id: user.id.clone(),
                    name: user.name.clone(),
                    role: user.role,
                    blocked: !user.active,
                    ..Default::default()
                };
            }
            ui.end_row();
        }
    });
}
