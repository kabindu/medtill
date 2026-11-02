//! Several computers in one shop, each a full till that keeps working when
//! the network is down.
//!
//! How it works:
//! - One computer is the **main** one. The others **join** it once (its
//!   name or address and a link code) and become till 2, 3, ... Each
//!   computer has the whole database.
//! - Triggers note every change to a shared table in `sync_log`. Every 10 s
//!   a till sends the main computer its changes (**push**) and fetches
//!   everyone else's (**pull**). The main computer keeps one log for all,
//!   so a change from till 2 reaches till 3 through it.
//! - Rows are copied whole, newest arrival wins. Ids are random, so new
//!   rows never clash. Stock is a ledger of moves, so sales made on two
//!   tills at once simply add up. Three cases get a rule of their own:
//!   a sale's status only moves forward (open → cancelled → done → void),
//!   deleted rows are sent as "gone", and a medicine code taken on both
//!   sides is given the next free code.
//! - Nothing leaves the shop: the computers talk to each other on the local
//!   network only (TCP port `PORT`), and every request carries the link code.
//!
//! The sync's own SQL lives here; everything else is in `db`.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db;

pub const PORT: u16 = 47800;
const EVERY: Duration = Duration::from_secs(10);
/// Changes sent per request.
const BATCH: i64 = 1000;

/// The tables shared between computers, with their key columns.
const SHARED: [(&str, &[&str]); 13] = [
    ("settings", &["key"]),
    ("branches", &["id"]),
    ("users", &["id"]),
    ("suppliers", &["id"]),
    ("products", &["id"]),
    ("deliveries", &["id"]),
    ("batches", &["id"]),
    ("stock_moves", &["id"]),
    ("shifts", &["id"]),
    ("sales", &["id"]),
    ("sale_items", &["id"]),
    ("audit", &["id"]),
    ("box_words", &["product_id", "word"]),
];

/// Settings that belong to one computer (its printer, its backups).
const LOCAL_SETTINGS: [&str; 4] = ["branch_id", "print_receipts", "backup_folder", "backup_last"];

// ---------- The change log (used by `db::migrate`) ----------

/// `NEW.a || char(31) || NEW.b`: a row's key as one text.
fn key_expr(row: &str, cols: &[&str]) -> String {
    cols.iter().map(|c| format!("{row}\"{c}\"")).collect::<Vec<_>>().join(" || char(31) || ")
}

fn local_settings() -> String {
    LOCAL_SETTINGS.map(|k| format!("'{k}'")).join(", ")
}

/// Triggers that note each insert, update and delete in `sync_log`, unless
/// the change itself came from another computer (`mute`).
pub fn log_triggers() -> String {
    let mut sql = String::new();
    for (table, cols) in SHARED {
        for (event, row, gone) in [("INSERT", "NEW.", 0), ("UPDATE", "NEW.", 0), ("DELETE", "OLD.", 1)] {
            let key = key_expr(row, cols);
            let local = match table {
                "settings" => format!(" AND {row}key NOT IN ({})", local_settings()),
                _ => String::new(),
            };
            sql += &format!(
                "CREATE TRIGGER sync_{table}_{event} AFTER {event} ON {table}
                 WHEN (SELECT v FROM sync_state WHERE k = 'mute') = '0'{local} BEGIN
                     DELETE FROM sync_log WHERE tbl = '{table}' AND key = {key};
                     INSERT INTO sync_log (tbl, key, gone) VALUES ('{table}', {key}, {gone});
                 END;\n"
            );
        }
    }
    sql
}

/// Notes every row already there, so a computer joining later gets them all.
pub fn log_everything() -> String {
    SHARED
        .iter()
        .map(|(table, cols)| {
            let only =
                if *table == "settings" { format!(" WHERE key NOT IN ({})", local_settings()) } else { "".into() };
            format!("INSERT INTO sync_log (tbl, key) SELECT '{table}', {} FROM {table}{only};\n", key_expr("", cols))
        })
        .collect()
}

// ---------- This computer's sync settings (never shared) ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The only computer.
    Alone,
    /// Other computers join this one.
    Main,
    /// Joined a main computer.
    Till,
}

fn state(conn: &Connection, k: &str) -> rusqlite::Result<String> {
    Ok(conn.query_row("SELECT v FROM sync_state WHERE k = ?1", [k], |r| r.get(0)).optional()?.unwrap_or_default())
}

fn set_state(conn: &Connection, k: &str, v: &str) -> rusqlite::Result<()> {
    conn.execute("INSERT OR REPLACE INTO sync_state (k, v) VALUES (?1, ?2)", [k, v]).map(|_| ())
}

fn number(conn: &Connection, k: &str) -> rusqlite::Result<i64> {
    Ok(state(conn, k)?.parse().unwrap_or(0))
}

pub fn role(conn: &Connection) -> Role {
    match state(conn, "role").as_deref() {
        Ok("main") => Role::Main,
        Ok("till") => Role::Till,
        _ => Role::Alone,
    }
}

/// This computer's till number, the main computer's name, and the link code.
pub fn details(conn: &Connection) -> (i64, String, String) {
    (
        number(conn, "till").unwrap_or(1).max(1),
        state(conn, "main").unwrap_or_default(),
        state(conn, "link").unwrap_or_default(),
    )
}

/// Makes this computer the main one, with a new link code.
pub fn make_main(conn: &Connection) -> Result<String, String> {
    let link: String = conn.query_row("SELECT upper(hex(randomblob(4)))", [], |r| r.get(0)).map_err(err)?;
    set_state(conn, "role", "main").map_err(err)?;
    set_state(conn, "link", &link).map_err(err)?;
    if state(conn, "next_till").map_err(err)?.is_empty() {
        set_state(conn, "next_till", "2").map_err(err)?;
    }
    Ok(link)
}

fn err(e: impl std::fmt::Display) -> String {
    format!("Sync: {e}")
}

/// What other computers type to reach this one: its name and its address
/// on the shop network ("DESKTOP-2VPR9IB or 192.168.1.20").
pub fn this_computer() -> String {
    let name = std::env::var("COMPUTERNAME").unwrap_or_default();
    // Connecting a UDP socket sends nothing; it only picks the network card.
    let ip = UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("10.255.255.255:1").and_then(|()| s.local_addr()))
        .map(|a| a.ip().to_string())
        .unwrap_or_default();
    [name, ip].into_iter().filter(|s| !s.is_empty() && s != "0.0.0.0").collect::<Vec<_>>().join(" or ")
}

// ---------- Changes ----------

/// One change to send: a whole row, or a row that was deleted.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Row { table: String, values: Vec<(String, Value)> },
    Gone { table: String, key: String },
}

fn shared(table: &str) -> Result<&'static [&'static str], String> {
    SHARED.iter().find(|(t, _)| *t == table).map(|(_, cols)| *cols).ok_or_else(|| format!("unknown table {table}"))
}

/// Changes logged after `after`, oldest first, leaving out the ones that
/// came from till `skip`. Returns them, the last log number looked at, and
/// whether more are waiting.
pub fn collect(conn: &Connection, after: i64, skip: Option<i64>) -> Result<(Vec<Change>, i64, bool), String> {
    let mut stmt = conn
        .prepare("SELECT seq, tbl, key, gone, origin FROM sync_log WHERE seq > ?1 ORDER BY seq LIMIT ?2")
        .map_err(err)?;
    let entries = stmt
        .query_map(params![after, BATCH], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get(3)?, r.get(4)?))
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<(i64, String, String, bool, Option<i64>)>>>)
        .map_err(err)?;
    let more = entries.len() as i64 == BATCH;
    let last = entries.last().map_or(after, |e| e.0);
    let mut changes = Vec::new();
    for (_, table, key, gone, origin) in entries {
        if skip.is_some() && origin == skip {
            continue;
        }
        let cols = shared(&table)?;
        let row = match gone {
            true => None,
            false => {
                let sql = format!("SELECT * FROM {table} WHERE {} = ?1", key_expr("", cols));
                let mut stmt = conn.prepare(&sql).map_err(err)?;
                let names: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
                stmt.query_row([&key], |r| {
                    names.iter().enumerate().map(|(i, n)| Ok((n.clone(), r.get::<_, Value>(i)?))).collect()
                })
                .optional()
                .map_err(err)?
            }
        };
        changes.push(match row {
            Some(values) => Change::Row { table, values },
            None => Change::Gone { table, key }, // deleted since it was logged
        });
    }
    Ok((changes, last, more))
}

/// How far a sale has got; a copy from another till never moves it back.
fn status_rank(status: &str) -> i32 {
    match status {
        "open" => 0,
        "cancelled" => 1,
        "done" => 2,
        "void" => 3,
        _ => 0,
    }
}

/// Where changes being applied came from.
#[derive(Clone, Copy)]
pub enum Source {
    /// Pushed to the main computer by this till: logged for the others.
    Till(i64),
    /// Pulled from the main computer; `cursor` is saved with them.
    Main { cursor: i64 },
}

/// Applies changes from another computer in one transaction, without
/// logging them as this computer's own (except fixes, which go back out).
pub fn apply(conn: &Connection, changes: &[Change], from: Source) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(err)?;
    tx.execute("UPDATE sync_state SET v = '1' WHERE k = 'mute'", []).map_err(err)?;
    let mut columns: HashMap<String, HashSet<String>> = HashMap::new();
    for change in changes {
        let (table, key, fixed) = apply_one(&tx, change, &mut columns)?;
        let log = |origin: Option<i64>| -> rusqlite::Result<()> {
            tx.execute("DELETE FROM sync_log WHERE tbl = ?1 AND key = ?2", [&table, &key])?;
            tx.execute(
                "INSERT INTO sync_log (tbl, key, gone, origin) VALUES (?1, ?2, ?3, ?4)",
                params![table, key, matches!(change, Change::Gone { .. }), origin],
            )
            .map(|_| ())
        };
        match (from, fixed) {
            (_, true) => log(None).map_err(err)?, // the fix goes to every computer
            (Source::Till(till), false) => log(Some(till)).map_err(err)?,
            (Source::Main { .. }, false) => {}
        }
    }
    tx.execute("UPDATE sync_state SET v = '0' WHERE k = 'mute'", []).map_err(err)?;
    if let Source::Main { cursor } = from {
        set_state(&tx, "cursor", &cursor.to_string()).map_err(err)?;
    }
    tx.commit().map_err(err)
}

/// Applies one change. Returns its table, key, and whether it had to be
/// changed to fit (a medicine code already taken here).
fn apply_one(
    conn: &Connection,
    change: &Change,
    columns: &mut HashMap<String, HashSet<String>>,
) -> Result<(String, String, bool), String> {
    let (table, values) = match change {
        Change::Gone { table, key } => {
            let sql = format!("DELETE FROM {table} WHERE {} = ?1", key_expr("", shared(table)?));
            conn.execute(&sql, [key]).map_err(err)?;
            return Ok((table.clone(), key.clone(), false));
        }
        Change::Row { table, values } => (table, values),
    };
    let keys = shared(table)?;
    // Only real columns of a shared table get into the SQL.
    if !columns.contains_key(table) {
        let mut stmt = conn.prepare(&format!("SELECT name FROM pragma_table_info('{table}')")).map_err(err)?;
        let names = stmt.query_map([], |r| r.get(0)).and_then(Iterator::collect).map_err(err)?;
        columns.insert(table.clone(), names);
    }
    if let Some((bad, _)) = values.iter().find(|(c, _)| !columns[table].contains(c)) {
        return Err(format!("Sync: {table} has no column {bad}. Update MedTill on every computer."));
    }
    let get = |col: &str| values.iter().find(|(c, _)| c == col).map(|(_, v)| v);
    let text = |v: Option<&Value>| match v {
        Some(Value::Text(t)) => t.clone(),
        Some(Value::Integer(n)) => n.to_string(),
        _ => String::new(),
    };
    let key = keys.iter().map(|k| text(get(k))).collect::<Vec<_>>().join("\u{1f}");
    if table == "settings" && LOCAL_SETTINGS.contains(&key.as_str()) {
        return Ok((table.clone(), key, false));
    }
    if table == "sales" {
        let here: Option<String> =
            conn.query_row("SELECT status FROM sales WHERE id = ?1", [&key], |r| r.get(0)).optional().map_err(err)?;
        if here.is_some_and(|h| status_rank(&h) > status_rank(&text(get("status")))) {
            return Ok((table.clone(), key, false));
        }
    }
    let names: Vec<&str> = values.iter().map(|(c, _)| c.as_str()).collect();
    let others: Vec<String> =
        names.iter().filter(|c| !keys.contains(c)).map(|c| format!("\"{c}\" = excluded.\"{c}\"")).collect();
    let on_conflict = match others.is_empty() {
        true => "NOTHING".to_string(),
        false => format!("UPDATE SET {}", others.join(", ")),
    };
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({}) ON CONFLICT ({}) DO {on_conflict}",
        names.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", "),
        (1..=names.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", "),
        keys.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", "),
    );
    let mut row: Vec<Value> = values.iter().map(|(_, v)| v.clone()).collect();
    match conn.execute(&sql, rusqlite::params_from_iter(&row)) {
        Ok(_) => Ok((table.clone(), key, false)),
        // Two tills gave different new medicines the same code (or barcode):
        // this one gets the next free code, and the fix is sent back out.
        Err(rusqlite::Error::SqliteFailure(e, _))
            if table == "products" && e.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            let next: String = conn
                .query_row(
                    "SELECT CAST(COALESCE(MAX(CAST(code AS INTEGER)), 1000) + 1 AS TEXT) FROM products
                     WHERE code GLOB '[0-9]*'",
                    [],
                    |r| r.get(0),
                )
                .map_err(err)?;
            for (i, name) in names.iter().enumerate() {
                match *name {
                    "code" => row[i] = Value::Text(next.clone()),
                    "barcode" => row[i] = Value::Null,
                    _ => {}
                }
            }
            conn.execute(&sql, rusqlite::params_from_iter(&row)).map_err(err)?;
            Ok((table.clone(), key, true))
        }
        Err(e) => Err(err(e)),
    }
}

// ---------- On the wire: one change per line ----------

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r")
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, true) => match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some(other) => out.push(other),
                None => {}
            },
            (c, false) => out.push(c),
        }
    }
    out
}

fn encode_value(v: &Value) -> String {
    match v {
        Value::Null => "~".into(),
        Value::Integer(n) => format!("i{n}"),
        Value::Real(x) => format!("r{x}"),
        Value::Text(t) => format!("t{}", escape(t)),
        Value::Blob(b) => format!("b{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>()),
    }
}

fn decode_value(text: &str) -> Result<Value, String> {
    let bad = || format!("Sync: unreadable value {text}");
    let (kind, rest) = text.split_at(text.len().min(1));
    Ok(match kind {
        "~" => Value::Null,
        "i" => Value::Integer(rest.parse().map_err(|_| bad())?),
        "r" => Value::Real(rest.parse().map_err(|_| bad())?),
        "t" => Value::Text(unescape(rest)),
        "b" => Value::Blob(
            (0..rest.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(rest.get(i..i + 2).unwrap_or(""), 16))
                .collect::<Result<_, _>>()
                .map_err(|_| bad())?,
        ),
        _ => return Err(bad()),
    })
}

/// `R table col value col value ...` or `D table key`, tab-separated.
pub fn encode(change: &Change) -> String {
    match change {
        Change::Row { table, values } => {
            let cells: Vec<String> = values.iter().map(|(c, v)| format!("{c}\t{}", encode_value(v))).collect();
            format!("R\t{table}\t{}", cells.join("\t"))
        }
        Change::Gone { table, key } => format!("D\t{table}\t{}", escape(key)),
    }
}

pub fn decode(line: &str) -> Result<Change, String> {
    let mut parts = line.split('\t');
    let (kind, table) = (parts.next().unwrap_or(""), parts.next().unwrap_or("").to_string());
    shared(&table)?;
    match kind {
        "D" => Ok(Change::Gone { table, key: unescape(parts.next().unwrap_or("")) }),
        "R" => {
            let rest: Vec<&str> = parts.collect();
            let values = rest
                .chunks(2)
                .map(|pair| match pair {
                    [col, value] => Ok((col.to_string(), decode_value(value)?)),
                    _ => Err("Sync: a row was cut short".to_string()),
                })
                .collect::<Result<_, String>>()?;
            Ok(Change::Row { table, values })
        }
        _ => Err(format!("Sync: unreadable line {kind}")),
    }
}

// ---------- Talking to the main computer ----------
//
// A request is `MEDTILL <version> <link code> <op> <till> <arg>`, then one
// change per line, then `END`. The answer is `OK <arg>`, changes and `END`,
// or `ERR <message>`.

fn send(stream: &mut TcpStream, head: &str, changes: &[Change]) -> std::io::Result<()> {
    let mut out = String::from(head);
    out.push('\n');
    for change in changes {
        out += &encode(change);
        out.push('\n');
    }
    out += "END\n";
    stream.write_all(out.as_bytes())?;
    stream.flush()
}

/// Reads a head line and changes up to `END`.
fn receive(reader: &mut impl BufRead) -> Result<(String, Vec<Change>), String> {
    let mut lines = reader.lines();
    let mut next = || lines.next().unwrap_or(Ok(String::new())).map_err(|e| format!("The connection broke: {e}"));
    let head = next()?;
    if head.is_empty() {
        return Err("The other computer closed the connection.".into());
    }
    let mut changes = Vec::new();
    if head.starts_with("ERR") {
        return Ok((head, changes));
    }
    loop {
        let line = next()?;
        match line.as_str() {
            "END" => return Ok((head, changes)),
            "" => return Err("The connection broke before the end.".into()),
            line => changes.push(decode(line)?),
        }
    }
}

/// Sends one request to the main computer and returns its answer.
fn request(
    main: &str,
    link: &str,
    op: &str,
    till: i64,
    arg: &str,
    changes: &[Change],
) -> Result<(String, Vec<Change>), String> {
    let address = if main.contains(':') { main.to_string() } else { format!("{main}:{PORT}") };
    let unreachable = |e: std::io::Error| {
        format!(
            "Can't reach the main computer ({main}): {e}. Check it is on, on the same network, and MedTill is open."
        )
    };
    let target =
        address.to_socket_addrs().map_err(unreachable)?.next().ok_or_else(|| format!("Unknown computer {main}"))?;
    let mut stream = TcpStream::connect_timeout(&target, Duration::from_secs(3)).map_err(unreachable)?;
    stream.set_read_timeout(Some(Duration::from_secs(30))).map_err(unreachable)?;
    stream.set_write_timeout(Some(Duration::from_secs(30))).map_err(unreachable)?;
    send(&mut stream, &format!("MEDTILL {} {link} {op} {till} {arg}", db::VERSION), changes).map_err(unreachable)?;
    let (head, changes) = receive(&mut BufReader::new(stream))?;
    match head.strip_prefix("OK") {
        Some(arg) => Ok((arg.trim().to_string(), changes)),
        None => Err(head.strip_prefix("ERR").unwrap_or(&head).trim().to_string()),
    }
}

/// Joins the main computer from a fresh install: becomes the next till and
/// copies everything (staff, medicines, stock, sales). Returns the till number.
pub fn join(conn: &Connection, main: &str, link: &str) -> Result<i64, String> {
    let (main, link) = (main.trim(), link.trim().to_uppercase());
    if !db::needs_setup(conn)? {
        return Err("This computer already has its own data, so it can't join another one.".into());
    }
    let (answer, _) = request(main, &link, "JOIN", 0, "", &[])?;
    let (till, branch) = answer.split_once(' ').ok_or("The main computer gave an unreadable answer.")?;
    let till: i64 = till.parse().map_err(|_| "The main computer gave an unreadable till number.")?;
    let tx = conn.unchecked_transaction().map_err(err)?;
    tx.execute("DELETE FROM sync_log", []).map_err(err)?; // this fresh install's defaults stay here
    for (k, v) in
        [("role", "till"), ("main", main), ("link", &link), ("till", &till.to_string()), ("sent", "0"), ("cursor", "0")]
    {
        set_state(&tx, k, v).map_err(err)?;
    }
    tx.execute("INSERT OR REPLACE INTO settings (key, value) VALUES ('branch_id', ?1)", [branch]).map_err(err)?;
    tx.commit().map_err(err)?;
    sync_once(conn)?;
    Ok(till)
}

/// One round on a till: send this till's changes, then fetch everyone else's.
pub fn sync_once(conn: &Connection) -> Result<(), String> {
    let (till, main, link) = details(conn);
    loop {
        let sent = number(conn, "sent").map_err(err)?;
        let (changes, last, more) = collect(conn, sent, None)?;
        if !changes.is_empty() {
            request(&main, &link, "PUSH", till, "", &changes)?;
        }
        set_state(conn, "sent", &last.to_string()).map_err(err)?;
        conn.execute("DELETE FROM sync_log WHERE seq <= ?1", [last]).map_err(err)?; // delivered
        if !more {
            break;
        }
    }
    loop {
        let cursor = number(conn, "cursor").map_err(err)?;
        let (answer, changes) = request(&main, &link, "PULL", till, &cursor.to_string(), &[])?;
        let mut parts = answer.split(' ');
        let cursor: i64 =
            parts.next().and_then(|c| c.parse().ok()).ok_or("The main computer gave an unreadable answer.")?;
        apply(conn, &changes, Source::Main { cursor })?;
        if parts.next() != Some("more") {
            return Ok(());
        }
    }
}

// ---------- Running in the background ----------

/// What the sync is doing, shown in the menu bar and in Staff & settings.
#[derive(Default)]
pub struct Status {
    running: bool,
    pub last_ok: Option<Instant>,
    pub problem: Option<String>,
    /// Main computer: when each till was last heard from.
    pub tills: HashMap<i64, Instant>,
    /// Set by "Sync now".
    pub wake: bool,
}

pub type Shared = Arc<Mutex<Status>>;

fn update(status: &Shared, f: impl FnOnce(&mut Status)) {
    if let Ok(mut s) = status.lock() {
        f(&mut s);
    }
}

/// Starts this computer's side of the sync (once): the server on the main
/// computer, the 10-second round on a till. Each runs on its own thread with
/// its own database connection, so the till never waits for the network.
pub fn start(conn: &Connection, path: PathBuf, status: &Shared) {
    let role = role(conn);
    let Ok(mut s) = status.lock() else { return };
    if s.running || role == Role::Alone {
        return;
    }
    s.running = true;
    drop(s);
    let status = status.clone();
    std::thread::spawn(move || {
        let conn = match db::open(&path.to_string_lossy()) {
            Ok(conn) => conn,
            Err(e) => return update(&status, |s| s.problem = Some(e.0)),
        };
        match role {
            Role::Main => serve(&conn, &status),
            _ => run_till(&conn, &status),
        }
    });
}

fn run_till(conn: &Connection, status: &Shared) {
    loop {
        let result = sync_once(conn);
        update(status, |s| match result {
            Ok(()) => (s.last_ok, s.problem) = (Some(Instant::now()), None),
            Err(e) => s.problem = Some(e),
        });
        let started = Instant::now();
        while started.elapsed() < EVERY {
            std::thread::sleep(Duration::from_millis(250));
            if status.lock().map(|mut s| std::mem::take(&mut s.wake)).unwrap_or(false) {
                break;
            }
        }
    }
}

fn serve(conn: &Connection, status: &Shared) {
    let listener = match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(listener) => listener,
        Err(e) => {
            let problem = format!("Other computers can't connect: port {PORT} is busy ({e}). Is MedTill open twice?");
            return update(status, |s| s.problem = Some(problem));
        }
    };
    update(status, |s| s.problem = None);
    for stream in listener.incoming().flatten() {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
        let result = answer(conn, stream);
        update(status, |s| match result {
            Ok(till) => {
                s.tills.insert(till, Instant::now());
                s.last_ok = Some(Instant::now());
            }
            Err(e) => s.problem = Some(e),
        });
    }
}

/// Handles one request from a till; returns its till number.
fn answer(conn: &Connection, stream: TcpStream) -> Result<i64, String> {
    let mut out = stream.try_clone().map_err(err)?;
    let (head, changes) = receive(&mut BufReader::new(stream))?;
    let reply = handle(conn, &head, &changes);
    let till = head.split(' ').nth(4).and_then(|t| t.parse().ok()).unwrap_or(0);
    let result = match &reply {
        Ok((arg, changes)) => send(&mut out, &format!("OK {arg}"), changes),
        Err(e) => send(&mut out, &format!("ERR {e}"), &[]),
    };
    result.map_err(err)?;
    reply.map(|(arg, _)| {
        arg.split(' ').next().and_then(|t| t.parse().ok()).filter(|_| head.contains(" JOIN ")).unwrap_or(till)
    })
}

fn handle(conn: &Connection, head: &str, changes: &[Change]) -> Result<(String, Vec<Change>), String> {
    let parts: Vec<&str> = head.split(' ').collect();
    let &[magic, version, link, op, till, arg] = parts.as_slice() else { return Err("Unreadable request.".into()) };
    if magic != "MEDTILL" {
        return Err("Not a MedTill request.".into());
    }
    if version != db::VERSION.to_string() {
        return Err(format!(
            "This computer runs MedTill data version {}; update MedTill on both computers.",
            db::VERSION
        ));
    }
    if role(conn) != Role::Main || link != state(conn, "link").map_err(err)? {
        return Err("Wrong link code. Copy it from Staff & settings on the main computer.".into());
    }
    let till: i64 = till.parse().map_err(|_| "Unreadable till number.")?;
    match op {
        "JOIN" => {
            let next = number(conn, "next_till").map_err(err)?.max(2);
            set_state(conn, "next_till", &(next + 1).to_string()).map_err(err)?;
            Ok((format!("{next} {}", db::branch_id(conn)?), Vec::new()))
        }
        "PUSH" => {
            apply(conn, changes, Source::Till(till))?;
            Ok((changes.len().to_string(), Vec::new()))
        }
        "PULL" => {
            let cursor: i64 = arg.parse().map_err(|_| "Unreadable cursor.")?;
            let (changes, last, more) = collect(conn, cursor, Some(till))?;
            Ok((format!("{last} {}", if more { "more" } else { "done" }), changes))
        }
        _ => Err(format!("Unknown request {op}.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn temp_db(name: &str) -> (Connection, PathBuf) {
        let path = std::env::temp_dir().join(format!("medtill-sync-{name}-{}.sqlite", std::process::id()));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
        }
        (db::open(&path.to_string_lossy()).unwrap(), path)
    }

    #[test]
    fn values_survive_the_wire() {
        let row = Change::Row {
            table: "audit".into(),
            values: vec![
                ("id".into(), Value::Text("a\tb\\c\nd".into())),
                ("n".into(), Value::Integer(-5)),
                ("x".into(), Value::Real(1.5)),
                ("z".into(), Value::Null),
                ("b".into(), Value::Blob(vec![0, 255])),
            ],
        };
        assert_eq!(decode(&encode(&row)).unwrap(), row);
        let gone = Change::Gone { table: "box_words".into(), key: "p1\u{1f}PANADOL".into() };
        assert_eq!(decode(&encode(&gone)).unwrap(), gone);
        assert!(decode("R\tsqlite_master\tx\ti1").is_err(), "only shared tables");
    }

    #[test]
    fn two_tills_sell_offline_and_meet() {
        // The main computer has the shop; a fresh computer joins it over the network.
        let (main, main_path) = temp_db("main");
        let owner = db::setup(&main, "Town", "Kelvin", "1234").unwrap();
        let pan = db::save_product(
            &main,
            &owner,
            &Product { name: "Panadol".into(), price: Cents(500), ..Default::default() },
        )
        .unwrap();
        let line = DeliveryLine {
            product_id: pan.clone(),
            batch_no: "B1".into(),
            expiry: "2099-01-01".parse().unwrap(),
            qty: 10,
            cost: Cents(300),
            ..Default::default()
        };
        db::receive(&main, &owner, &Delivery { lines: vec![line], ..Default::default() }).unwrap();
        let link = make_main(&main).unwrap();
        let status = Shared::default();
        start(&main, main_path.clone(), &status);
        std::thread::sleep(Duration::from_millis(300));

        let (till, _) = temp_db("till");
        assert!(join(&till, "127.0.0.1", "WRONG").unwrap_err().contains("link code"));
        assert_eq!(join(&till, "127.0.0.1", &link.to_lowercase()).unwrap(), 2);
        assert!(!db::needs_setup(&till).unwrap(), "the staff came across");
        let ann = db::login(&till, &owner.user.id, "1234").unwrap().expect("same PIN works on the till");
        assert_eq!(db::list_products(&till, &ann.branch_id).unwrap()[0].stock, 10);

        // Each till opens its own shift and sells 3, without talking.
        db::open_shift(&main, &owner).unwrap();
        db::open_shift(&till, &ann).unwrap();
        let three = [CartLine { product_id: pan.clone(), qty: 3 }];
        db::record_sale(&main, &owner, None, &three, Payment::Cash, None, None).unwrap();
        db::record_sale(&till, &ann, None, &three, Payment::Mpesa, None, Some("QJH7XK2L9P")).unwrap();
        // An order sent from the till gets number 1001 and is paid on the main computer.
        assert_eq!(db::place_order(&till, &ann, &[CartLine { product_id: pan.clone(), qty: 1 }]).unwrap(), 1001);
        // Both add a new medicine: both get code 1002.
        let new = |conn: &Connection, s: &Session, name: &str| {
            db::save_product(conn, s, &Product { name: name.into(), price: Cents(100), ..Default::default() }).unwrap()
        };
        new(&main, &owner, "Brufen");
        new(&till, &ann, "Piriton");

        sync_once(&till).unwrap();
        let order = db::open_orders(&main, &owner.branch_id).unwrap().remove(0);
        db::record_sale(&main, &owner, Some(&order.id), &order.lines, Payment::Cash, None, None).unwrap();
        sync_once(&till).unwrap();
        sync_once(&till).unwrap(); // the code fix made on the till goes back out

        for conn in [&main, &till] {
            let products = db::list_products(conn, &owner.branch_id).unwrap();
            let stock: Vec<(String, String, i64)> =
                products.iter().map(|p| (p.name.clone(), p.code.clone(), p.stock)).collect();
            let panadol = stock.iter().find(|p| p.0 == "Panadol").unwrap();
            assert_eq!(panadol, &("Panadol".into(), "1001".into(), 3), "10 − 3 − 3 − 1 on both computers");
            let mut codes: Vec<&str> = products.iter().map(|p| p.code.as_str()).collect();
            codes.sort();
            codes.dedup();
            assert_eq!(codes.len(), 3, "no two medicines share a code: {stock:?}");
            assert!(db::open_orders(conn, &owner.branch_id).unwrap().is_empty(), "the order was paid");
            let sales = db::report(conn, Report::Sales, &owner.branch_id, "2000-01-01", "2100-01-01").unwrap();
            assert_eq!(sales.rows.len(), 3);
            let items: i64 = sales.rows.iter().map(|r| r[7].parse::<i64>().unwrap()).sum();
            assert_eq!(items, 7, "the paid order's draft lines were removed everywhere");
        }
        // Each till's shift only counts its own sales.
        assert_eq!(db::current_shift(&till, &ann.branch_id).unwrap().unwrap().totals().mpesa, Cents(1500));
        assert_eq!(db::current_shift(&main, &owner.branch_id).unwrap().unwrap().totals().cash, Cents(2000));
        // The till's printer setting stays its own.
        db::put_setting(&till, "print_receipts", "0").unwrap();
        sync_once(&till).unwrap();
        assert_eq!(db::setting(&main, "print_receipts").unwrap(), "1");
    }
}
