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
            reorder_level     INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS suppliers (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            name            TEXT NOT NULL,
            contact_phone   TEXT NOT NULL DEFAULT '',
            contact_email   TEXT NOT NULL DEFAULT '',
            address         TEXT NOT NULL DEFAULT '',
            notes           TEXT NOT NULL DEFAULT ''
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

    Ok(conn)
}

// ---------- Products ----------

pub fn list_products(conn: &Connection) -> Result<Vec<Product>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, category, batch_number, expiry_date, price_cents,
                cost_price_cents, stock_quantity, supplier_id, reorder_level
         FROM products ORDER BY name",
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
        })
    })?;

    rows.collect()
}

pub fn add_product(conn: &Connection, product: &Product) -> Result<i64> {
    conn.execute(
        "INSERT INTO products
            (name, category, batch_number, expiry_date, price_cents,
             cost_price_cents, stock_quantity, supplier_id, reorder_level)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_product(conn: &Connection, product: &Product) -> Result<()> {
    conn.execute(
        "UPDATE products SET name=?1, category=?2, batch_number=?3, expiry_date=?4,
            price_cents=?5, cost_price_cents=?6, stock_quantity=?7, supplier_id=?8,
            reorder_level=?9
         WHERE id=?10",
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

// ---------- Suppliers ----------

pub fn list_suppliers(conn: &Connection) -> Result<Vec<Supplier>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, contact_phone, contact_email, address, notes FROM suppliers ORDER BY name",
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

/// New: updates an existing supplier's details in place.
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
// --- Sales & delivery history ---

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
// --- Dashboard aggregates ---

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
        "SELECT COUNT(*) FROM products WHERE stock_quantity <= reorder_level",
        [],
        |row| row.get(0),
    )
}

pub fn count_expiring_soon(conn: &Connection, within_days: i64) -> Result<i64> {
    let today = chrono::Local::now().date_naive();
    let cutoff = today + chrono::Duration::days(within_days);
    conn.query_row(
        "SELECT COUNT(*) FROM products WHERE expiry_date >= ?1 AND expiry_date <= ?2",
        params![date_to_str(today), date_to_str(cutoff)],
        |row| row.get(0),
    )
}

pub fn count_products(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM products", [], |row| row.get(0))
}

/// Daily sales totals for the last `days` days, including days with zero
/// sales (so the chart doesn't silently skip a quiet day). SQLite only
/// returns rows for dates that actually had a sale, so the gaps are filled
/// in here rather than in SQL.
pub fn sales_totals_last_n_days(conn: &Connection, days: i64) -> Result<Vec<(NaiveDate, Cents)>> {
    let today = chrono::Local::now().date_naive();
    let start = today - chrono::Duration::days(days - 1);

    let mut stmt = conn.prepare(
        "SELECT sales.sale_date, SUM(sale_items.unit_price_cents * sale_items.quantity)
         FROM sale_items
         JOIN sales ON sales.id = sale_items.sale_id
         WHERE sales.sale_date >= ?1
         GROUP BY sales.sale_date",
    )?;
    let rows: Vec<(String, i64)> = stmt
        .query_map(params![date_to_str(start)], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>>>()?;

    let totals: std::collections::HashMap<String, i64> = rows.into_iter().collect();

    let mut result = Vec::with_capacity(days as usize);
    for i in 0..days {
        let day = start + chrono::Duration::days(i);
        let cents = *totals.get(&date_to_str(day)).unwrap_or(&0);
        result.push((day, Cents(cents)));
    }
    Ok(result)
}

pub fn top_products_by_quantity(conn: &Connection, limit: i64) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT product_name, SUM(quantity) AS total_qty
         FROM sale_items
         GROUP BY product_name
         ORDER BY total_qty DESC
         LIMIT ?1",
    )?;
    stmt.query_map(params![limit], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>>>()
}