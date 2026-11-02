//! Where MedTill keeps its files, and backups.
//!
//! - **The database** lives in `AppData\Local\MedTill\medtill.sqlite`, out of
//!   the way of updates and tidying up. An older `medtill.sqlite` beside the
//!   program is copied there on the first start (the old file is renamed,
//!   never deleted). MedTill then works from that folder, so receipts and
//!   training examples land there too.
//! - **Backups** are full, checked copies of the database in
//!   `Documents\MedTill backups` (or a folder chosen in Staff & settings,
//!   such as a USB stick). One is made automatically every Friday — on the
//!   next start if the computer was off that day — and the last
//!   `KEEP_AUTO` automatic ones are kept. Backups made with the button are
//!   never deleted.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::{Datelike, Local, NaiveDate, Weekday};
use rusqlite::{Connection, OpenFlags};

use crate::db;

pub const DB_FILE: &str = "medtill.sqlite";

/// How many automatic backups are kept (about two months of Fridays).
const KEEP_AUTO: usize = 10;

/// The folder MedTill was started from (where `cargo run` keeps `models`).
static START_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn start_dir() -> &'static Path {
    START_DIR.get_or_init(|| std::env::current_dir().unwrap_or_default())
}

/// `AppData\Local\MedTill` (on Linux, `~/.local/share/MedTill`).
pub fn data_dir() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share"));
    local.or(home).unwrap_or_else(|| start_dir().to_path_buf()).join("MedTill")
}

/// The full path of the database.
pub fn db_path() -> PathBuf {
    data_dir().join(DB_FILE)
}

/// Documents (which may live under OneDrive), or the data folder if there is none.
pub fn documents_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    let documents = home.iter().flat_map(|h| [h.join("Documents"), h.join("OneDrive").join("Documents")]);
    documents.into_iter().find(|d| d.is_dir()).unwrap_or_else(data_dir)
}

/// Gets the data folder ready and moves into it: creates it, copies an older
/// database found beside the program, and makes it the working folder.
/// Returns a note for the person at the till when a database was moved.
pub fn prepare() -> Result<Option<String>, String> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let mut note = None;
    let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
    let mut old = [Some(start_dir().to_path_buf()), exe_dir].into_iter().flatten().map(|d| d.join(DB_FILE));
    let target = db_path();
    if !target.exists()
        && let Some(old) = old.find(|p| p.is_file())
    {
        copy_database(&old, &target)?;
        // Renamed, not deleted: the old copy stays as a spare.
        let _ = std::fs::rename(&old, old.with_extension("sqlite.moved"));
        note = Some(format!("Your data was moved to {}.", target.display()));
    }
    std::env::set_current_dir(&dir).map_err(|e| format!("Could not open {}: {e}", dir.display()))?;
    Ok(note)
}

/// A consistent copy of a database that may be in use (`VACUUM INTO` also
/// takes in what is still in the -wal file), checked before it counts.
fn copy_database(from: &Path, to: &Path) -> Result<(), String> {
    let src = Connection::open(from).map_err(|e| format!("Could not open {}: {e}", from.display()))?;
    src.execute("VACUUM INTO ?1", [to.to_string_lossy()]).map_err(|e| format!("Could not copy the database: {e}"))?;
    let check = Connection::open_with_flags(to, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0)));
    match check {
        Ok(ok) if ok == "ok" => Ok(()),
        other => {
            let _ = std::fs::remove_file(to);
            Err(format!("The copy failed its check ({other:?}); nothing was changed."))
        }
    }
}

// ---------- Backups ----------

/// Where backups go: the folder set in Staff & settings, else Documents\MedTill backups.
pub fn backup_folder(conn: &Connection) -> PathBuf {
    match db::setting(conn, "backup_folder").unwrap_or_default().trim() {
        "" => documents_dir().join("MedTill backups"),
        folder => PathBuf::from(folder),
    }
}

/// When the last backup was made ("2026-10-16 18:02"), if ever.
pub fn last_backup(conn: &Connection) -> String {
    db::setting(conn, "backup_last").unwrap_or_default()
}

/// Makes a checked copy of the database now. `auto` backups are named so
/// they can be thinned out; ones made with the button are kept.
pub fn backup(conn: &Connection, auto: bool) -> Result<PathBuf, String> {
    let folder = backup_folder(conn);
    std::fs::create_dir_all(&folder).map_err(|e| format!("Could not open {}: {e}", folder.display()))?;
    let kind = if auto { "auto" } else { "manual" };
    let stamp = Local::now().format("%Y-%m-%d_%H%M");
    let path = (1..)
        .map(|n| match n {
            1 => folder.join(format!("medtill-{kind}-{stamp}.sqlite")),
            n => folder.join(format!("medtill-{kind}-{stamp}-{n}.sqlite")),
        })
        .find(|p| !p.exists())
        .unwrap_or_default();
    conn.execute("VACUUM INTO ?1", [path.to_string_lossy()]).map_err(|e| format!("Backup failed: {e}"))?;
    let check = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|c| c.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0)));
    if !matches!(check.as_deref(), Ok("ok")) {
        let _ = std::fs::remove_file(&path);
        return Err("The backup copy failed its check and was removed. Try again.".into());
    }
    db::put_setting(conn, "backup_last", &Local::now().format("%Y-%m-%d %H:%M").to_string())?;
    if auto {
        thin_out(&folder);
    }
    Ok(path)
}

/// Keeps only the newest `KEEP_AUTO` automatic backups (their names sort by date).
fn thin_out(folder: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    let mut autos: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("medtill-auto-")))
        .collect();
    autos.sort();
    let extra = autos.len().saturating_sub(KEEP_AUTO);
    for old in &autos[..extra] {
        let _ = std::fs::remove_file(old);
    }
}

/// True when the automatic backup should run: it's Friday and none was
/// made today, or the last one is a week old (the computer was off on Friday).
pub fn backup_due(last: &str, today: NaiveDate) -> bool {
    let last = last.get(..10).and_then(|d| d.parse::<NaiveDate>().ok());
    match last {
        None => true,
        Some(last) => (today.weekday() == Weekday::Fri && last < today) || (today - last).num_days() >= 7,
    }
}

/// Runs the Friday backup if it is due. `None` when nothing was due.
pub fn auto_backup(conn: &Connection) -> Option<Result<PathBuf, String>> {
    backup_due(&last_backup(conn), Local::now().date_naive()).then(|| backup(conn, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friday_backups() {
        let day = |d: &str| d.parse::<NaiveDate>().unwrap();
        // 2026-10-16 is a Friday.
        assert!(backup_due("", day("2026-10-14")), "the very first one runs at once");
        assert!(backup_due("2026-10-09 18:00", day("2026-10-16")), "Friday");
        assert!(!backup_due("2026-10-16 08:00", day("2026-10-16")), "once per Friday");
        assert!(!backup_due("2026-10-16 08:00", day("2026-10-20")), "not again before next Friday");
        assert!(backup_due("2026-10-09 08:00", day("2026-10-17")), "Friday missed: the next start catches up");
        assert!(!backup_due("2026-10-17 08:00", day("2026-10-22")));
    }

    #[test]
    fn backup_is_a_checked_copy() {
        let dir = std::env::temp_dir().join(format!("medtill-backup-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let conn = db::open(":memory:").unwrap();
        db::setup(&conn, "Town", "Kelvin", "1234").unwrap();
        db::put_setting(&conn, "backup_folder", &dir.to_string_lossy()).unwrap();
        for _ in 0..12 {
            backup(&conn, true).unwrap();
        }
        let manual = backup(&conn, false).unwrap();
        let copy = Connection::open(&manual).unwrap();
        assert!(!db::needs_setup(&copy).unwrap(), "the copy has the owner");
        let files = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(files, KEEP_AUTO + 1, "old automatic backups are thinned out; manual ones stay");
        assert!(!backup_due(&last_backup(&conn), Local::now().date_naive()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
