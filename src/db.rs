use chrono::NaiveDate;
use rusqlite::{params, Connection, Result};

use crate::models::*;

const DATE_FMT: &str = "%Y-%m-%d";

fn date_to_str(d: NaiveDate) -> String {
    d.format(DATE_FMT).to_string()
}

fn str_to_date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, DATE_FMT).expect("stored date should always be valid")
}

fn payment_method_to_str(pm: PaymentMethod) -> &'static str {
    match pm {
        PaymentMethod::Cash => "cash",
        PaymentMethod::Card => "card",
        PaymentMethod::MobileMoney => "mobile_money",
    }
}

pub fn init_db(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS products (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            name              TEXT NOT NULL,
            category          TEXT NOT NULL,
            batch_number      TEXT NOT NULL,
            expiry_date       TEXT NOT NULL,
            price_cents       INTEGER NOT NULL,
            cost_price_cents  INTEGER NOT NULL,
            stock_quantity    INTEGER NOT NULL DEFAULT 0,
            supplier_id       INTEGER,
            reorder_level     INTEGER NOT NULL DEFAULT 0,
            is_active         INTEGER NOT NULL DEFAULT 1,
            barcode           TEXT
        );

        CREATE TABLE IF NOT EXISTS suppliers (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            name            TEXT NOT NULL,
            contact_phone   TEXT NOT NULL DEFAULT '',
            contact_email   TEXT NOT NULL DEFAULT '',
            address         TEXT NOT NULL DEFAULT '',
            notes           TEXT NOT NULL DEFAULT '',
            is_active       INTEGER NOT NULL DEFAULT 1
        );

        CREATE TABLE IF NOT EXISTS deliveries (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            supplier_id     INTEGER NOT NULL,
            delivery_date   TEXT NOT NULL,
            invoice_number  TEXT NOT NULL DEFAULT '',
            notes           TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS delivery_items (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            delivery_id       INTEGER NOT NULL,
            product_id        INTEGER NOT NULL,
            quantity          INTEGER NOT NULL,
            unit_cost_cents   INTEGER NOT NULL,
            batch_number      TEXT NOT NULL,
            expiry_date       TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS sales (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            sale_date       TEXT NOT NULL,
            payment_method  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS sale_items (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            sale_id           INTEGER NOT NULL,
            product_id        INTEGER NOT NULL,
            product_name      TEXT NOT NULL,
            quantity          INTEGER NOT NULL,
            unit_price_cents  INTEGER NOT NULL
        );
        ",
    )?;

    // A brand-new database already has every column from the CREATE TABLEs
    // above. An existing medtill.sqlite (your current one) was created before
    // `is_active` existed, so this adds it without touching any data.
    migrate(&conn)?;

    Ok(conn)
}

// ---------- Schema upgrades ----------

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    // `table` only ever comes from the fixed names in `migrate` below, never
    // from user input, so building the SQL with format! is safe here.
    let sql = format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let found = stmt.exists(params![column])?;
    Ok(found)
}

/// Brings an existing database up to date. Safe to run on every startup:
/// each step checks first and only adds what's missing.
pub fn migrate(conn: &Connection) -> Result<()> {
    if !has_column(conn, "products", "is_active")? {
        conn.execute(
            "ALTER TABLE products ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }
    if !has_column(conn, "suppliers", "is_active")? {
        conn.execute(
            "ALTER TABLE suppliers ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }
    if !has_column(conn, "products", "barcode")? {
        conn.execute("ALTER TABLE products ADD COLUMN barcode TEXT", [])?;
    }
    // No two active products may share a barcode, or one scan could match
    // two medicines. Deleted (inactive) products are left out, so a deleted
    // product's barcode can be reused for its replacement.
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_products_active_barcode
         ON products(barcode)
         WHERE barcode IS NOT NULL AND is_active = 1",
        [],
    )?;
    Ok(())
}

// ---------- Products ----------

/// Active (not deleted) products only, sorted A-Z ignoring upper/lower case.
pub fn list_products(conn: &Connection) -> Result<Vec<Product>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, category, batch_number, expiry_date, price_cents,
                cost_price_cents, stock_quantity, supplier_id, reorder_level, barcode
         FROM products
         WHERE is_active = 1
         ORDER BY name COLLATE NOCASE",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(Product {
            id: row.get(0)?,
            name: row.get(1)?,
            category: row.get(2)?,
            batch_number: row.get(3)?,
            expiry_date: str_to_date(&row.get::<_, String>(4)?),
            price: Cents(row.get(5)?),
            cost_price: Cents(row.get(6)?),
            stock_quantity: row.get(7)?,
            supplier_id: row.get(8)?,
            reorder_level: row.get(9)?,
            barcode: row.get(10)?,
        })
    })?;

    rows.collect()
}

pub fn add_product(conn: &Connection, product: &Product) -> Result<i64> {
    conn.execute(
        "INSERT INTO products
            (name, category, batch_number, expiry_date, price_cents,
             cost_price_cents, stock_quantity, supplier_id, reorder_level, barcode)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            product.name,
            product.category,
            product.batch_number,
            date_to_str(product.expiry_date),
            product.price.0,
            product.cost_price.0,
            product.stock_quantity,
            product.supplier_id,
            product.reorder_level,
            product.barcode,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_product(conn: &Connection, product: &Product) -> Result<()> {
    conn.execute(
        "UPDATE products SET name=?1, category=?2, batch_number=?3, expiry_date=?4,
            price_cents=?5, cost_price_cents=?6, stock_quantity=?7, supplier_id=?8,
            reorder_level=?9, barcode=?10
         WHERE id=?11",
        params![
            product.name,
            product.category,
            product.batch_number,
            date_to_str(product.expiry_date),
            product.price.0,
            product.cost_price.0,
            product.stock_quantity,
            product.supplier_id,
            product.reorder_level,
            product.barcode,
            product.id,
        ],
    )?;
    Ok(())
}

pub fn adjust_stock(conn: &Connection, product_id: i64, delta: i64) -> Result<()> {
    conn.execute(
        "UPDATE products SET stock_quantity = stock_quantity + ?1 WHERE id = ?2",
        params![delta, product_id],
    )?;
    Ok(())
}

/// "Deletes" a product by hiding it. The row stays in the database because
/// past sale and delivery lines still point at it, so erasing it for real
/// would break history.
pub fn deactivate_product(conn: &Connection, product_id: i64) -> Result<()> {
    conn.execute("UPDATE products SET is_active = 0 WHERE id = ?1", params![product_id])?;
    Ok(())
}

// ---------- Suppliers ----------

/// Active (not deleted) suppliers only, sorted A-Z ignoring upper/lower case.
pub fn list_suppliers(conn: &Connection) -> Result<Vec<Supplier>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, contact_phone, contact_email, address, notes
         FROM suppliers
         WHERE is_active = 1
         ORDER BY name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Supplier {
            id: row.get(0)?,
            name: row.get(1)?,
            contact_phone: row.get(2)?,
            contact_email: row.get(3)?,
            address: row.get(4)?,
            notes: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn add_supplier(conn: &Connection, supplier: &Supplier) -> Result<i64> {
    conn.execute(
        "INSERT INTO suppliers (name, contact_phone, contact_email, address, notes)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            supplier.name,
            supplier.contact_phone,
            supplier.contact_email,
            supplier.address,
            supplier.notes,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_supplier(conn: &Connection, supplier: &Supplier) -> Result<()> {
    conn.execute(
        "UPDATE suppliers SET name=?1, contact_phone=?2, contact_email=?3, address=?4, notes=?5
         WHERE id=?6",
        params![
            supplier.name,
            supplier.contact_phone,
            supplier.contact_email,
            supplier.address,
            supplier.notes,
            supplier.id,
        ],
    )?;
    Ok(())
}

/// "Deletes" a supplier by hiding it. Their past deliveries still show in
/// History with the supplier's name, because those rows stay in place.
pub fn deactivate_supplier(conn: &Connection, supplier_id: i64) -> Result<()> {
    conn.execute("UPDATE suppliers SET is_active = 0 WHERE id = ?1", params![supplier_id])?;
    Ok(())
}

// ---------- Deliveries (restocking) ----------

pub fn record_delivery(conn: &mut Connection, delivery: &Delivery) -> Result<i64> {
    let tx = conn.transaction()?;

    tx.execute(
        "INSERT INTO deliveries (supplier_id, delivery_date, invoice_number, notes)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            delivery.supplier_id,
            date_to_str(delivery.delivery_date),
            delivery.invoice_number,
            delivery.notes,
        ],
    )?;
    let delivery_id = tx.last_insert_rowid();

    for item in &delivery.items {
        tx.execute(
            "INSERT INTO delivery_items
                (delivery_id, product_id, quantity, unit_cost_cents, batch_number, expiry_date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                delivery_id,
                item.product_id,
                item.quantity,
                item.unit_cost.0,
                item.batch_number,
                date_to_str(item.expiry_date),
            ],
        )?;

        tx.execute(
            "UPDATE products SET stock_quantity = stock_quantity + ?1 WHERE id = ?2",
            params![item.quantity, item.product_id],
        )?;
    }

    tx.commit()?;
    Ok(delivery_id)
}

// ---------- Sales (checkout) ----------

pub fn record_sale(conn: &mut Connection, sale: &Sale) -> Result<i64> {
    let tx = conn.transaction()?;

    tx.execute(
        "INSERT INTO sales (sale_date, payment_method) VALUES (?1, ?2)",
        params![date_to_str(sale.sale_date), payment_method_to_str(sale.payment_method)],
    )?;
    let sale_id = tx.last_insert_rowid();

    for item in &sale.items {
        tx.execute(
            "INSERT INTO sale_items (sale_id, product_id, product_name, quantity, unit_price_cents)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![sale_id, item.product_id, item.product_name, item.quantity, item.unit_price.0],
        )?;

        tx.execute(
            "UPDATE products SET stock_quantity = stock_quantity - ?1 WHERE id = ?2",
            params![item.quantity, item.product_id],
        )?;
    }

    tx.commit()?;
    Ok(sale_id)
}

// ---------- Sales & delivery history ----------

pub fn list_sales(conn: &Connection) -> Result<Vec<SaleRecord>> {
    let mut sale_stmt = conn.prepare(
        "SELECT id, sale_date, payment_method FROM sales ORDER BY sale_date DESC, id DESC",
    )?;
    let mut sales: Vec<SaleRecord> = sale_stmt
        .query_map([], |row| {
            let date_str: String = row.get(1)?;
            Ok(SaleRecord {
                id: row.get(0)?,
                sale_date: str_to_date(&date_str),
                payment_method: row.get(2)?,
                items: Vec::new(),
            })
        })?
        .collect::<Result<Vec<_>>>()?;

    let mut item_stmt = conn.prepare(
        "SELECT product_name, quantity, unit_price_cents FROM sale_items WHERE sale_id = ?1",
    )?;

    for sale in &mut sales {
        sale.items = item_stmt
            .query_map(params![sale.id], |row| {
                Ok(SaleItemRecord {
                    product_name: row.get(0)?,
                    quantity: row.get::<_, i64>(1)? as u32,
                    unit_price: Cents(row.get(2)?),
                })
            })?
            .collect::<Result<Vec<_>>>()?;
    }

    Ok(sales)
}

pub fn list_deliveries(conn: &Connection) -> Result<Vec<DeliveryRecord>> {
    // No `is_active` filter here on purpose: History should still show
    // deliveries from suppliers and products that were later deleted.
    let mut delivery_stmt = conn.prepare(
        "SELECT d.id, d.supplier_id, s.name, d.delivery_date, d.invoice_number, d.notes
         FROM deliveries d
         JOIN suppliers s ON s.id = d.supplier_id
         ORDER BY d.delivery_date DESC, d.id DESC",
    )?;
    let mut deliveries: Vec<DeliveryRecord> = delivery_stmt
        .query_map([], |row| {
            let date_str: String = row.get(3)?;
            Ok(DeliveryRecord {
                id: row.get(0)?,
                supplier_id: row.get(1)?,
                supplier_name: row.get(2)?,
                delivery_date: str_to_date(&date_str),
                invoice_number: row.get(4)?,
                notes: row.get(5)?,
                items: Vec::new(),
            })
        })?
        .collect::<Result<Vec<_>>>()?;

    let mut item_stmt = conn.prepare(
        "SELECT p.name, di.quantity, di.unit_cost_cents, di.batch_number, di.expiry_date
         FROM delivery_items di
         JOIN products p ON p.id = di.product_id
         WHERE di.delivery_id = ?1",
    )?;

    for delivery in &mut deliveries {
        delivery.items = item_stmt
            .query_map(params![delivery.id], |row| {
                let expiry_str: String = row.get(4)?;
                Ok(DeliveryItemRecord {
                    product_id: 0,
                    product_name: row.get(0)?,
                    quantity: row.get::<_, i64>(1)? as u32,
                    unit_cost: Cents(row.get(2)?),
                    batch_number: row.get(3)?,
                    expiry_date: str_to_date(&expiry_str),
                })
            })?
            .collect::<Result<Vec<_>>>()?;
    }

    Ok(deliveries)
}

// ---------- Dashboard aggregates ----------

pub fn today_sales_total(conn: &Connection) -> Result<Cents> {
    let today_str = date_to_str(chrono::Local::now().date_naive());
    let total: i64 = conn.query_row(
        "SELECT COALESCE(SUM(si.unit_price_cents * si.quantity), 0)
         FROM sale_items si
         JOIN sales s ON s.id = si.sale_id
         WHERE s.sale_date = ?1",
        params![today_str],
        |row| row.get(0),
    )?;
    Ok(Cents(total))
}

pub fn count_low_stock(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM products WHERE is_active = 1 AND stock_quantity <= reorder_level",
        [],
        |row| row.get(0),
    )
}

pub fn count_expiring_soon(conn: &Connection, within_days: i64) -> Result<i64> {
    let today = chrono::Local::now().date_naive();
    let cutoff = today + chrono::Duration::days(within_days);
    conn.query_row(
        "SELECT COUNT(*) FROM products
         WHERE is_active = 1 AND expiry_date >= ?1 AND expiry_date <= ?2",
        params![date_to_str(today), date_to_str(cutoff)],
        |row| row.get(0),
    )
}

pub fn count_products(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM products WHERE is_active = 1", [], |row| row.get(0))
}