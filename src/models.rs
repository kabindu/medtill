use chrono::NaiveDate;

/// Money as integer cents, never as a float. Floats can't represent
/// amounts like 0.10 exactly in binary, so repeated addition/multiplication
/// (adding up a cart, computing a delivery's total cost) silently drifts —
/// unacceptable when real currency is on the line. `Cents` wraps a plain
/// `i64`, so it's `Copy`: passing it around, returning it from a function,
/// summing a list of them — none of that touches the heap or clones anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cents(pub i64);

impl Cents {
    pub fn from_major(units: f64) -> Self {
        Cents((units * 100.0).round() as i64)
    }

    pub fn as_major(&self) -> f64 {
        self.0 as f64 / 100.0
    }
}

impl std::ops::Add for Cents {
    type Output = Cents;
    fn add(self, other: Cents) -> Cents {
        Cents(self.0 + other.0)
    }
}

impl std::ops::Mul<u32> for Cents {
    type Output = Cents;
    fn mul(self, qty: u32) -> Cents {
        Cents(self.0 * qty as i64)
    }
}

/// A drug/product on the shelf. This is the single owner of everything
/// about that product — name, price, stock level. Nothing else in the app
/// keeps its own copy of this data; other types refer to it by `id` instead
/// (see `CartItem` below), so there's exactly one place price or stock can
/// ever be wrong.
#[derive(Debug, Clone)]
pub struct Product {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub batch_number: String,
    pub expiry_date: NaiveDate,
    pub price: Cents,
    pub cost_price: Cents,
    pub stock_quantity: u32,
    pub supplier_id: Option<i64>,
    pub reorder_level: u32,
    /// The code printed under the barcode on the box (e.g. "6162004002795"),
    /// or `None` for products entered without one. An owned `String` rather
    /// than a number: barcodes can start with 0 and some label formats mix
    /// letters and digits, so they're identifiers, not quantities.
    pub barcode: Option<String>,
}

impl Product {
    // These take `&self` — a borrow — because answering "is this low on
    // stock?" doesn't need to consume the Product. The caller (an inventory
    // list still rendering 200 other rows) keeps ownership the whole time.
    pub fn is_low_stock(&self) -> bool {
        self.stock_quantity <= self.reorder_level
    }

    pub fn is_expired(&self, today: NaiveDate) -> bool {
        self.expiry_date < today
    }

    pub fn is_expiring_soon(&self, today: NaiveDate, within_days: i64) -> bool {
        let days_left = (self.expiry_date - today).num_days();
        (0..=within_days).contains(&days_left)
    }
}

/// One line in the active cart. Deliberately holds only `product_id` +
/// `quantity` — NOT a cloned copy of the whole Product. If a cart item
/// stored its own `name: String` and `price: Cents` snapshot, every add-
/// to-cart click would heap-allocate a copy of the product's name, and a
/// price change mid-shift wouldn't be reflected in items already in the
/// cart. Because it's just two integers, `CartItem` is `Copy` — cheap to
/// store in a `Vec` and cheap to duplicate if the user clicks "+1".
#[derive(Debug, Clone, Copy)]
pub struct CartItem {
    pub product_id: i64,
    pub quantity: u32,
}

impl CartItem {
    /// Takes `product: &Product` — a borrow, not ownership. This function
    /// only needs to read the price for one instant to compute a number;
    /// there's no reason to move or clone the Product just to ask it a
    /// question, and the borrow checker enforces that the Product this
    /// points at is still valid (still in the inventory list) when called.
    pub fn subtotal(&self, product: &Product) -> Cents {
        product.price * self.quantity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaymentMethod {
    #[default]
    Cash,
    Card,
    MobileMoney,
}

/// A completed sale — a historical record. Unlike CartItem, SaleItem DOES
/// snapshot the product's name and price at sale time (see below), because
/// a receipt from last month must stay correct even if that product is
/// later renamed, repriced, or deleted. That's an intentional ownership
/// choice, not an oversight: once a sale is final, it needs to own its own
/// facts rather than borrow ones that can drift.
#[derive(Debug, Clone)]
pub struct Sale {
    pub id: i64,
    pub sale_date: NaiveDate,
    pub items: Vec<SaleItem>,
    pub payment_method: PaymentMethod,
}

impl Sale {
    /// `.iter()` borrows each `SaleItem` in turn rather than draining or
    /// cloning the Vec — the caller still owns `self.items` afterward.
    pub fn total(&self) -> Cents {
        self.items.iter().fold(Cents(0), |acc, item| acc + item.subtotal())
    }
}

#[derive(Debug, Clone)]
pub struct SaleItem {
    pub product_id: i64,
    pub product_name: String, // snapshot, deliberately owned — see Sale's doc comment
    pub quantity: u32,
    pub unit_price: Cents,
}

impl SaleItem {
    pub fn subtotal(&self) -> Cents {
        self.unit_price * self.quantity
    }
}

#[derive(Debug, Clone)]
pub struct Supplier {
    pub id: i64,
    pub name: String,
    pub contact_phone: String,
    pub contact_email: String,
    pub address: String,
    pub notes: String,
}

/// A delivery received from a supplier — restocking inventory. Like Sale,
/// this snapshots batch/expiry/cost per item rather than borrowing from
/// Product, because a delivery record documents what physically arrived on
/// that date and must stay accurate as a paper trail.
#[derive(Debug, Clone)]
pub struct Delivery {
    pub id: i64,
    pub supplier_id: i64,
    pub delivery_date: NaiveDate,
    pub invoice_number: String,
    pub notes: String,
    pub items: Vec<DeliveryItem>,
}

impl Delivery {
    pub fn total_cost(&self) -> Cents {
        self.items.iter().fold(Cents(0), |acc, item| acc + item.line_cost())
    }
}

#[derive(Debug, Clone)]
pub struct DeliveryItem {
    pub product_id: i64,
    pub quantity: u32,
    pub unit_cost: Cents,
    pub batch_number: String,
    pub expiry_date: NaiveDate,
}

impl DeliveryItem {
    pub fn line_cost(&self) -> Cents {
        self.unit_cost * self.quantity
    }
}

// --- Sales & delivery history (read models) ---
//
// `SaleRecord`/`DeliveryRecord` exist only for the history screen. Unlike
// `Sale`/`Delivery` (built once from live user input right before a write),
// a `*Record` is built fresh from a database query every time the history
// screen renders — the row data lived in a `rusqlite::Row` that's dropped as
// soon as the query finishes, so owning every field isn't a style choice,
// it's required for the data to outlive the query.

/// One historical sale with its line items, for the history screen.
pub struct SaleRecord {
    pub id: i64,
    pub sale_date: NaiveDate,
    pub payment_method: String,
    pub items: Vec<SaleItemRecord>,
}

impl SaleRecord {
    /// Recomputed from line items rather than stored redundantly — the
    /// line items stay the single source of truth for the total.
    pub fn total(&self) -> Cents {
        self.items
            .iter()
            .fold(Cents(0), |acc, item| acc + item.subtotal())
    }
}

/// One line of a historical sale. `product_name` is the snapshot taken at
/// sale time, not a live lookup into `products` — history stays accurate
/// even if the product is later renamed, repriced, or removed.
pub struct SaleItemRecord {
    pub product_name: String,
    pub quantity: u32,
    pub unit_price: Cents,
}

impl SaleItemRecord {
    pub fn subtotal(&self) -> Cents {
        self.unit_price * self.quantity
    }
}

/// One historical delivery with its line items, for the history screen.
pub struct DeliveryRecord {
    pub id: i64,
    pub supplier_id: i64,
    pub supplier_name: String,
    pub delivery_date: NaiveDate,
    pub invoice_number: String,
    pub notes: String,
    pub items: Vec<DeliveryItemRecord>,
}

impl DeliveryRecord {
    pub fn total_cost(&self) -> Cents {
        self.items
            .iter()
            .fold(Cents(0), |acc, item| acc + item.line_cost())
    }
}

/// One line of a historical delivery.
pub struct DeliveryItemRecord {
    pub product_id: i64,
    pub product_name: String,
    pub quantity: u32,
    pub unit_cost: Cents,
    pub batch_number: String,
    pub expiry_date: NaiveDate,
}

impl DeliveryItemRecord {
    pub fn line_cost(&self) -> Cents {
        self.unit_cost * self.quantity
    }
}