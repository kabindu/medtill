//! Staff and settings (owner only): add staff, change anyone's name, role
//! or PIN (other owners too), block or delete them, and name the branch.
//! Nobody can block or delete themselves, and one active owner always remains.
//! Also here: backups, and linking other computers (see `sync`).

use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{Role, Session, User};
use crate::storage;
use crate::sync::{self, Role as SyncRole};
use crate::ui::theme::{self, Notice, RowClick};
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
    confirm_delete: Option<String>,
    backup_folder: String,
    last_backup: Option<std::path::PathBuf>,
    backup_notice: Notice,
    sync_notice: Notice,
}

pub fn show(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut AdminState, sync: &sync::Shared) {
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
        st.backup_folder = db::setting(conn, "backup_folder").unwrap_or_default();
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
            ui.add_space(16.0);
            backups(ui, conn, st);
            ui.add_space(16.0);
            computers(ui, conn, st, sync);
        },
        |ui, st| staff_list(ui, conn, s, st, &users),
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

fn staff_list(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut AdminState, users: &[User]) {
    theme::section_title(ui, &format!("Staff ({})", users.len()));
    let mut clicked = None;
    theme::list(ui, "staff_list", &["Name", "Role", "Status", ""], |ui| {
        for user in users {
            let me = user.id == s.user.id;
            ui.label(if me { format!("{} (you)", user.name) } else { user.name.clone() });
            ui.label(user.role.label());
            if user.active {
                ui.colored_label(theme::STATUS_GOOD, "Active");
            } else {
                ui.colored_label(theme::STATUS_CRITICAL, "Blocked");
            }
            ui.horizontal(|ui| {
                if me {
                    // Your own row: edit only (no blocking or deleting yourself).
                    if ui.button("Edit").clicked() {
                        clicked = Some((user, RowClick::Edit));
                    }
                    return;
                }
                if theme::quiet_button(ui, if user.active { "Block" } else { "Unblock" }).clicked() {
                    clicked = Some((user, RowClick::None));
                }
                match theme::edit_delete(ui, &user.id, &mut st.confirm_delete) {
                    RowClick::None => {}
                    click => clicked = Some((user, click)),
                }
            });
            ui.end_row();
        }
    });
    match clicked {
        Some((user, RowClick::Edit)) => {
            *st = AdminState {
                id: user.id.clone(),
                name: user.name.clone(),
                role: user.role,
                blocked: !user.active,
                ..Default::default()
            };
        }
        Some((user, RowClick::Delete)) => {
            st.notice =
                db::delete_user(conn, s, user).map(|()| format!("Deleted {}.", user.name)).map_err(String::from).into();
        }
        // The Block / Unblock button.
        Some((user, RowClick::None)) => {
            let flipped = User { active: !user.active, ..user.clone() };
            let word = if flipped.active { "unblocked: they can log in again" } else { "blocked: they can't log in" };
            st.notice = db::save_user(conn, s, &flipped, None)
                .map(|()| format!("{} is {word}.", user.name))
                .map_err(String::from)
                .into();
        }
        None => {}
    }
}

fn backups(ui: &mut egui::Ui, conn: &Connection, st: &mut AdminState) {
    theme::section_title(ui, "Backups");
    theme::panel(ui, |ui| {
        let last = storage::last_backup(conn);
        theme::muted(
            ui,
            match last.is_empty() {
                true => "No backup yet.".to_string(),
                false => {
                    format!("Last backup: {last}. A new one is made every Friday (or at the next start if missed).")
                }
            },
        );
        theme::form(ui, "backup_form", |ui| {
            let default = storage::documents_dir().join("MedTill backups");
            let hint = format!("empty = {}", default.display());
            theme::field(ui, "Backup folder", &mut st.backup_folder, &hint);
        });
        theme::muted(ui, "Best: a USB stick or another drive, so a broken computer doesn't take the backups with it.");
        theme::actions(ui, |ui| {
            if theme::primary_button(ui, "Back up now").clicked() {
                let saved = db::put_setting(conn, "backup_folder", st.backup_folder.trim()).map_err(String::from);
                st.backup_notice = match saved.and_then(|()| storage::backup(conn, false)) {
                    Ok(path) => {
                        let note = format!("Backed up to {}", path.display());
                        st.last_backup = Some(path);
                        Notice::success(note)
                    }
                    Err(e) => Notice::error(e),
                };
            }
            if theme::quiet_button(ui, "Open backups folder").clicked() {
                let _ = std::process::Command::new("explorer").arg(storage::backup_folder(conn)).spawn();
            }
        });
        st.backup_notice.show(ui);
    });
}

/// Linking other computers: make this the main one, or see how the link is doing.
fn computers(ui: &mut egui::Ui, conn: &Connection, st: &mut AdminState, status: &sync::Shared) {
    theme::section_title(ui, "Other computers");
    theme::panel(ui, |ui| {
        let (till, main, link) = sync::details(conn);
        let (last_ok, problem, mut online) = match status.lock() {
            Ok(s) => (s.last_ok, s.problem.clone(), s.tills.iter().map(|(t, at)| (*t, *at)).collect::<Vec<_>>()),
            Err(_) => (None, None, Vec::new()),
        };
        let ago = |at: std::time::Instant| format!("{} s ago", at.elapsed().as_secs());
        match sync::role(conn) {
            SyncRole::Alone => {
                theme::muted(
                    ui,
                    "Every computer in the shop can be a full till that keeps selling when the network is down. \
                     Make this the main computer, then choose \"Join a shop\" when setting up MedTill on the others.",
                );
                if theme::primary_button(ui, "Make this the main computer").clicked() {
                    st.sync_notice = match sync::make_main(conn) {
                        Ok(_) => {
                            sync::start(conn, storage::db_path(), status);
                            Notice::success(
                                "This is now the main computer. Allow MedTill through the Windows firewall if asked.",
                            )
                        }
                        Err(e) => Notice::error(e),
                    };
                }
            }
            SyncRole::Main => {
                theme::form(ui, "main_computer", |ui| {
                    ui.label("Main computer");
                    ui.label(egui::RichText::new(sync::this_computer()).strong());
                    ui.end_row();
                    ui.label("Link code");
                    ui.label(egui::RichText::new(&link).strong().size(22.0));
                    ui.end_row();
                });
                online.sort();
                if online.is_empty() {
                    theme::muted(ui, "No other computer has connected yet.");
                }
                for (till, at) in online {
                    theme::muted(ui, format!("Till {till}: last heard from {}", ago(at)));
                }
                if let Some(problem) = problem {
                    ui.colored_label(theme::STATUS_CRITICAL, problem);
                }
            }
            SyncRole::Till => {
                theme::muted(ui, format!("This is till {till}, linked to {main}."));
                match (last_ok, problem) {
                    (_, Some(problem)) => ui.colored_label(theme::STATUS_SERIOUS, problem),
                    (Some(at), None) => ui.colored_label(theme::STATUS_GOOD, format!("Synced {}", ago(at))),
                    (None, None) => ui.label("Connecting..."),
                };
                if ui.button("Sync now").clicked()
                    && let Ok(mut s) = status.lock()
                {
                    s.wake = true;
                }
            }
        }
        st.sync_notice.show(ui);
    });
}
