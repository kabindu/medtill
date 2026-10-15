//! All database access.
//!
//! Three rules keep this file small and the data trustworthy:
//! - **Stock is never stored as a number.** Every change is a row in
//!   `stock_moves` (+ received, − sold, − written off, ± counted, + voided),
//!   and a batch's stock is the sum of its moves (the `batch_stock` view).
//! - **Every id is random** (128 bits, made by SQLite), so records created
//!   offline at different branches never clash when they are synced.
//! - **Every change is made by a `Session`** and written to `audit` in the
//!   same transaction, so the log always says who did what.

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OptionalExtension, ToSql, params};

use crate::models::*;

/// A database or business-rule problem, worded for the person at the till.
#[derive(Debug)]
pub struct Error(pub String);

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error(format!("Database error: {e}"))
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lets screens use `?` on database calls inside functions that return
/// `Result<_, String>` (the message to show).
impl From<Error> for String {
    fn from(e: Error) -> String {
        e.0
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error(message.into()))
}

// ---------- Schema ----------

const VERSION: i32 = 4;

/// `{id}` and `{now}` are expanded before running (see `migrate`).
const SCHEMA: &str = "
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE branches (id {id}, name TEXT NOT NULL);
CREATE TABLE users (
    id {id}, name TEXT NOT NULL, role TEXT NOT NULL, pin_hash TEXT NOT NULL,
    branch_id TEXT,                              -- NULL: every branch (owner)
    is_active INTEGER NOT NULL DEFAULT 1);
CREATE TABLE suppliers (
    id {id}, name TEXT NOT NULL, phone TEXT NOT NULL DEFAULT '',
    email TEXT NOT NULL DEFAULT '', address TEXT NOT NULL DEFAULT '',
    notes TEXT NOT NULL DEFAULT '', is_active INTEGER NOT NULL DEFAULT 1);
CREATE TABLE products (
    id {id}, name TEXT NOT NULL, category TEXT NOT NULL DEFAULT '', barcode TEXT,
    price_cents INTEGER NOT NULL, reorder_level INTEGER NOT NULL DEFAULT 0,
    rx_only INTEGER NOT NULL DEFAULT 0, is_active INTEGER NOT NULL DEFAULT 1);
CREATE UNIQUE INDEX products_barcode ON products(barcode)
    WHERE barcode IS NOT NULL AND is_active = 1;
CREATE TABLE deliveries (
    id {id}, branch_id TEXT NOT NULL, supplier_id TEXT, invoice_no TEXT NOT NULL DEFAULT '',
    notes TEXT NOT NULL DEFAULT '', user_id TEXT, at {now});
CREATE TABLE batches (
    id {id}, product_id TEXT NOT NULL, branch_id TEXT NOT NULL, delivery_id TEXT,
    batch_no TEXT NOT NULL, expiry TEXT NOT NULL, cost_cents INTEGER NOT NULL,
    qty_received INTEGER NOT NULL, received_at {now});
CREATE INDEX batches_product ON batches(product_id, branch_id);
CREATE TABLE stock_moves (
    id {id}, batch_id TEXT NOT NULL, qty INTEGER NOT NULL,
    kind TEXT NOT NULL,                          -- receive, restock, sale, void, writeoff, count, opening
    ref_id TEXT, note TEXT NOT NULL DEFAULT '', user_id TEXT, at {now});
CREATE INDEX stock_moves_batch ON stock_moves(batch_id);
CREATE TABLE shifts (
    id {id}, branch_id TEXT NOT NULL, user_id TEXT NOT NULL, float_cents INTEGER NOT NULL,
    opened_at {now}, closed_at TEXT, counted_cents INTEGER);
CREATE TABLE sales (
    id {id}, branch_id TEXT NOT NULL, shift_id TEXT, user_id TEXT, payment TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'done',         -- done or void
    at {now});
CREATE INDEX sales_at ON sales(branch_id, at);
CREATE TABLE sale_items (
    id {id}, sale_id TEXT NOT NULL, product_id TEXT NOT NULL, batch_id TEXT,
    product_name TEXT NOT NULL, qty INTEGER NOT NULL,
    price_cents INTEGER NOT NULL, cost_cents INTEGER NOT NULL);
CREATE INDEX sale_items_sale ON sale_items(sale_id);
CREATE TABLE audit (
    id {id}, branch_id TEXT NOT NULL, user_id TEXT,
    action TEXT NOT NULL, detail TEXT NOT NULL, at {now});
CREATE INDEX audit_at ON audit(at);
CREATE VIEW batch_stock AS
    SELECT b.*, (SELECT COALESCE(SUM(m.qty), 0) FROM stock_moves m WHERE m.batch_id = b.id) AS qty
    FROM batches b;
";

/// The tables of MedTill before logins and batches, renamed out of the way.
const RENAME_OLD: &str = "
DROP INDEX IF EXISTS idx_products_active_barcode;
ALTER TABLE products RENAME TO old_products;
ALTER TABLE suppliers RENAME TO old_suppliers;
ALTER TABLE deliveries RENAME TO old_deliveries;
ALTER TABLE delivery_items RENAME TO old_delivery_items;
ALTER TABLE sales RENAME TO old_sales;
ALTER TABLE sale_items RENAME TO old_sale_items;
";

/// Copies the old data in. Each product's stock becomes one opening batch
/// (its old batch number and expiry); old delivery lines are kept as history
/// only, since their stock is already counted in the opening batches.
const COPY_OLD: &str = "
CREATE TEMP TABLE id_map AS
    SELECT 'p' AS t, id AS old, lower(hex(randomblob(16))) AS new FROM old_products
    UNION ALL SELECT 's', id, lower(hex(randomblob(16))) FROM old_suppliers
    UNION ALL SELECT 'd', id, lower(hex(randomblob(16))) FROM old_deliveries
    UNION ALL SELECT 'x', id, lower(hex(randomblob(16))) FROM old_sales;
INSERT INTO suppliers (id, name, phone, email, address, notes, is_active)
    SELECT m.new, s.name, s.contact_phone, s.contact_email, s.address, s.notes, s.is_active
    FROM old_suppliers s JOIN id_map m ON m.t = 's' AND m.old = s.id;
INSERT INTO products (id, name, category, barcode, price_cents, reorder_level, is_active)
    SELECT m.new, p.name, p.category, p.barcode, p.price_cents, p.reorder_level, p.is_active
    FROM old_products p JOIN id_map m ON m.t = 'p' AND m.old = p.id;
INSERT INTO batches (product_id, branch_id, batch_no, expiry, cost_cents, qty_received)
    SELECT m.new, {branch}, p.batch_number, p.expiry_date, p.cost_price_cents, p.stock_quantity
    FROM old_products p JOIN id_map m ON m.t = 'p' AND m.old = p.id
    WHERE p.stock_quantity > 0;
INSERT INTO stock_moves (batch_id, qty, kind, note)
    SELECT id, qty_received, 'opening', 'Stock carried over from the old MedTill' FROM batches;
INSERT INTO deliveries (id, branch_id, supplier_id, invoice_no, notes, at)
    SELECT m.new, {branch}, (SELECT new FROM id_map WHERE t = 's' AND old = d.supplier_id),
           d.invoice_number, d.notes, d.delivery_date || ' 00:00:00'
    FROM old_deliveries d JOIN id_map m ON m.t = 'd' AND m.old = d.id;
INSERT INTO batches (product_id, branch_id, delivery_id, batch_no, expiry, cost_cents,
                     qty_received, received_at)
    SELECT (SELECT new FROM id_map WHERE t = 'p' AND old = i.product_id), {branch},
           (SELECT new FROM id_map WHERE t = 'd' AND old = i.delivery_id),
           i.batch_number, i.expiry_date, i.unit_cost_cents, i.quantity, d.delivery_date || ' 00:00:00'
    FROM old_delivery_items i JOIN old_deliveries d ON d.id = i.delivery_id;
INSERT INTO sales (id, branch_id, payment, at)
    SELECT m.new, {branch},
           CASE s.payment_method WHEN 'mobile_money' THEN 'mpesa' ELSE s.payment_method END,
           s.sale_date || ' 00:00:00'
    FROM old_sales s JOIN id_map m ON m.t = 'x' AND m.old = s.id;
INSERT INTO sale_items (sale_id, product_id, product_name, qty, price_cents, cost_cents)
    SELECT (SELECT new FROM id_map WHERE t = 'x' AND old = i.sale_id),
           (SELECT new FROM id_map WHERE t = 'p' AND old = i.product_id),
           i.product_name, i.quantity, i.unit_price_cents, 0
    FROM old_sale_items i;
DROP TABLE id_map;
DROP TABLE old_sale_items; DROP TABLE old_sales; DROP TABLE old_delivery_items;
DROP TABLE old_deliveries; DROP TABLE old_suppliers; DROP TABLE old_products;
";

/// Version 3: medicine codes (existing medicines get 1001, 1002, ...), the
/// cash handed over on each sale, one shared shift per till (any extra open
/// shifts are closed) and receipt settings.
const V3: &str = "
ALTER TABLE products ADD COLUMN code TEXT;
UPDATE products SET code = CAST(1000 + (SELECT COUNT(*) FROM products p WHERE p.rowid <= products.rowid) AS TEXT);
CREATE UNIQUE INDEX products_code ON products(code) WHERE is_active = 1;
ALTER TABLE sales ADD COLUMN paid_cents INTEGER;
UPDATE shifts SET closed_at = datetime('now', 'localtime') WHERE closed_at IS NULL
    AND id <> (SELECT id FROM shifts WHERE closed_at IS NULL ORDER BY opened_at DESC LIMIT 1);
INSERT OR IGNORE INTO settings (key, value) VALUES
    ('print_receipts', '1'), ('receipt_header', ''), ('receipt_footer', 'Thank you. Get well soon!');
";

/// Version 4: orders sent to the cashier. A sale is now `open` (an order
/// waiting for payment), `done`, `void` or `cancelled`; it remembers its order
/// number, who took the money (`cashier_id`), the M-Pesa code and when it was paid.
const V4: &str = "
ALTER TABLE sales ADD COLUMN order_no INTEGER;
ALTER TABLE sales ADD COLUMN cashier_id TEXT;
ALTER TABLE sales ADD COLUMN mpesa_ref TEXT;
ALTER TABLE sales ADD COLUMN paid_at TEXT;
UPDATE sales SET cashier_id = user_id, paid_at = at;
CREATE INDEX sales_status ON sales(branch_id, status);
";

/// Opens (or creates) the database and brings it up to date.
pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version >= VERSION {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    if version < 2 {
        let has_old_tables =
            tx.prepare("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'products'")?.exists([])?;
        if has_old_tables {
            tx.execute_batch(RENAME_OLD)?;
        }
        tx.execute_batch(
            &SCHEMA
                .replace("{id}", "TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16))))")
                .replace("{now}", "TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))"),
        )?;
        let branch: String =
            tx.query_row("INSERT INTO branches (name) VALUES ('Main branch') RETURNING id", [], |r| r.get(0))?;
        tx.execute("INSERT INTO settings (key, value) VALUES ('branch_id', ?1)", [&branch])?;
        if has_old_tables {
            tx.execute_batch(&COPY_OLD.replace("{branch}", &format!("'{branch}'")))?;
        }
    }
    if version < 3 {
        tx.execute_batch(V3)?;
    }
    tx.execute_batch(V4)?;
    tx.pragma_update(None, "user_version", VERSION)?;
    tx.commit()?;
    Ok(())
}

fn audit(conn: &Connection, s: &Session, action: &str, detail: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO audit (branch_id, user_id, action, detail) VALUES (?1, ?2, ?3, ?4)",
        params![s.branch_id, s.user.id, action, detail],
    )?;
    Ok(())
}

fn add_move(
    conn: &Connection,
    s: &Session,
    batch: &str,
    qty: i64,
    kind: &str,
    ref_id: Option<&str>,
    note: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO stock_moves (batch_id, qty, kind, ref_id, note, user_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![batch, qty, kind, ref_id, note, s.user.id],
    )?;
    Ok(())
}

// ---------- Branch, users, login ----------

pub fn branch_id(conn: &Connection) -> Result<String> {
    Ok(conn.query_row("SELECT value FROM settings WHERE key = 'branch_id'", [], |r| r.get(0))?)
}

pub fn branch_name(conn: &Connection) -> Result<String> {
    Ok(conn.query_row(
        "SELECT name FROM branches WHERE id = (SELECT value FROM settings WHERE key = 'branch_id')",
        [],
        |r| r.get(0),
    )?)
}

pub fn rename_branch(conn: &Connection, s: &Session, name: &str) -> Result<()> {
    conn.execute("UPDATE branches SET name = ?1 WHERE id = ?2", params![name, s.branch_id])?;
    audit(conn, s, "branch", &format!("Branch renamed to {name}"))
}

/// A setting such as "receipt_footer" (empty if never set).
pub fn setting(conn: &Connection, key: &str) -> Result<String> {
    Ok(conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()?.unwrap_or_default())
}

pub fn save_settings(conn: &Connection, s: &Session, pairs: &[(&str, &str)]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for (key, value) in pairs {
        tx.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)", [key, value])?;
    }
    audit(&tx, s, "settings", "Receipt settings saved")?;
    tx.commit()?;
    Ok(())
}

/// True on the very first start: nobody can log in yet.
pub fn needs_setup(conn: &Connection) -> Result<bool> {
    Ok(!conn.prepare("SELECT 1 FROM users")?.exists([])?)
}

fn hash_pin(pin: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(pin.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| Error(format!("Could not secure the PIN: {e}")))
}

fn pin_matches(pin: &str, stored: &str) -> bool {
    PasswordHash::new(stored).is_ok_and(|hash| Argon2::default().verify_password(pin.as_bytes(), &hash).is_ok())
}

/// First start: names the branch and creates the owner, who is logged in.
pub fn setup(conn: &Connection, branch: &str, owner: &str, pin: &str) -> Result<Session> {
    let tx = conn.unchecked_transaction()?;
    let branch_id = branch_id(&tx)?;
    tx.execute("UPDATE branches SET name = ?1 WHERE id = ?2", params![branch, branch_id])?;
    let id: String = tx.query_row(
        "INSERT INTO users (name, role, pin_hash) VALUES (?1, 'owner', ?2) RETURNING id",
        params![owner, hash_pin(pin)?],
        |r| r.get(0),
    )?;
    let user = User { id, name: owner.to_string(), role: Role::Owner, active: true };
    let session = Session { user, branch_id };
    audit(&tx, &session, "setup", &format!("MedTill set up for {branch}"))?;
    tx.commit()?;
    Ok(session)
}

/// Users who may log in at this branch (`all`: include deactivated ones).
pub fn list_users(conn: &Connection, all: bool) -> Result<Vec<User>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, role, is_active FROM users
         WHERE (branch_id IS NULL OR branch_id = (SELECT value FROM settings WHERE key = 'branch_id'))
           AND (?1 OR is_active = 1)
         ORDER BY is_active DESC, name COLLATE NOCASE",
    )?;
    let users = stmt.query_map([all], |r| {
        Ok(User { id: r.get(0)?, name: r.get(1)?, role: Role::from_code(&r.get::<_, String>(2)?), active: r.get(3)? })
    })?;
    Ok(users.collect::<rusqlite::Result<_>>()?)
}

/// `Ok(None)` when the PIN is wrong.
pub fn login(conn: &Connection, user_id: &str, pin: &str) -> Result<Option<Session>> {
    let found = conn
        .query_row("SELECT name, role, pin_hash FROM users WHERE id = ?1 AND is_active = 1", [user_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })
        .optional()?;
    let Some((name, role, stored)) = found else { return Ok(None) };
    if !pin_matches(pin, &stored) {
        return Ok(None);
    }
    let user = User { id: user_id.to_string(), name, role: Role::from_code(&role), active: true };
    let session = Session { user, branch_id: branch_id(conn)? };
    audit(conn, &session, "login", "Logged in")?;
    Ok(Some(session))
}

/// Adds (empty `user.id`) or updates a user. `pin`: `None` keeps the old one.
pub fn save_user(conn: &Connection, s: &Session, user: &User, pin: Option<&str>) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let pin_hash = pin.map(hash_pin).transpose()?;
    let branch = (user.role != Role::Owner).then_some(&s.branch_id);
    if user.id.is_empty() {
        let Some(pin_hash) = pin_hash else { return fail("A new user needs a PIN.") };
        tx.execute(
            "INSERT INTO users (name, role, pin_hash, branch_id, is_active) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![user.name, user.role.code(), pin_hash, branch, user.active],
        )?;
    } else {
        tx.execute(
            "UPDATE users SET name = ?1, role = ?2, branch_id = ?3, is_active = ?4,
                pin_hash = COALESCE(?5, pin_hash) WHERE id = ?6",
            params![user.name, user.role.code(), branch, user.active, pin_hash, user.id],
        )?;
    }
    let owners: i64 =
        tx.query_row("SELECT COUNT(*) FROM users WHERE role = 'owner' AND is_active = 1", [], |r| r.get(0))?;
    if owners == 0 {
        return fail("There must always be at least one active owner.");
    }
    let state = if user.active { "active" } else { "deactivated" };
    let pin_note = if pin.is_some() { ", PIN set" } else { "" };
    audit(&tx, s, "user", &format!("{} — {} ({state}{pin_note})", user.name, user.role.label()))?;
    tx.commit()?;
    Ok(())
}

// ---------- Medicines ----------

/// Active medicines with their stock at this branch, A–Z.
pub fn list_products(conn: &Connection, branch: &str) -> Result<Vec<Product>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.code, p.name, p.category, p.barcode, p.price_cents, p.reorder_level, p.rx_only,
                COALESCE(SUM(CASE WHEN b.expiry >= date('now', 'localtime') THEN b.qty END), 0),
                COALESCE(SUM(CASE WHEN b.expiry <  date('now', 'localtime') THEN b.qty END), 0),
                MIN(CASE WHEN b.expiry >= date('now', 'localtime') AND b.qty > 0 THEN b.expiry END),
                (SELECT x.cost_cents FROM batches x WHERE x.product_id = p.id ORDER BY x.received_at DESC LIMIT 1),
                (SELECT x.qty_received FROM batches x WHERE x.product_id = p.id ORDER BY x.received_at DESC LIMIT 1),
                (SELECT COALESCE(SUM(si.qty), 0) FROM sale_items si JOIN sales sa ON sa.id = si.sale_id
                  WHERE sa.status = 'open' AND sa.branch_id = ?1 AND si.product_id = p.id)
         FROM products p LEFT JOIN batch_stock b ON b.product_id = p.id AND b.branch_id = ?1
         WHERE p.is_active = 1
         GROUP BY p.id ORDER BY p.name COLLATE NOCASE",
    )?;
    let products = stmt.query_map([branch], |r| {
        Ok(Product {
            id: r.get(0)?,
            code: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            name: r.get(2)?,
            category: r.get(3)?,
            barcode: r.get(4)?,
            price: Cents(r.get(5)?),
            reorder_level: r.get(6)?,
            prescription_only: r.get(7)?,
            stock: r.get::<_, i64>(8)? - r.get::<_, i64>(13)?,
            held: r.get(13)?,
            expired: r.get(9)?,
            next_expiry: r.get::<_, Option<String>>(10)?.and_then(|d| d.parse().ok()),
            last_cost: r.get::<_, Option<i64>>(11)?.map(Cents),
            last_qty: r.get(12)?,
        })
    })?;
    Ok(products.collect::<rusqlite::Result<_>>()?)
}

/// Fails if another active medicine already uses this barcode or code.
fn check_unique(conn: &Connection, column: &str, value: &str, id: &str) -> Result<()> {
    let sql = format!("SELECT name FROM products WHERE {column} = ?1 AND is_active = 1 AND id <> ?2");
    match conn.query_row(&sql, params![value, id], |r| r.get::<_, String>(0)).optional()? {
        Some(name) => fail(format!("The {column} {value} already belongs to {name}.")),
        None => Ok(()),
    }
}

/// Adds (empty `p.id`) or updates a medicine's details and returns its id.
/// An empty code gets the next number (1001, 1002, ...). Stock is not touched.
pub fn save_product(conn: &Connection, s: &Session, p: &Product) -> Result<String> {
    if let Some(barcode) = &p.barcode {
        check_unique(conn, "barcode", barcode, &p.id)?;
    }
    if !p.code.is_empty() {
        check_unique(conn, "code", &p.code, &p.id)?;
    }
    let tx = conn.unchecked_transaction()?;
    let next_code: String = tx.query_row(
        "SELECT CAST(COALESCE(MAX(CAST(code AS INTEGER)), 1000) + 1 AS TEXT) FROM products WHERE code GLOB '[0-9]*'",
        [],
        |r| r.get(0),
    )?;
    let code = if p.code.is_empty() { None } else { Some(&p.code) };
    let (id, detail) = if p.id.is_empty() {
        let id: String = tx.query_row(
            "INSERT INTO products (code, name, category, barcode, price_cents, reorder_level, rx_only)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
            params![
                code.unwrap_or(&next_code),
                p.name,
                p.category,
                p.barcode,
                p.price.0,
                p.reorder_level,
                p.prescription_only
            ],
            |r| r.get(0),
        )?;
        (id, format!("Added {} at {}", p.name, p.price))
    } else {
        let old_price: i64 = tx.query_row("SELECT price_cents FROM products WHERE id = ?1", [&p.id], |r| r.get(0))?;
        tx.execute(
            "UPDATE products SET code = COALESCE(?1, code), name = ?2, category = ?3, barcode = ?4,
                price_cents = ?5, reorder_level = ?6, rx_only = ?7 WHERE id = ?8",
            params![code, p.name, p.category, p.barcode, p.price.0, p.reorder_level, p.prescription_only, p.id],
        )?;
        let detail = match old_price == p.price.0 {
            true => format!("Edited {}", p.name),
            false => format!("{}: price {} → {}", p.name, Cents(old_price), p.price),
        };
        (p.id.clone(), detail)
    };
    audit(&tx, s, "product", &detail)?;
    tx.commit()?;
    Ok(id)
}

/// Ids of the medicines checked in most recently at this branch, newest first.
pub fn recent_check_ins(conn: &Connection, branch: &str, limit: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT product_id FROM (
             SELECT product_id, received_at AS at FROM batches WHERE branch_id = ?1 AND delivery_id IS NOT NULL
             UNION ALL
             SELECT b.product_id, m.at FROM stock_moves m JOIN batches b ON b.id = m.batch_id
             WHERE m.kind = 'restock' AND b.branch_id = ?1)
         GROUP BY product_id ORDER BY MAX(at) DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![branch, limit], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Quick picks for the till: the medicines sold most in the last 30 days,
/// and the ones sold most recently (ids, best first).
pub fn quick_picks(conn: &Connection, branch: &str, limit: i64) -> Result<(Vec<String>, Vec<String>)> {
    let ids = |order: &str| -> Result<Vec<String>> {
        let sql = format!(
            "SELECT si.product_id FROM sale_items si JOIN sales sa ON sa.id = si.sale_id
             WHERE sa.branch_id = ?1 AND sa.status = 'done' AND sa.at >= datetime('now', 'localtime', '-30 days')
             GROUP BY si.product_id ORDER BY {order} DESC LIMIT ?2"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![branch, limit], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    };
    Ok((ids("SUM(si.qty)")?, ids("MAX(sa.at)")?))
}

/// Hides a medicine. Its rows stay, because past sales point at them.
pub fn delete_product(conn: &Connection, s: &Session, p: &Product) -> Result<()> {
    conn.execute("UPDATE products SET is_active = 0 WHERE id = ?1", [&p.id])?;
    audit(conn, s, "product", &format!("Deleted {}", p.name))
}

// ---------- Stock ----------

/// Records a check-in: each line is a new batch (a `receive` move), or more
/// units of a batch already on the shelf (a `restock` move).
pub fn receive(conn: &Connection, s: &Session, d: &Delivery) -> Result<()> {
    if d.lines.is_empty() {
        return fail("Add at least one line first.");
    }
    let tx = conn.unchecked_transaction()?;
    let delivery_id: String = tx.query_row(
        "INSERT INTO deliveries (branch_id, invoice_no, user_id) VALUES (?1, ?2, ?3) RETURNING id",
        params![s.branch_id, d.invoice_no.trim(), s.user.id],
        |r| r.get(0),
    )?;
    for line in &d.lines {
        if let Some(batch) = &line.batch_id {
            add_move(&tx, s, batch, line.qty, "restock", Some(&delivery_id), "")?;
            continue;
        }
        let batch: String = tx.query_row(
            "INSERT INTO batches (product_id, branch_id, delivery_id, batch_no, expiry, cost_cents, qty_received)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
            params![
                line.product_id,
                s.branch_id,
                delivery_id,
                line.batch_no,
                line.expiry.to_string(),
                line.cost.0,
                line.qty
            ],
            |r| r.get(0),
        )?;
        add_move(&tx, s, &batch, line.qty, "receive", Some(&delivery_id), "")?;
    }
    let units: i64 = d.lines.iter().map(|l| l.qty).sum();
    let invoice = if d.invoice_no.is_empty() { String::new() } else { format!(", invoice {}", d.invoice_no) };
    audit(&tx, s, "receive", &format!("{} lines, {units} units{invoice}", d.lines.len()))?;
    tx.commit()?;
    Ok(())
}

/// Batches at this branch that still have stock, earliest expiry first.
pub fn list_batches(conn: &Connection, branch: &str) -> Result<Vec<Batch>> {
    let mut stmt = conn.prepare(
        "SELECT b.id, b.product_id, p.name, b.batch_no, b.expiry, b.cost_cents, b.qty
         FROM batch_stock b JOIN products p ON p.id = b.product_id
         WHERE b.branch_id = ?1 AND b.qty > 0
         ORDER BY b.expiry, p.name COLLATE NOCASE",
    )?;
    let batches = stmt.query_map([branch], |r| {
        Ok(Batch {
            id: r.get(0)?,
            product_id: r.get(1)?,
            product_name: r.get(2)?,
            batch_no: r.get(3)?,
            expiry: r.get::<_, String>(4)?.parse().unwrap_or_default(),
            cost: Cents(r.get(5)?),
            qty: r.get(6)?,
        })
    })?;
    Ok(batches.collect::<rusqlite::Result<_>>()?)
}

fn batch_qty(conn: &Connection, batch: &str) -> Result<i64> {
    Ok(conn.query_row("SELECT qty FROM batch_stock WHERE id = ?1", [batch], |r| r.get(0))?)
}

/// Takes units out of a batch (expired, damaged, returned to supplier).
pub fn write_off(conn: &Connection, s: &Session, b: &Batch, qty: i64, reason: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    if qty > batch_qty(&tx, &b.id)? {
        return fail(format!("Batch {} only has {} left.", b.batch_no, b.qty));
    }
    add_move(&tx, s, &b.id, -qty, "writeoff", None, reason)?;
    audit(&tx, s, "write-off", &format!("{} batch {}: −{qty} ({reason})", b.product_name, b.batch_no))?;
    tx.commit()?;
    Ok(())
}

/// Sets a batch to the quantity physically counted on the shelf.
pub fn count_stock(conn: &Connection, s: &Session, b: &Batch, counted: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let diff = counted - batch_qty(&tx, &b.id)?;
    if diff != 0 {
        add_move(&tx, s, &b.id, diff, "count", None, "Stock count")?;
        audit(
            &tx,
            s,
            "stock count",
            &format!("{} batch {}: counted {counted} ({diff:+})", b.product_name, b.batch_no),
        )?;
    }
    tx.commit()?;
    Ok(())
}

// ---------- Shifts and sales ----------

/// The open shift at this till, with what each person sold and took so far.
pub fn current_shift(conn: &Connection, branch: &str) -> Result<Option<Shift>> {
    let head = conn
        .query_row(
            "SELECT sh.id, sh.opened_at, COALESCE(u.name, ''),
                    (SELECT COUNT(*) FROM sales o WHERE o.shift_id = sh.id AND o.status = 'open')
             FROM shifts sh LEFT JOIN users u ON u.id = sh.user_id
             WHERE sh.branch_id = ?1 AND sh.closed_at IS NULL ORDER BY sh.opened_at DESC LIMIT 1",
            [branch],
            |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((id, opened_at, opened_by, waiting)) = head else { return Ok(None) };
    let by = |person: &str| -> Result<Vec<SellerTotals>> {
        let sql = format!(
            "SELECT COALESCE(u.name, '(unknown)'), COUNT(DISTINCT sa.id),
                    COALESCE(SUM(CASE WHEN sa.payment = 'cash' THEN si.qty * si.price_cents END), 0),
                    COALESCE(SUM(CASE WHEN sa.payment <> 'cash' THEN si.qty * si.price_cents END), 0)
             FROM sales sa JOIN sale_items si ON si.sale_id = sa.id LEFT JOIN users u ON u.id = sa.{person}
             WHERE sa.shift_id = ?1 AND sa.status = 'done' GROUP BY sa.{person} ORDER BY 1"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([&id], |r| {
            Ok(SellerTotals { name: r.get(0)?, sales: r.get(1)?, cash: Cents(r.get(2)?), mpesa: Cents(r.get(3)?) })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    };
    let (sellers, cashiers) = (by("user_id")?, by("cashier_id")?);
    Ok(Some(Shift { id, opened_at, opened_by, sellers, cashiers, waiting }))
}

/// Opens the till's shift. No cash count: the shift adds up what each
/// person sells and takes until it is closed.
pub fn open_shift(conn: &Connection, s: &Session) -> Result<()> {
    if current_shift(conn, &s.branch_id)?.is_some() {
        return fail("A shift is already open at this till.");
    }
    conn.execute(
        "INSERT INTO shifts (branch_id, user_id, float_cents) VALUES (?1, ?2, 0)",
        params![s.branch_id, s.user.id],
    )?;
    audit(conn, s, "shift", "Opened the shift")
}

/// Closes the till's shift and returns its totals (the Z report). Orders
/// still waiting for payment are cancelled, which frees their stock.
pub fn close_shift(conn: &Connection, s: &Session) -> Result<Shift> {
    let Some(shift) = current_shift(conn, &s.branch_id)? else { return fail("No open shift.") };
    let tx = conn.unchecked_transaction()?;
    tx.execute("UPDATE shifts SET closed_at = datetime('now', 'localtime') WHERE id = ?1", [&shift.id])?;
    let cancelled =
        tx.execute("UPDATE sales SET status = 'cancelled' WHERE shift_id = ?1 AND status = 'open'", [&shift.id])?;
    let t = shift.totals();
    let detail =
        format!("Closed: {} sales, cash {}, M-Pesa {}, {cancelled} unpaid orders cancelled", t.sales, t.cash, t.mpesa);
    audit(&tx, s, "shift", &detail)?;
    tx.commit()?;
    Ok(shift)
}

/// A sellable batch: (id, unit cost in cents, units left).
type BatchRow = (String, i64, i64);

/// Checks a cart line: the medicine exists, the person may sell it, and
/// enough non-expired stock is free (not held for other orders).
/// Returns (name, price in cents, sellable batches in FEFO order).
fn check_line(
    conn: &Connection,
    s: &Session,
    line: &CartLine,
    may_sell_rx: bool,
    order: &str,
) -> Result<(String, i64, Vec<BatchRow>)> {
    let product = conn
        .query_row(
            "SELECT name, price_cents, rx_only FROM products WHERE id = ?1 AND is_active = 1",
            [&line.product_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, bool>(2)?)),
        )
        .optional()?;
    let Some((name, price, rx_only)) = product else { return fail("A medicine in the cart was deleted.") };
    if rx_only && !may_sell_rx {
        return fail(format!("{name} is prescription-only: a pharmacist must sell it."));
    }
    let mut stmt = conn.prepare(
        "SELECT id, cost_cents, qty FROM batch_stock
         WHERE product_id = ?1 AND branch_id = ?2 AND qty > 0 AND expiry >= date('now', 'localtime')
         ORDER BY expiry, received_at",
    )?;
    let batches = stmt
        .query_map(params![line.product_id, s.branch_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let held: i64 = conn.query_row(
        "SELECT COALESCE(SUM(si.qty), 0) FROM sale_items si JOIN sales sa ON sa.id = si.sale_id
         WHERE sa.status = 'open' AND sa.branch_id = ?1 AND si.product_id = ?2 AND sa.id <> ?3",
        params![s.branch_id, line.product_id, order],
        |r| r.get(0),
    )?;
    let free = batches.iter().map(|b| b.2).sum::<i64>() - held;
    if line.qty > free {
        let waiting =
            if held > 0 { format!(" ({held} more are held for orders waiting for payment)") } else { String::new() };
        return fail(format!("Only {} of {name} can be sold{waiting}.", free.max(0)));
    }
    Ok((name, price, batches))
}

/// Sends a cart to the cashier as an order: the stock is held (not yet
/// taken off the shelf) until the order is paid or cancelled.
/// Returns today's order number.
pub fn place_order(conn: &Connection, s: &Session, lines: &[CartLine]) -> Result<i64> {
    let Some(shift) = current_shift(conn, &s.branch_id)? else { return fail("Open the shift first.") };
    if lines.is_empty() {
        return fail("The cart is empty.");
    }
    let tx = conn.unchecked_transaction()?;
    let (id, number): (String, i64) = tx.query_row(
        "INSERT INTO sales (branch_id, shift_id, user_id, payment, status, order_no)
         VALUES (?1, ?2, ?3, '', 'open', (SELECT COALESCE(MAX(order_no), 0) + 1 FROM sales
             WHERE branch_id = ?1 AND date(at) = date('now', 'localtime'))) RETURNING id, order_no",
        params![s.branch_id, shift.id, s.user.id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    for line in lines {
        let (name, price, _) = check_line(&tx, s, line, s.can(Action::SellPrescription), &id)?;
        tx.execute(
            "INSERT INTO sale_items (sale_id, product_id, product_name, qty, price_cents, cost_cents)
             VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            params![id, line.product_id, name, line.qty, price],
        )?;
    }
    tx.commit()?;
    Ok(number)
}

/// Orders at this branch waiting for payment, oldest first.
pub fn open_orders(conn: &Connection, branch: &str) -> Result<Vec<Order>> {
    let mut stmt = conn.prepare(
        "SELECT sa.id, sa.order_no, COALESCE(u.name, ''), sa.at FROM sales sa LEFT JOIN users u ON u.id = sa.user_id
         WHERE sa.branch_id = ?1 AND sa.status = 'open' ORDER BY sa.at",
    )?;
    let heads = stmt
        .query_map([branch], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<Vec<(String, i64, String, String)>>>()?;
    let mut items = conn.prepare("SELECT product_id, qty, price_cents FROM sale_items WHERE sale_id = ?1")?;
    let mut orders = Vec::new();
    for (id, number, seller, at) in heads {
        let rows = items
            .query_map([&id], |r| Ok((CartLine { product_id: r.get(0)?, qty: r.get(1)? }, Cents(r.get(2)?))))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let total = rows.iter().map(|(line, price)| *price * line.qty).sum();
        orders.push(Order { id, number, seller, at, lines: rows.into_iter().map(|(line, _)| line).collect(), total });
    }
    Ok(orders)
}

/// Cancels an order that was not paid; its held stock is free again.
pub fn cancel_order(conn: &Connection, s: &Session, order: &Order, reason: &str) -> Result<()> {
    let changed =
        conn.execute("UPDATE sales SET status = 'cancelled' WHERE id = ?1 AND status = 'open'", [&order.id])?;
    if changed == 0 {
        return fail("That order was already paid or cancelled.");
    }
    audit(conn, s, "order", &format!("Order {} from {} cancelled — {reason}", order.number, order.seller))
}

/// Takes payment and records the sale, taking stock from the batches that
/// expire first (FEFO); expired batches are never sold. `order` is the order
/// being paid (its seller keeps the credit), or `None` for a sale made
/// straight at the till. `paid` is the cash handed over, if typed in.
/// Returns the sale id.
pub fn record_sale(
    conn: &Connection,
    s: &Session,
    order: Option<&str>,
    lines: &[CartLine],
    payment: Payment,
    paid: Option<Cents>,
    mpesa_code: Option<&str>,
) -> Result<String> {
    let Some(shift) = current_shift(conn, &s.branch_id)? else { return fail("Open the shift before selling.") };
    if lines.is_empty() {
        return fail("The cart is empty.");
    }
    if let Some(code) = mpesa_code
        && conn.prepare("SELECT 1 FROM sales WHERE mpesa_ref = ?1 AND status = 'done'")?.exists([code])?
    {
        return fail(format!("M-Pesa code {code} was already used for another sale."));
    }
    let tx = conn.unchecked_transaction()?;
    let (sale_id, may_sell_rx) = match order {
        Some(id) => {
            let seller_role: Option<String> = tx
                .query_row(
                    "SELECT COALESCE(u.role, '') FROM sales sa LEFT JOIN users u ON u.id = sa.user_id
                     WHERE sa.id = ?1 AND sa.branch_id = ?2 AND sa.status = 'open'",
                    params![id, s.branch_id],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(role) = seller_role else { return fail("That order was already paid or cancelled.") };
            tx.execute("DELETE FROM sale_items WHERE sale_id = ?1", [id])?; // the draft lines, re-added below
            (id.to_string(), Role::from_code(&role).can(Action::SellPrescription) || s.can(Action::SellPrescription))
        }
        None => {
            let id = tx.query_row(
                "INSERT INTO sales (branch_id, shift_id, user_id, payment, status) VALUES (?1, ?2, ?3, '', 'open') RETURNING id",
                params![s.branch_id, shift.id, s.user.id],
                |r| r.get(0),
            )?;
            (id, s.can(Action::SellPrescription))
        }
    };
    for line in lines {
        let (name, price, batches) = check_line(&tx, s, line, may_sell_rx, &sale_id)?;
        let mut left = line.qty;
        for (batch, cost, available) in batches {
            let take = left.min(available);
            tx.execute(
                "INSERT INTO sale_items (sale_id, product_id, batch_id, product_name, qty, price_cents, cost_cents)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![sale_id, line.product_id, batch, name, take, price, cost],
            )?;
            add_move(&tx, s, &batch, -take, "sale", Some(&sale_id), "")?;
            left -= take;
            if left == 0 {
                break;
            }
        }
    }
    tx.execute(
        "UPDATE sales SET status = 'done', payment = ?2, paid_cents = ?3, mpesa_ref = ?4, cashier_id = ?5,
            shift_id = ?6, paid_at = datetime('now', 'localtime') WHERE id = ?1",
        params![sale_id, payment.code(), paid.map(|p| p.0), mpesa_code, s.user.id, shift.id],
    )?;
    tx.commit()?;
    Ok(sale_id)
}

/// Everything needed to print (or reprint) a sale's receipt.
pub fn receipt(conn: &Connection, sale_id: &str) -> Result<Receipt> {
    let (order, at, seller, cashier, payment, mpesa_code, paid, status) = conn.query_row(
        "SELECT sa.order_no, COALESCE(sa.paid_at, sa.at), COALESCE(u.name, ''), COALESCE(c.name, ''), sa.payment,
                sa.mpesa_ref, sa.paid_cents, sa.status
         FROM sales sa LEFT JOIN users u ON u.id = sa.user_id LEFT JOIN users c ON c.id = sa.cashier_id
         WHERE sa.id = ?1",
        [sale_id],
        |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, String>(7)?,
            ))
        },
    )?;
    let mut stmt = conn.prepare(
        "SELECT product_name, SUM(qty), price_cents FROM sale_items WHERE sale_id = ?1
         GROUP BY product_id, price_cents ORDER BY MIN(rowid)",
    )?;
    let lines: Vec<(String, i64, Cents)> = stmt
        .query_map([sale_id], |r| Ok((r.get(0)?, r.get(1)?, Cents(r.get(2)?))))?
        .collect::<rusqlite::Result<_>>()?;
    let payment = match payment.as_str() {
        "cash" => "Cash",
        "mpesa" => "M-Pesa",
        _ => "Card",
    };
    Ok(Receipt {
        number: receipt_no(sale_id).to_string(),
        order,
        at,
        seller,
        cashier,
        payment: payment.to_string(),
        mpesa_code,
        paid: paid.map(Cents),
        total: lines.iter().map(|(_, qty, price)| *price * *qty).sum(),
        lines,
        void: status == "void",
    })
}

/// Cancels a sale and puts its units back into the same batches.
pub fn void_sale(conn: &Connection, s: &Session, sale_id: &str, reason: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let changed = tx.execute(
        "UPDATE sales SET status = 'void' WHERE id = ?1 AND branch_id = ?2 AND status = 'done'",
        params![sale_id, s.branch_id],
    )?;
    if changed == 0 {
        return fail("That sale was not found or is already void.");
    }
    tx.execute(
        "INSERT INTO stock_moves (batch_id, qty, kind, ref_id, note, user_id)
         SELECT batch_id, qty, 'void', sale_id, ?2, ?3 FROM sale_items WHERE sale_id = ?1 AND batch_id IS NOT NULL",
        params![sale_id, reason, s.user.id],
    )?;
    audit(&tx, s, "void", &format!("Receipt {} — {reason}", receipt_no(sale_id)))?;
    tx.commit()?;
    Ok(())
}

/// The short receipt number shown to people: the first 8 characters of the id.
pub fn receipt_no(sale_id: &str) -> &str {
    &sale_id[..sale_id.len().min(8)]
}

// ---------- Reports ----------

pub fn stats(conn: &Connection, branch: &str) -> Result<Stats> {
    Ok(conn.query_row(
        "SELECT
           (SELECT COALESCE(SUM(si.qty * si.price_cents), 0) FROM sales sa JOIN sale_items si ON si.sale_id = sa.id
             WHERE sa.branch_id = ?1 AND sa.status = 'done' AND date(sa.at) = date('now', 'localtime')),
           (SELECT COALESCE(SUM(si.qty * (si.price_cents - si.cost_cents)), 0) FROM sales sa JOIN sale_items si ON si.sale_id = sa.id
             WHERE sa.branch_id = ?1 AND sa.status = 'done' AND date(sa.at) = date('now', 'localtime')),
           (SELECT COUNT(*) FROM (SELECT p.id FROM products p
             LEFT JOIN batch_stock b ON b.product_id = p.id AND b.branch_id = ?1 AND b.expiry >= date('now', 'localtime')
             WHERE p.is_active = 1 GROUP BY p.id HAVING COALESCE(SUM(b.qty), 0) < 10)),
           (SELECT COUNT(*) FROM batch_stock WHERE branch_id = ?1 AND qty > 0
             AND expiry BETWEEN date('now', 'localtime') AND date('now', 'localtime', '+90 days')),
           (SELECT COALESCE(SUM(qty * cost_cents), 0) FROM batch_stock WHERE branch_id = ?1 AND qty > 0
             AND expiry < date('now', 'localtime'))",
        [branch],
        |r| {
            Ok(Stats {
                today_sales: Cents(r.get(0)?),
                today_profit: Cents(r.get(1)?),
                low_stock: r.get(2)?,
                expiring_soon: r.get(3)?,
                expired_value: Cents(r.get(4)?),
            })
        },
    )?)
}

/// Shared pieces of the report queries. ?1 = branch, ?2/?3 = from/to dates.
const SOLD: &str = "FROM sales sa JOIN sale_items si ON si.sale_id = sa.id
    WHERE sa.branch_id = ?1 AND sa.status = 'done' AND date(sa.at) BETWEEN ?2 AND ?3";
const REVENUE: &str = "SUM(si.qty * si.price_cents) AS \"Revenue (KSh)\"";
const PROFIT: &str = "SUM(si.qty * (si.price_cents - si.cost_cents)) AS \"Profit (KSh)\"";
const PAYMENT: &str = "CASE sa.payment WHEN 'cash' THEN 'Cash' WHEN 'mpesa' THEN 'M-Pesa' ELSE 'Card' END";
const CASH: &str = "COALESCE(SUM(CASE WHEN sa.payment = 'cash' THEN si.qty * si.price_cents END), 0) AS \"Cash (KSh)\"";
const MPESA: &str =
    "COALESCE(SUM(CASE WHEN sa.payment <> 'cash' THEN si.qty * si.price_cents END), 0) AS \"M-Pesa (KSh)\"";
const STOCK: &str = "FROM products p LEFT JOIN batch_stock b ON b.product_id = p.id AND b.branch_id = ?1
    AND b.expiry >= date('now', 'localtime') WHERE p.is_active = 1 GROUP BY p.id";

/// Runs a report. The first column of each query is the row key (not shown);
/// columns named "... (KSh)" hold cents and are shown as money.
pub fn report(conn: &Connection, kind: Report, branch: &str, from: &str, to: &str) -> Result<Table> {
    let sql = match kind {
        Report::SalesByDay => format!(
            "SELECT date(sa.at), date(sa.at) AS \"Date\", COUNT(DISTINCT sa.id) AS \"Sales\", {REVENUE},
                    SUM(si.qty * si.cost_cents) AS \"Cost (KSh)\", {PROFIT}
             {SOLD} GROUP BY date(sa.at) ORDER BY 1 DESC"
        ),
        Report::Sales => format!(
            "SELECT sa.id, COALESCE(sa.paid_at, sa.at) AS \"Time\", substr(sa.id, 1, 8) AS \"Receipt\",
                    COALESCE(sa.order_no, '') AS \"Order\", COALESCE(u.name, '') AS \"Seller\",
                    COALESCE(c.name, '') AS \"Cashier\", {PAYMENT} AS \"Payment\",
                    COALESCE(sa.mpesa_ref, '') AS \"M-Pesa code\", SUM(si.qty) AS \"Items\",
                    SUM(si.qty * si.price_cents) AS \"Total (KSh)\", sa.status AS \"Status\"
             FROM sales sa JOIN sale_items si ON si.sale_id = sa.id LEFT JOIN users u ON u.id = sa.user_id
             LEFT JOIN users c ON c.id = sa.cashier_id
             WHERE sa.branch_id = ?1 AND sa.status IN ('done', 'void') AND date(sa.at) BETWEEN ?2 AND ?3
             GROUP BY sa.id ORDER BY 2 DESC"
        ),
        Report::CollectedByCashier => format!(
            "SELECT sa.cashier_id, COALESCE(c.name, '(before logins)') AS \"Cashier\",
                    COUNT(DISTINCT sa.id) AS \"Sales\", {CASH}, {MPESA}, SUM(si.qty * si.price_cents) AS \"Total (KSh)\"
             FROM sales sa JOIN sale_items si ON si.sale_id = sa.id LEFT JOIN users c ON c.id = sa.cashier_id
             WHERE sa.branch_id = ?1 AND sa.status = 'done' AND date(sa.at) BETWEEN ?2 AND ?3
             GROUP BY sa.cashier_id ORDER BY 6 DESC"
        ),
        Report::UnpaidOrders => "SELECT sa.id, sa.at AS \"Time\", sa.order_no AS \"Order\",
                    COALESCE(u.name, '') AS \"Seller\", SUM(si.qty) AS \"Items\",
                    SUM(si.qty * si.price_cents) AS \"Total (KSh)\",
                    CASE sa.status WHEN 'open' THEN 'waiting' ELSE 'cancelled' END AS \"Status\"
             FROM sales sa JOIN sale_items si ON si.sale_id = sa.id LEFT JOIN users u ON u.id = sa.user_id
             WHERE sa.branch_id = ?1 AND sa.status IN ('open', 'cancelled') AND date(sa.at) BETWEEN ?2 AND ?3
             GROUP BY sa.id ORDER BY sa.at DESC"
            .to_string(),
        Report::SalesByMedicine => format!(
            "SELECT si.product_id, COALESCE(p.code, '') AS \"Code\", si.product_name AS \"Medicine\",
                    SUM(si.qty) AS \"Qty\", {REVENUE}, {PROFIT}
             FROM sales sa JOIN sale_items si ON si.sale_id = sa.id LEFT JOIN products p ON p.id = si.product_id
             WHERE sa.branch_id = ?1 AND sa.status = 'done' AND date(sa.at) BETWEEN ?2 AND ?3
             GROUP BY si.product_id ORDER BY 5 DESC"
        ),
        Report::SalesByCashier => format!(
            "SELECT sa.user_id, COALESCE(u.name, '(before logins)') AS \"Seller\",
                    COUNT(DISTINCT sa.id) AS \"Sales\", {REVENUE}, {PROFIT}
             FROM sales sa JOIN sale_items si ON si.sale_id = sa.id LEFT JOIN users u ON u.id = sa.user_id
             WHERE sa.branch_id = ?1 AND sa.status = 'done' AND date(sa.at) BETWEEN ?2 AND ?3
             GROUP BY sa.user_id ORDER BY 4 DESC"
        ),
        Report::SellersPerShift => format!(
            "SELECT sh.id || COALESCE(sa.user_id, ''), sh.opened_at AS \"Shift opened\",
                    COALESCE(sh.closed_at, 'still open') AS \"Closed\", COALESCE(u.name, '') AS \"Seller\",
                    COUNT(DISTINCT sa.id) AS \"Sales\", {CASH}, {MPESA}, SUM(si.qty * si.price_cents) AS \"Total (KSh)\"
             FROM shifts sh JOIN sales sa ON sa.shift_id = sh.id AND sa.status = 'done'
             JOIN sale_items si ON si.sale_id = sa.id LEFT JOIN users u ON u.id = sa.user_id
             WHERE sh.branch_id = ?1 AND date(sh.opened_at) BETWEEN ?2 AND ?3
             GROUP BY sh.id, sa.user_id ORDER BY sh.opened_at DESC, 4"
        ),
        Report::SalesByPayment => format!(
            "SELECT sa.payment, {PAYMENT} AS \"Payment\", COUNT(DISTINCT sa.id) AS \"Sales\", {REVENUE}
             {SOLD} GROUP BY sa.payment ORDER BY 4 DESC"
        ),
        Report::StockOnHand => format!(
            "SELECT p.id, p.code AS \"Code\", p.name AS \"Medicine\", COALESCE(SUM(b.qty), 0) AS \"In stock\",
                    p.price_cents AS \"Price (KSh)\",
                    COALESCE(SUM(b.qty * b.cost_cents), 0) AS \"Value at cost (KSh)\"
             {STOCK} ORDER BY p.name COLLATE NOCASE"
        ),
        Report::Reorder => format!(
            "SELECT p.id, p.code AS \"Code\", p.name AS \"Medicine\", COALESCE(SUM(b.qty), 0) AS \"In stock\",
                    CASE WHEN COALESCE(SUM(b.qty), 0) < 10 THEN 'Below 10 — reorder now'
                         WHEN COALESCE(SUM(b.qty), 0) < 25 THEN 'Below 25' ELSE 'Below 50' END AS \"Alert\"
             {STOCK} HAVING COALESCE(SUM(b.qty), 0) < 50 ORDER BY 4, p.name COLLATE NOCASE"
        ),
        Report::Expiry => "SELECT b.id, p.name AS \"Medicine\", b.batch_no AS \"Batch\", b.expiry AS \"Expiry\",
                    CAST(julianday(b.expiry) - julianday(date('now', 'localtime')) AS INTEGER) AS \"Days left\",
                    b.qty AS \"Qty\", b.qty * b.cost_cents AS \"Value at cost (KSh)\"
             FROM batch_stock b JOIN products p ON p.id = b.product_id
             WHERE b.branch_id = ?1 AND b.qty > 0 AND b.expiry <= date('now', 'localtime', '+90 days')
             ORDER BY b.expiry"
            .to_string(),
        Report::Deliveries => "SELECT d.id, d.at AS \"Checked in\", COALESCE(u.name, '') AS \"By\",
                    d.invoice_no AS \"Reference\", COUNT(*) AS \"Lines\", SUM(x.qty) AS \"Units\",
                    SUM(x.qty * x.cost) AS \"Cost (KSh)\"
             FROM deliveries d LEFT JOIN users u ON u.id = d.user_id JOIN (
                 SELECT delivery_id AS d, qty_received AS qty, cost_cents AS cost FROM batches
                 UNION ALL
                 SELECT m.ref_id, m.qty, b.cost_cents FROM stock_moves m JOIN batches b ON b.id = m.batch_id
                 WHERE m.kind = 'restock') x ON x.d = d.id
             WHERE d.branch_id = ?1 AND date(d.at) BETWEEN ?2 AND ?3
             GROUP BY d.id ORDER BY d.at DESC"
            .to_string(),
        Report::Shifts => format!(
            "SELECT sh.id, sh.opened_at AS \"Opened\", COALESCE(u.name, '') AS \"Opened by\",
                    COALESCE(sh.closed_at, 'still open') AS \"Closed\", COUNT(DISTINCT sa.id) AS \"Sales\",
                    {CASH}, {MPESA}, COALESCE(SUM(si.qty * si.price_cents), 0) AS \"Total (KSh)\"
             FROM shifts sh LEFT JOIN users u ON u.id = sh.user_id
             LEFT JOIN sales sa ON sa.shift_id = sh.id AND sa.status = 'done'
             LEFT JOIN sale_items si ON si.sale_id = sa.id
             WHERE sh.branch_id = ?1 AND date(sh.opened_at) BETWEEN ?2 AND ?3
             GROUP BY sh.id ORDER BY sh.opened_at DESC"
        ),
        Report::Audit => "SELECT a.id, a.at AS \"Time\", COALESCE(u.name, '') AS \"User\",
                    a.action AS \"Action\", a.detail AS \"Detail\"
             FROM audit a LEFT JOIN users u ON u.id = a.user_id
             WHERE a.branch_id = ?1 AND date(a.at) BETWEEN ?2 AND ?3 ORDER BY a.at DESC"
            .to_string(),
    };
    if kind.uses_dates() { table(conn, &sql, params![branch, from, to]) } else { table(conn, &sql, params![branch]) }
}

fn table(conn: &Connection, sql: &str, args: &[&dyn ToSql]) -> Result<Table> {
    let mut stmt = conn.prepare(sql)?;
    let headers: Vec<String> = stmt.column_names().iter().skip(1).map(|h| h.to_string()).collect();
    let money: Vec<bool> = headers.iter().map(|h| h.ends_with("(KSh)")).collect();
    let mut table = Table { headers, ..Table::default() };
    let mut rows = stmt.query(args)?;
    while let Some(row) = rows.next()? {
        let text = |i: usize, money: bool| -> rusqlite::Result<String> {
            Ok(match row.get_ref(i)? {
                ValueRef::Null | ValueRef::Blob(_) => String::new(),
                ValueRef::Integer(n) if money => Cents(n).to_string(),
                ValueRef::Integer(n) => n.to_string(),
                ValueRef::Real(x) => format!("{x:.2}"),
                ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
            })
        };
        table.keys.push(text(0, false)?);
        table.rows.push((1..=money.len()).map(|i| text(i, money[i - 1])).collect::<rusqlite::Result<_>>()?);
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner() -> (Connection, Session) {
        let conn = open(":memory:").unwrap();
        let s = setup(&conn, "Town branch", "Kelvin", "1234").unwrap();
        (conn, s)
    }

    fn add_medicine(conn: &Connection, s: &Session, name: &str, rx: bool) -> String {
        let p = Product {
            name: name.into(),
            price: Cents(5000),
            reorder_level: 5,
            prescription_only: rx,
            ..Product::default()
        };
        save_product(conn, s, &p).unwrap()
    }

    fn line(product_id: &str, batch_no: &str, expiry: &str, qty: i64) -> DeliveryLine {
        DeliveryLine {
            product_id: product_id.into(),
            product_name: String::new(),
            batch_no: batch_no.into(),
            expiry: expiry.parse().unwrap(),
            qty,
            cost: Cents(3000),
            ..DeliveryLine::default()
        }
    }

    fn stock(conn: &Connection, s: &Session) -> i64 {
        list_products(conn, &s.branch_id).unwrap()[0].stock
    }

    #[test]
    fn restock_tops_up_or_adds_a_batch() {
        let (conn, s) = owner();
        let id = add_medicine(&conn, &s, "Panadol", false);
        receive(&conn, &s, &Delivery { lines: vec![line(&id, "P1", "2099-01-01", 10)], ..Delivery::default() })
            .unwrap();
        let b = list_batches(&conn, &s.branch_id).unwrap().remove(0);
        let top_up = DeliveryLine { batch_id: Some(b.id.clone()), ..line(&id, "P1", "2099-01-01", 5) };
        let lines = vec![top_up, line(&id, "", "2100-06-30", 4)];
        receive(&conn, &s, &Delivery { lines, ..Delivery::default() }).unwrap();
        let shelf = list_batches(&conn, &s.branch_id).unwrap();
        assert_eq!((shelf.len(), shelf[0].qty, shelf[1].qty), (2, 15, 4), "a top-up joins its batch");
        let checked = report(&conn, Report::Deliveries, &s.branch_id, "2000-01-01", "2100-01-01").unwrap();
        let units: i64 = checked.rows.iter().map(|r| r[4].parse::<i64>().unwrap()).sum();
        assert_eq!((checked.rows.len(), units), (2, 19));
        assert_eq!(recent_check_ins(&conn, &s.branch_id, 5).unwrap(), vec![id]);
    }

    #[test]
    fn fefo_sale_void_and_shift() {
        let (conn, s) = owner();
        let id = add_medicine(&conn, &s, "Panadol", false);
        let lines = vec![line(&id, "LATE", "2099-01-01", 10), line(&id, "EARLY", "2098-01-01", 3)];
        receive(&conn, &s, &Delivery { lines, ..Delivery::default() }).unwrap();
        assert_eq!(stock(&conn, &s), 13);

        let cart = [CartLine { product_id: id.clone(), qty: 5 }];
        assert!(record_sale(&conn, &s, None, &cart, Payment::Cash, None, None).is_err(), "needs an open shift");
        open_shift(&conn, &s).unwrap();
        assert!(open_shift(&conn, &s).is_err(), "one shift per till");
        let sale = record_sale(&conn, &s, None, &cart, Payment::Cash, Some(Cents(30000)), None).unwrap();
        let r = receipt(&conn, &sale).unwrap();
        assert_eq!(
            (r.total, r.paid, r.lines.len(), r.seller.as_str()),
            (Cents(25000), Some(Cents(30000)), 1, "Kelvin")
        );
        assert_eq!(r.lines[0], ("Panadol".to_string(), 5, Cents(5000)), "FEFO split lines are merged on the receipt");

        // FEFO: all 3 from EARLY, then 2 from LATE.
        let batches = list_batches(&conn, &s.branch_id).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!((batches[0].batch_no.as_str(), batches[0].qty), ("LATE", 8));

        let too_many = [CartLine { product_id: id.clone(), qty: 9 }];
        assert!(record_sale(&conn, &s, None, &too_many, Payment::Cash, None, None).is_err());
        assert_eq!(stock(&conn, &s), 8, "a failed sale changes nothing");

        let shift = current_shift(&conn, &s.branch_id).unwrap().unwrap();
        assert_eq!((shift.totals().cash, shift.totals().sales), (Cents(25000), 1));
        let (popular, recent) = quick_picks(&conn, &s.branch_id, 5).unwrap();
        assert_eq!((popular, recent), (vec![id.clone()], vec![id.clone()]));

        let sales = report(&conn, Report::Sales, &s.branch_id, "2000-01-01", "2100-01-01").unwrap();
        void_sale(&conn, &s, &sales.keys[0], "test").unwrap();
        assert_eq!(stock(&conn, &s), 13);
        assert!(void_sale(&conn, &s, &sales.keys[0], "again").is_err());
        assert_eq!(close_shift(&conn, &s).unwrap().totals().total(), Cents(0));
        assert!(current_shift(&conn, &s.branch_id).unwrap().is_none());
    }

    #[test]
    fn expired_and_prescription_rules() {
        let (conn, owner) = owner();
        let id = add_medicine(&conn, &owner, "Amoxil", true);
        receive(&conn, &owner, &Delivery { lines: vec![line(&id, "OLD", "2000-01-01", 4)], ..Delivery::default() })
            .unwrap();
        let p = &list_products(&conn, &owner.branch_id).unwrap()[0];
        assert_eq!((p.stock, p.expired), (0, 4));

        let cashier = User { id: String::new(), name: "Ann".into(), role: Role::Cashier, active: true };
        save_user(&conn, &owner, &cashier, Some("5555")).unwrap();
        let ann = list_users(&conn, false).unwrap().into_iter().find(|u| u.name == "Ann").unwrap();
        assert!(login(&conn, &ann.id, "0000").unwrap().is_none());
        let ann = login(&conn, &ann.id, "5555").unwrap().unwrap();
        open_shift(&conn, &ann).unwrap();
        receive(&conn, &owner, &Delivery { lines: vec![line(&id, "NEW", "2099-01-01", 4)], ..Delivery::default() })
            .unwrap();
        let err =
            record_sale(&conn, &ann, None, &[CartLine { product_id: id.clone(), qty: 1 }], Payment::Mpesa, None, None);
        assert!(err.unwrap_err().0.contains("prescription"));
        // Two sellers in the same shift are totalled separately.
        let panadol = add_medicine(&conn, &owner, "Panadol", false);
        receive(&conn, &owner, &Delivery { lines: vec![line(&panadol, "P1", "2099-01-01", 9)], ..Delivery::default() })
            .unwrap();
        let one = [CartLine { product_id: panadol.clone(), qty: 1 }];
        record_sale(&conn, &ann, None, &one, Payment::Mpesa, None, None).unwrap();
        record_sale(&conn, &owner, None, &one, Payment::Cash, None, None).unwrap();
        let sellers = current_shift(&conn, &owner.branch_id).unwrap().unwrap().sellers;
        let names: Vec<_> = sellers.iter().map(|x| (x.name.as_str(), x.cash, x.mpesa)).collect();
        assert_eq!(names, [("Ann", Cents(0), Cents(5000)), ("Kelvin", Cents(5000), Cents(0))]);
        // Codes: given automatically, unique, and typed codes are kept.
        let codes: Vec<_> = list_products(&conn, &owner.branch_id).unwrap().into_iter().map(|p| p.code).collect();
        assert_eq!(codes, ["1001", "1002"]);
        let dup = Product { name: "X".into(), code: "1001".into(), price: Cents(1), ..Product::default() };
        assert!(save_product(&conn, &owner, &dup).unwrap_err().0.contains("already belongs to Amoxil"));

        let mut me = owner.user.clone();
        me.active = false;
        assert!(save_user(&conn, &owner, &me, None).is_err(), "last owner can't be deactivated");
    }

    #[test]
    fn orders_sent_to_the_cashier() {
        let (conn, owner) = owner();
        let rx = add_medicine(&conn, &owner, "Amoxil", true);
        let pan = add_medicine(&conn, &owner, "Panadol", false);
        let lines = vec![line(&rx, "R1", "2099-01-01", 3), line(&pan, "P1", "2099-01-01", 5)];
        receive(&conn, &owner, &Delivery { lines, ..Delivery::default() }).unwrap();
        let staff = |name: &str, role| {
            let u = User { id: String::new(), name: name.into(), role, active: true };
            save_user(&conn, &owner, &u, Some("1111")).unwrap();
            let u = list_users(&conn, false).unwrap().into_iter().find(|u| u.name == name).unwrap();
            login(&conn, &u.id, "1111").unwrap().unwrap()
        };
        let (pharm, cashier) = (staff("Ann", Role::Pharmacist), staff("Ben", Role::Cashier));
        open_shift(&conn, &pharm).unwrap();
        let cart = vec![CartLine { product_id: rx.clone(), qty: 2 }, CartLine { product_id: pan.clone(), qty: 4 }];
        assert_eq!(place_order(&conn, &pharm, &cart).unwrap(), 1);
        assert!(place_order(&conn, &cashier, &cart[1..]).is_err(), "only 1 Panadol is free");
        let stock: Vec<_> = list_products(&conn, &owner.branch_id).unwrap().iter().map(|p| (p.stock, p.held)).collect();
        assert_eq!(stock, [(1, 2), (1, 4)], "held stock is not sellable");
        assert_eq!(
            place_order(
                &conn,
                &pharm,
                &cart[1..2].iter().map(|l| CartLine { qty: 1, ..l.clone() }).collect::<Vec<_>>()
            )
            .unwrap(),
            2
        );

        let orders = open_orders(&conn, &owner.branch_id).unwrap();
        assert_eq!(
            (orders.len(), orders[0].number, orders[0].total, orders[0].seller.as_str()),
            (2, 1, Cents(30000), "Ann")
        );
        // The cashier may take payment for the pharmacist's prescription order.
        let sale = record_sale(
            &conn,
            &cashier,
            Some(&orders[0].id),
            &orders[0].lines,
            Payment::Mpesa,
            None,
            Some("QJH7XK2L9P"),
        )
        .unwrap();
        let r = receipt(&conn, &sale).unwrap();
        assert_eq!(
            (r.order, r.seller.as_str(), r.cashier.as_str(), r.mpesa_code.as_deref()),
            (Some(1), "Ann", "Ben", Some("QJH7XK2L9P"))
        );
        assert!(
            record_sale(&conn, &cashier, Some(&orders[0].id), &orders[0].lines, Payment::Cash, None, None).is_err(),
            "paid once"
        );
        let again = [CartLine { product_id: pan.clone(), qty: 1 }];
        assert!(
            record_sale(&conn, &owner, None, &again, Payment::Mpesa, None, Some("QJH7XK2L9P"))
                .unwrap_err()
                .0
                .contains("already used")
        );
        assert!(
            record_sale(
                &conn,
                &cashier,
                None,
                &[CartLine { product_id: rx.clone(), qty: 1 }],
                Payment::Cash,
                None,
                None
            )
            .is_err()
        );

        let shift = current_shift(&conn, &owner.branch_id).unwrap().unwrap();
        assert_eq!((shift.sellers[0].name.as_str(), shift.cashiers[0].name.as_str(), shift.waiting), ("Ann", "Ben", 1));
        close_shift(&conn, &owner).unwrap();
        assert!(open_orders(&conn, &owner.branch_id).unwrap().is_empty(), "closing cancels unpaid orders");
        assert_eq!(list_products(&conn, &owner.branch_id).unwrap()[1].stock, 1, "their stock is free again");
        let unpaid = report(&conn, Report::UnpaidOrders, &owner.branch_id, "2000-01-01", "2100-01-01").unwrap();
        assert_eq!(unpaid.rows[0].last().unwrap(), "cancelled");
    }

    #[test]
    fn upgrades_old_database() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT, category TEXT, batch_number TEXT,
                expiry_date TEXT, price_cents INTEGER, cost_price_cents INTEGER, stock_quantity INTEGER,
                supplier_id INTEGER, reorder_level INTEGER, is_active INTEGER, barcode TEXT);
             CREATE TABLE suppliers (id INTEGER PRIMARY KEY, name TEXT, contact_phone TEXT, contact_email TEXT,
                address TEXT, notes TEXT, is_active INTEGER);
             CREATE TABLE deliveries (id INTEGER PRIMARY KEY, supplier_id INTEGER, delivery_date TEXT,
                invoice_number TEXT, notes TEXT);
             CREATE TABLE delivery_items (id INTEGER PRIMARY KEY, delivery_id INTEGER, product_id INTEGER,
                quantity INTEGER, unit_cost_cents INTEGER, batch_number TEXT, expiry_date TEXT);
             CREATE TABLE sales (id INTEGER PRIMARY KEY, sale_date TEXT, payment_method TEXT);
             CREATE TABLE sale_items (id INTEGER PRIMARY KEY, sale_id INTEGER, product_id INTEGER,
                product_name TEXT, quantity INTEGER, unit_price_cents INTEGER);
             INSERT INTO products VALUES (1, 'panadol', 'painkiller', '123', '2099-09-20', 45000, 0, 109, NULL, 2, 1, NULL);
             INSERT INTO suppliers VALUES (1, 'Dawa Ltd', '', '', '', '', 1);
             INSERT INTO deliveries VALUES (1, 1, '2026-09-20', 'INV1', '');
             INSERT INTO delivery_items VALUES (1, 1, 1, 5, 67800, '6677', '2099-12-23');
             INSERT INTO sales VALUES (1, '2026-09-20', 'mobile_money');
             INSERT INTO sale_items VALUES (1, 1, 1, 'panadol', 3, 45000);",
        )
        .unwrap();
        migrate(&conn).unwrap();
        assert!(needs_setup(&conn).unwrap());
        let p = &list_products(&conn, &branch_id(&conn).unwrap()).unwrap()[0];
        assert_eq!((p.name.as_str(), p.code.as_str(), p.stock, p.price), ("panadol", "1001", 109, Cents(45000)));
        assert_eq!(setting(&conn, "receipt_footer").unwrap(), "Thank you. Get well soon!");
        let b = branch_id(&conn).unwrap();
        let sales = report(&conn, Report::SalesByPayment, &b, "2026-01-01", "2026-12-31").unwrap();
        assert_eq!(sales.rows, vec![vec!["M-Pesa".to_string(), "1".into(), "1,350.00".into()]]);
        let deliveries = report(&conn, Report::Deliveries, &b, "2026-01-01", "2026-12-31").unwrap();
        assert_eq!(deliveries.rows[0][2..], ["INV1", "1", "5", "3,390.00"]);
        migrate(&conn).unwrap(); // running again changes nothing
    }
}
