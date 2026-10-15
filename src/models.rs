//! The data MedTill works with, and the role/permission table.

use chrono::NaiveDate;
use std::fmt;

/// Money in whole cents. Never a float: 0.10 can't be stored exactly in
/// binary, so float totals drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Cents(pub i64);

impl std::ops::Add for Cents {
    type Output = Cents;
    fn add(self, other: Cents) -> Cents {
        Cents(self.0 + other.0)
    }
}

impl std::ops::Sub for Cents {
    type Output = Cents;
    fn sub(self, other: Cents) -> Cents {
        Cents(self.0 - other.0)
    }
}

impl std::ops::Mul<i64> for Cents {
    type Output = Cents;
    fn mul(self, qty: i64) -> Cents {
        Cents(self.0 * qty)
    }
}

impl std::iter::Sum for Cents {
    fn sum<I: Iterator<Item = Cents>>(iter: I) -> Cents {
        Cents(iter.map(|c| c.0).sum())
    }
}

/// `Cents(123456)` shows as `1,234.56`.
impl fmt::Display for Cents {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        let whole = (abs / 100).to_string();
        let mut grouped = String::new();
        for (i, digit) in whole.chars().enumerate() {
            if i > 0 && (whole.len() - i).is_multiple_of(3) {
                grouped.push(',');
            }
            grouped.push(digit);
        }
        write!(f, "{sign}{grouped}.{:02}", abs % 100)
    }
}

// ---------- Users and permissions ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    Owner,
    Manager,
    Pharmacist,
    #[default]
    Cashier,
}

/// Everything that needs permission. Each button asks `role.can(action)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Sell,
    SellPrescription,
    VoidSale,
    ReceiveStock,
    AdjustStock,
    EditProducts,
    ViewReports,
    ManageUsers,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Owner, Role::Manager, Role::Pharmacist, Role::Cashier];

    /// The whole permission table.
    pub fn can(self, action: Action) -> bool {
        use Action::*;
        match self {
            Role::Owner => true,
            Role::Manager => action != ManageUsers,
            Role::Pharmacist => matches!(action, Sell | SellPrescription | ReceiveStock),
            Role::Cashier => action == Sell,
        }
    }

    /// The code stored in the database (never shown to users).
    pub fn code(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Manager => "manager",
            Role::Pharmacist => "pharmacist",
            Role::Cashier => "cashier",
        }
    }

    pub fn from_code(code: &str) -> Role {
        Role::ALL.into_iter().find(|r| r.code() == code).unwrap_or(Role::Cashier) // least privilege if the value is unknown
    }

    pub fn label(self) -> &'static str {
        match self {
            Role::Owner => "Owner",
            Role::Manager => "Branch manager",
            Role::Pharmacist => "Pharmacist",
            Role::Cashier => "Cashier",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct User {
    pub id: String,
    pub name: String,
    pub role: Role,
    pub active: bool,
}

/// Who is logged in, and at which branch. Every change to the database is
/// made "by" a session, so the audit log always knows who did it.
#[derive(Debug, Clone)]
pub struct Session {
    pub user: User,
    pub branch_id: String,
}

impl Session {
    pub fn can(&self, action: Action) -> bool {
        self.user.role.can(action)
    }
}

// ---------- Catalogue ----------

/// A medicine. Its stock isn't stored here: it's the sum of its batches,
/// filled in when the list is loaded.
#[derive(Debug, Clone, Default)]
pub struct Product {
    pub id: String,
    /// Short code typed at the till, like a supermarket's (e.g. "1001").
    pub code: String,
    pub name: String,
    pub category: String,
    pub barcode: Option<String>,
    pub price: Cents,
    pub reorder_level: i64,
    pub prescription_only: bool,
    /// Sellable units at this branch that are free to sell: not expired and
    /// not held for an order waiting for payment.
    pub stock: i64,
    /// Units held for orders waiting for payment.
    pub held: i64,
    /// Expired units still on the shelf, waiting to be written off.
    pub expired: i64,
    /// Earliest expiry among the sellable batches.
    pub next_expiry: Option<NaiveDate>,
    /// Unit cost and quantity of the latest batch received (to pre-fill the next one).
    pub last_cost: Option<Cents>,
    pub last_qty: Option<i64>,
}

impl Product {
    /// Search by name, category, code or barcode (an empty query matches all).
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty()
            || self.name.to_lowercase().contains(&q)
            || self.category.to_lowercase().contains(&q)
            || self.code.to_lowercase().starts_with(&q)
            || self.barcode.as_deref().is_some_and(|code| code.contains(&q))
    }

    /// True when `query` is exactly this medicine's barcode or code, the way
    /// a scanner or a cashier keying a code would enter it.
    pub fn is_exactly(&self, query: &str) -> bool {
        let q = query.trim();
        !q.is_empty() && (self.code.eq_ignore_ascii_case(q) || self.barcode.as_deref() == Some(q))
    }

    /// "1001 · Panadol 500mg"
    pub fn label(&self) -> String {
        format!("{} · {}", self.code, self.name)
    }

    pub fn expires_within(&self, today: NaiveDate, days: i64) -> bool {
        self.next_expiry.is_some_and(|d| (d - today).num_days() <= days)
    }
}

/// One batch (lot) of a medicine at this branch, with its current stock.
#[derive(Debug, Clone)]
pub struct Batch {
    pub id: String,
    pub product_id: String,
    pub product_name: String,
    pub batch_no: String,
    pub expiry: NaiveDate,
    pub cost: Cents,
    pub qty: i64,
}

// ---------- Selling ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Payment {
    #[default]
    Cash,
    Mpesa,
}

impl Payment {
    pub const ALL: [Payment; 2] = [Payment::Cash, Payment::Mpesa];

    pub fn code(self) -> &'static str {
        match self {
            Payment::Cash => "cash",
            Payment::Mpesa => "mpesa",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Payment::Cash => "Cash",
            Payment::Mpesa => "M-Pesa",
        }
    }
}

/// A cart line: which medicine and how many. Price, batch and cost are
/// looked up when the sale is recorded, so they can't go stale.
#[derive(Debug, Clone)]
pub struct CartLine {
    pub product_id: String,
    pub qty: i64,
}

/// One seller's takings in a shift.
#[derive(Debug, Clone, Default)]
pub struct SellerTotals {
    pub name: String,
    pub sales: i64,
    pub cash: Cents,
    pub mpesa: Cents,
}

impl SellerTotals {
    pub fn total(&self) -> Cents {
        self.cash + self.mpesa
    }
}

/// The till's shift. Everyone who logs in at this till sells into the open
/// shift. `sellers` totals the sales each person made (served the customer);
/// `cashiers` totals the money each person took at payment.
#[derive(Debug, Clone)]
pub struct Shift {
    pub id: String,
    pub opened_at: String,
    pub opened_by: String,
    pub sellers: Vec<SellerTotals>,
    pub cashiers: Vec<SellerTotals>,
    /// Orders still waiting for payment.
    pub waiting: i64,
}

impl Shift {
    /// Everyone together.
    pub fn totals(&self) -> SellerTotals {
        total_of(&self.sellers)
    }
}

/// One "All" line adding up per-person totals.
pub fn total_of(people: &[SellerTotals]) -> SellerTotals {
    people.iter().fold(SellerTotals { name: "All".into(), ..Default::default() }, |t, s| SellerTotals {
        sales: t.sales + s.sales,
        cash: t.cash + s.cash,
        mpesa: t.mpesa + s.mpesa,
        ..t
    })
}

/// An order a seller sent to the cashier, waiting for payment.
#[derive(Debug, Clone)]
pub struct Order {
    pub id: String,
    /// Today's order number at this branch (1, 2, 3, ...).
    pub number: i64,
    pub seller: String,
    pub at: String,
    pub lines: Vec<CartLine>,
    pub total: Cents,
}

/// Everything printed on a receipt.
#[derive(Debug, Clone)]
pub struct Receipt {
    pub number: String,
    pub order: Option<i64>,
    pub at: String,
    pub seller: String,
    pub cashier: String,
    pub payment: String,
    pub mpesa_code: Option<String>,
    pub paid: Option<Cents>,
    /// (medicine, quantity, unit price)
    pub lines: Vec<(String, i64, Cents)>,
    pub total: Cents,
    pub void: bool,
}

// ---------- Receiving stock ----------

#[derive(Debug, Clone, Default)]
pub struct DeliveryLine {
    /// Set when these units top up a batch already on the shelf (same batch
    /// number and expiry); `None` makes a new batch.
    pub batch_id: Option<String>,
    pub product_id: String,
    pub product_name: String,
    pub batch_no: String,
    pub expiry: NaiveDate,
    pub qty: i64,
    pub cost: Cents,
}

#[derive(Debug, Clone, Default)]
pub struct Delivery {
    /// Optional invoice or note number, for finding it again later.
    pub invoice_no: String,
    pub lines: Vec<DeliveryLine>,
}

// ---------- Reports ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Report {
    #[default]
    SalesByDay,
    Sales,
    SalesByMedicine,
    SalesByCashier,
    SellersPerShift,
    CollectedByCashier,
    UnpaidOrders,
    SalesByPayment,
    StockOnHand,
    Reorder,
    Expiry,
    Deliveries,
    Shifts,
    Audit,
}

impl Report {
    pub const ALL: [Report; 14] = [
        Report::SalesByDay,
        Report::Sales,
        Report::SalesByMedicine,
        Report::SalesByCashier,
        Report::SellersPerShift,
        Report::CollectedByCashier,
        Report::UnpaidOrders,
        Report::SalesByPayment,
        Report::StockOnHand,
        Report::Reorder,
        Report::Expiry,
        Report::Deliveries,
        Report::Shifts,
        Report::Audit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Report::SalesByDay => "Sales & profit by day",
            Report::Sales => "Sales (receipts)",
            Report::SalesByMedicine => "Sales by medicine",
            Report::SalesByCashier => "Sales by seller",
            Report::SellersPerShift => "Sellers in each shift",
            Report::CollectedByCashier => "Money taken by cashier",
            Report::UnpaidOrders => "Orders not paid",
            Report::SalesByPayment => "Sales by payment",
            Report::StockOnHand => "Stock on hand",
            Report::Reorder => "Reorder list (below 50)",
            Report::Expiry => "Expiring in 90 days",
            Report::Deliveries => "Stock checked in",
            Report::Shifts => "Shifts (Z reports)",
            Report::Audit => "Audit log",
        }
    }

    /// Stock reports show the present moment; the rest use the date range.
    pub fn uses_dates(self) -> bool {
        !matches!(self, Report::StockOnHand | Report::Reorder | Report::Expiry)
    }
}

/// Any report, ready to show or export. `keys[i]` identifies row `i`
/// (e.g. the sale id behind a receipt row) and is never displayed.
#[derive(Debug, Clone, Default)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub keys: Vec<String>,
}

/// The numbers at the top of the Reports screen.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub today_sales: Cents,
    pub today_profit: Cents,
    pub low_stock: i64,
    pub expiring_soon: i64,
    pub expired_value: Cents,
}
