//! Checkout on one computer: find medicines (scan, code, name or a quick
//! pick), then either take payment straight away or "Send to cashier". Sent
//! orders wait in a queue (their stock is held) until the cashier takes
//! payment. Every sale prints a receipt. The till has one shift that everyone
//! sells into; its Z report shows what each person sold and took.

use std::path::Path;

use eframe::egui;
use rusqlite::Connection;

use crate::db;
use crate::models::{
    Action, CartLine, Cents, Order, Payment, Product, Receipt, SellerTotals, Session, Shift, total_of,
};
use crate::ui::scan;
use crate::ui::theme::{self, Notice};
use crate::validate;

const CART_WIDTH: f32 = 500.0;
const RECEIPT_WIDTH: usize = 40;

#[derive(Default)]
pub struct PosState {
    cart: Vec<CartLine>,
    /// The order being paid, if the cart was loaded from the queue.
    paying: Option<Order>,
    /// The find box: a scanner types here; so does the seller (code or name).
    query: String,
    payment: Payment,
    cash_given: String,
    mpesa_code: String,
    notice: Notice,
    last_sale: Option<String>,
    confirm_cancel: Option<String>,
    closing: bool,
    /// The shift just closed, shown as its Z report until the next one opens.
    closed: Option<Shift>,
    picks: crate::ui::Cache<(Vec<String>, Vec<String>)>,
}

pub fn show(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut PosState) {
    ui.heading("Checkout");
    let loaded = (|| -> db::Result<_> {
        Ok((
            db::current_shift(conn, &s.branch_id)?,
            db::list_products(conn, &s.branch_id)?,
            crate::ui::cached(&mut st.picks, || db::quick_picks(conn, &s.branch_id, 10))?,
            db::open_orders(conn, &s.branch_id)?,
        ))
    })();
    let (shift, products, picks, orders) = match loaded {
        Ok(data) => data,
        Err(e) => return Notice::error(e).show(ui),
    };
    let Some(shift) = shift else { return open_shift(ui, conn, s, st) };

    let left = (ui.available_width() - CART_WIDTH - 24.0).max(320.0);
    theme::two_columns(
        ui,
        "pos",
        left,
        st,
        |ui, st| finder(ui, s, st, &products, &picks),
        |ui, st| {
            queue(ui, conn, s, st, &orders);
            sale(ui, conn, s, st, &products);
            shift_panel(ui, conn, s, st, &shift);
        },
    );
}

/// "12" or "12 (+3 held)" when some units wait in unpaid orders.
fn held_note(p: &Product) -> String {
    match p.held {
        0 => p.stock.to_string(),
        held => format!("{} (+{held} held)", p.stock),
    }
}

// ---------- Finding medicines ----------

fn finder(ui: &mut egui::Ui, s: &Session, st: &mut PosState, products: &[Product], picks: &(Vec<String>, Vec<String>)) {
    theme::section_title(ui, "Find medicines");
    let hint = "Scan, or type a code or name, then Enter";
    if let Some(entered) = scan::find_box(ui, &mut st.query, "pos_find", hint, true) {
        enter(st, s, products, &entered);
    }
    // Five of each; a medicine already under Popular isn't repeated under Recent.
    let recent: Vec<String> = picks.1.iter().filter(|id| !picks.0.contains(id)).cloned().collect();
    quick_picks(ui, s, st, products, "Popular", &picks.0);
    quick_picks(ui, s, st, products, "Recently sold", &recent);
    if products.is_empty() {
        return theme::muted(ui, "No medicines yet. They are added under Medicines.");
    }
    let shown: Vec<&Product> = products.iter().filter(|p| p.matches(&st.query)).collect();
    let mut clicked = None;
    let headers = ["Code", "Medicine", "Price", "In stock", ""];
    theme::long_list(ui, "pos_products", &headers, &[70.0, 0.0, 90.0, 120.0, 110.0], shown.len(), |ui, i, w| {
        let (p, h) = (shown[i], theme::ROW_HEIGHT);
        theme::cell(ui, w[0], h, |ui| theme::cell_text(ui, p.code.as_str()));
        theme::cell(ui, w[1], h, |ui| theme::cell_text(ui, egui::RichText::new(&p.name).strong()));
        theme::cell(ui, w[2], h, |ui| theme::number_cell(ui, p.price.to_string()));
        theme::cell(ui, w[3], h, |ui| {
            theme::stock_dot(ui, p.stock + p.held);
            ui.label(held_note(p));
        });
        theme::cell(ui, w[4], h, |ui| {
            if p.stock == 0 {
                ui.colored_label(theme::STATUS_CRITICAL, "Out of stock");
            } else if theme::primary_button(ui, "Add").clicked() {
                clicked = Some(i);
            }
        });
    });
    if let Some(i) = clicked {
        add(st, s, shown[i]);
    }
}

/// Enter in the find box: an exact barcode or code, or a name that matches
/// only one medicine, goes straight into the cart (like a supermarket till).
fn enter(st: &mut PosState, s: &Session, products: &[Product], entered: &str) {
    let hits: Vec<&Product> = products.iter().filter(|p| p.matches(entered)).collect();
    match (products.iter().find(|p| p.is_exactly(entered)), hits.as_slice()) {
        (Some(p), _) | (None, &[p]) => {
            add(st, s, p);
            if !matches!(st.notice, Notice::Error(_)) {
                st.query.clear();
            }
        }
        (None, []) => st.notice = Notice::error(format!("Nothing matches \"{entered}\". Check the code or the name.")),
        (None, many) => {
            st.notice =
                Notice::error(format!("{} medicines match \"{entered}\" — press Add on the right one.", many.len()))
        }
    }
}

/// A row of one-tap buttons (best sellers, or the latest sold).
fn quick_picks(ui: &mut egui::Ui, s: &Session, st: &mut PosState, products: &[Product], label: &str, ids: &[String]) {
    let picks: Vec<&Product> = ids.iter().filter_map(|id| products.iter().find(|p| &p.id == id)).take(5).collect();
    let items: Vec<(&str, bool)> = picks.iter().map(|p| (p.name.as_str(), p.stock > 0)).collect();
    if let Some(i) = theme::chip_row(ui, label, &items) {
        add(st, s, picks[i]);
    }
}

/// How many of `p` the cart may hold: the free stock, plus what the order
/// being paid already holds.
fn limit(st: &PosState, p: &Product) -> i64 {
    let own = st.paying.iter().flat_map(|o| &o.lines).filter(|l| l.product_id == p.id).map(|l| l.qty).sum::<i64>();
    p.stock + own
}

/// Adds one unit, never more than can be sold.
fn add(st: &mut PosState, s: &Session, p: &Product) {
    if p.prescription_only && !s.can(Action::SellPrescription) && st.paying.is_none() {
        st.notice = Notice::error(format!("{} is prescription-only: a pharmacist must sell it.", p.name));
        return;
    }
    let limit = limit(st, p);
    match st.cart.iter_mut().find(|line| line.product_id == p.id) {
        Some(line) if line.qty >= limit => {
            st.notice = Notice::error(format!("Only {limit} of {} can be sold.", p.name));
            return;
        }
        Some(line) => line.qty += 1,
        None if limit == 0 => {
            st.notice = Notice::error(format!("{} is out of stock.", p.name));
            return;
        }
        None => st.cart.push(CartLine { product_id: p.id.clone(), qty: 1 }),
    }
    st.notice = Notice::None;
}

// ---------- Orders waiting for payment ----------

fn queue(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut PosState, orders: &[Order]) {
    if orders.is_empty() {
        return;
    }
    theme::section_title(ui, &format!("Waiting for payment ({})", orders.len()));
    let mut take = None;
    theme::panel(ui, |ui| {
        theme::table(ui, "pos_queue", &["Order", "From", "Total (KSh)", "Waiting", ""], |ui| {
            for order in orders {
                let paying = st.paying.as_ref().is_some_and(|o| o.id == order.id);
                ui.label(egui::RichText::new(order.number.to_string()).strong());
                ui.label(&order.seller);
                theme::number_cell(ui, order.total.to_string());
                ui.label(waiting(&order.at));
                ui.horizontal(|ui| {
                    if paying {
                        ui.colored_label(theme::ACCENT_STRONG, "being paid");
                    } else if st.confirm_cancel.as_deref() == Some(order.id.as_str()) {
                        if theme::danger_button(ui, "Cancel it").clicked() {
                            st.notice = db::cancel_order(conn, s, order, "cancelled at the till")
                                .map(|()| format!("Order {} cancelled; its stock is free again.", order.number))
                                .map_err(String::from)
                                .into();
                            st.confirm_cancel = None;
                        }
                        if theme::quiet_button(ui, "Keep").clicked() {
                            st.confirm_cancel = None;
                        }
                    } else {
                        if theme::primary_button(ui, "Take payment").clicked() {
                            take = Some(order.clone());
                        }
                        if theme::quiet_button(ui, "Cancel").clicked() {
                            st.confirm_cancel = Some(order.id.clone());
                        }
                    }
                });
                ui.end_row();
            }
        });
    });
    if let Some(order) = take {
        st.cart = order.lines.clone();
        st.notice = Notice::success(format!("Order {} from {} — take the payment.", order.number, order.seller));
        st.paying = Some(order);
        (st.cash_given, st.mpesa_code) = Default::default();
    }
    ui.add_space(12.0);
}

/// "just now", "5 min", "1 h 20 min" since `at` (local time).
fn waiting(at: &str) -> String {
    let Ok(then) = chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%d %H:%M:%S") else { return String::new() };
    let minutes = (chrono::Local::now().naive_local() - then).num_minutes().max(0);
    match minutes {
        0 => "just now".into(),
        1..60 => format!("{minutes} min"),
        _ => format!("{} h {} min", minutes / 60, minutes % 60),
    }
}

// ---------- The sale ----------

fn sale(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut PosState, products: &[Product]) {
    match &st.paying {
        Some(order) => theme::section_title(ui, &format!("Order {} from {}", order.number, order.seller)),
        None => theme::section_title(ui, "Sale"),
    }
    theme::panel(ui, |ui| {
        let items: i64 = st.cart.iter().map(|line| line.qty).sum();
        theme::step(ui, 1, &format!("Cart ({items} items)"));
        let mut total = Cents(0);
        let mut change: Option<(usize, i64)> = None; // (line, +1 / -1 / 0 = remove)
        if st.cart.is_empty() {
            theme::muted(ui, "Nothing yet. Scan a box, type a code, or tap a medicine.");
        } else {
            theme::table(ui, "pos_cart", &["Medicine", "Qty", "Amount", ""], |ui| {
                for (i, line) in st.cart.iter().enumerate() {
                    let Some(p) = products.iter().find(|p| p.id == line.product_id) else { continue };
                    let amount = p.price * line.qty;
                    total = total + amount;
                    ui.label(&p.name);
                    ui.horizontal(|ui| {
                        if ui.button("−").clicked() {
                            change = Some((i, -1));
                        }
                        ui.label(egui::RichText::new(line.qty.to_string()).strong());
                        if ui.add_enabled(line.qty < limit(st, p), egui::Button::new("+")).clicked() {
                            change = Some((i, 1));
                        }
                    });
                    theme::number_cell(ui, amount.to_string());
                    if theme::quiet_button(ui, "Remove").clicked() {
                        change = Some((i, 0));
                    }
                    ui.end_row();
                }
            });
        }
        if let Some((i, delta)) = change {
            st.cart[i].qty += delta;
            if delta == 0 || st.cart[i].qty == 0 {
                st.cart.remove(i);
            }
            st.notice = Notice::None;
        }
        if st.paying.is_none() && !st.cart.is_empty() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Send to cashier").clicked() {
                    st.notice = match db::place_order(conn, s, &st.cart) {
                        Ok(number) => {
                            st.cart.clear();
                            Notice::success(format!(
                                "Order {number} sent. It waits above until the cashier takes payment."
                            ))
                        }
                        Err(e) => Notice::error(e),
                    };
                }
                theme::muted(ui, "or take the payment below");
            });
        }

        theme::step(ui, 2, "Payment");
        theme::big_total(ui, format!("Total  KSh {total}"), theme::TEXT_PRIMARY);
        ui.horizontal(|ui| {
            for payment in Payment::ALL {
                if theme::chip(ui, payment.label(), st.payment == payment).clicked() {
                    st.payment = payment;
                }
            }
        });
        let cash = cash_given(st, total);
        theme::form(ui, "pos_payment", |ui| match st.payment {
            Payment::Cash => {
                theme::field(ui, "Cash received", &mut st.cash_given, "optional, e.g. 1000");
            }
            Payment::Mpesa => {
                theme::field(ui, "M-Pesa code", &mut st.mpesa_code, "from the customer's SMS, e.g. QJH7XK2L9P");
            }
        });
        if st.payment == Payment::Cash {
            match &cash {
                Ok(Some(given)) => {
                    theme::big_total(ui, format!("Change  KSh {}", *given - total), theme::ACCENT_STRONG)
                }
                Err(e) => {
                    ui.colored_label(theme::STATUS_CRITICAL, e);
                }
                Ok(None) => {}
            }
        }
        if theme::big_button(ui, &format!("Complete sale  ·  KSh {total}")).clicked() {
            st.notice = complete(conn, s, st, cash).into();
        }
        theme::actions(ui, |ui| {
            let clear = if st.paying.is_some() { "Back to the queue" } else { "Clear cart" };
            if (!st.cart.is_empty() || st.paying.is_some()) && theme::quiet_button(ui, clear).clicked() {
                st.cart.clear();
                st.paying = None;
                st.notice = Notice::None;
            }
            if let Some(sale_id) = st.last_sale.clone()
                && ui.button("Reprint last receipt").clicked()
            {
                st.notice = print_receipt(conn, &sale_id).into();
            }
        });
        st.notice.show(ui);
    });
}

/// The cash typed in, if any; an error if it can't cover `total`.
fn cash_given(st: &PosState, total: Cents) -> Result<Option<Cents>, String> {
    if st.payment != Payment::Cash || st.cash_given.trim().is_empty() {
        return Ok(None);
    }
    let given = validate::parse_money("Cash received", &st.cash_given)?;
    if given < total {
        return Err(format!("Cash received is KSh {} short.", total - given));
    }
    Ok(Some(given))
}

fn complete(
    conn: &Connection,
    s: &Session,
    st: &mut PosState,
    cash: Result<Option<Cents>, String>,
) -> Result<String, String> {
    let paid = cash?;
    let mpesa = match st.payment {
        Payment::Mpesa => validate::parse_mpesa_code(&st.mpesa_code)?,
        Payment::Cash => None,
    };
    let order = st.paying.as_ref().map(|o| o.id.as_str());
    let sale_id = db::record_sale(conn, s, order, &st.cart, st.payment, paid, mpesa.as_deref())?;
    st.cart.clear();
    st.paying = None;
    (st.cash_given, st.mpesa_code) = Default::default();
    let total = db::receipt(conn, &sale_id)?.total;
    let change = paid.map(|p| format!(" Change KSh {}.", p - total)).unwrap_or_default();
    let printed = print_receipt(conn, &sale_id)?;
    st.last_sale = Some(sale_id);
    Ok(format!("Sale complete: KSh {total}.{change} {printed}"))
}

// ---------- Receipts ----------

/// Saves the receipt as `receipts/<number>.txt` and, when switched on in
/// Staff & settings, prints it on the default Windows printer.
pub fn print_receipt(conn: &Connection, sale_id: &str) -> Result<String, String> {
    let r = db::receipt(conn, sale_id)?;
    let text = receipt_text(
        &r,
        &db::branch_name(conn)?,
        &db::setting(conn, "receipt_header")?,
        &db::setting(conn, "receipt_footer")?,
    );
    let path = Path::new("receipts").join(format!("{}.txt", r.number));
    std::fs::create_dir_all("receipts")
        .and_then(|()| std::fs::write(&path, text))
        .map_err(|e| format!("Could not save the receipt: {e}"))?;
    if db::setting(conn, "print_receipts")? != "1" {
        return Ok("Receipt saved.".into());
    }
    Ok(match send_to_printer(&path) {
        Ok(()) => "Printing the receipt.".into(),
        Err(e) => format!("Receipt saved, but it could not be printed: {e}"),
    })
}

/// A 40-character-wide receipt (fits 80 mm receipt printers and A4).
fn receipt_text(r: &Receipt, branch: &str, header: &str, footer: &str) -> String {
    let w = RECEIPT_WIDTH;
    let center = |text: &str| format!("{text:^w$}\n");
    let row = |left: &str, right: &str| format!("{left}{right:>pad$}\n", pad = w.saturating_sub(left.chars().count()));
    let rule = format!("{}\n", "-".repeat(w));
    let mut out = center(branch);
    if !header.is_empty() {
        out += &center(header);
    }
    if r.void {
        out += &center("*** VOID ***");
    }
    out += &rule;
    out += &row(&format!("Receipt {}", r.number), r.at.get(..16).unwrap_or(&r.at));
    if let Some(order) = r.order {
        out += &format!("Order {order}\n");
    }
    out += &format!("Served by {}\n", r.seller);
    if r.cashier != r.seller {
        out += &format!("Cashier {}\n", r.cashier);
    }
    out += &rule;
    for (name, qty, price) in &r.lines {
        out += &format!("{name}\n");
        out += &row(&format!("  {qty} x {price}"), &(*price * *qty).to_string());
    }
    out += &rule;
    out += &row("TOTAL  KSh", &r.total.to_string());
    out += &row(&format!("Paid by {}", r.payment), &r.paid.unwrap_or(r.total).to_string());
    if let Some(paid) = r.paid {
        out += &row("Change", &(paid - r.total).to_string());
    }
    if let Some(code) = &r.mpesa_code {
        out += &row("M-Pesa code", code);
    }
    out += &rule;
    if !footer.is_empty() {
        out += &center(footer);
    }
    out
}

/// Prints a text file on the default printer with Windows PowerShell's
/// `Out-Printer`, without a window flashing up.
fn send_to_printer(path: &Path) -> std::io::Result<()> {
    let full = std::env::current_dir()?.join(path);
    let script =
        format!("Get-Content -Raw -LiteralPath '{}' | Out-Printer", full.display().to_string().replace('\'', "''"));
    let mut command = std::process::Command::new("powershell");
    command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command.spawn().map(|_| ())
}

// ---------- The shift ----------

fn shift_panel(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut PosState, shift: &Shift) {
    ui.add_space(16.0);
    theme::panel(ui, |ui| {
        theme::subheading(ui, "This shift");
        theme::muted(ui, format!("Opened {} by {}", shift.opened_at, shift.opened_by));
        shift_totals(ui, "shift", shift);
        if !st.closing {
            if ui.button("Close shift").clicked() {
                st.closing = true;
            }
            return;
        }
        theme::muted(ui, "Closing ends the shift for everyone at this till.");
        if shift.waiting > 0 {
            ui.colored_label(
                theme::STATUS_SERIOUS,
                format!("{} unpaid orders will be cancelled and their stock freed.", shift.waiting),
            );
        }
        theme::actions(ui, |ui| {
            if theme::primary_button(ui, "Yes, close the shift").clicked() {
                match db::close_shift(conn, s) {
                    Ok(closed) => *st = PosState { closed: Some(closed), ..Default::default() },
                    Err(e) => st.notice = Notice::error(e),
                }
            }
            if theme::quiet_button(ui, "Cancel").clicked() {
                st.closing = false;
            }
        });
    });
}

/// Money taken per cashier, and sales made per seller, each with an "All" line.
fn shift_totals(ui: &mut egui::Ui, id: &str, shift: &Shift) {
    if shift.sellers.is_empty() {
        return theme::muted(ui, "No sales yet.");
    }
    let table = |ui: &mut egui::Ui, id: String, who: &str, people: &[SellerTotals]| {
        let all = total_of(people);
        theme::table(ui, &id, &[who, "Sales", "Cash (KSh)", "M-Pesa (KSh)", "Total (KSh)"], |ui| {
            for t in people.iter().chain([&all]) {
                ui.label(&t.name);
                theme::number_cell(ui, t.sales.to_string());
                theme::number_cell(ui, t.cash.to_string());
                theme::number_cell(ui, t.mpesa.to_string());
                theme::number_cell(ui, t.total().to_string());
                ui.end_row();
            }
        });
    };
    table(ui, format!("{id}_cashiers"), "Money taken by", &shift.cashiers);
    ui.add_space(6.0);
    table(ui, format!("{id}_sellers"), "Sold by", &shift.sellers);
}

fn open_shift(ui: &mut egui::Ui, conn: &Connection, s: &Session, st: &mut PosState) {
    ui.set_max_width(680.0);
    if let Some(z) = &st.closed {
        theme::panel(ui, |ui| {
            theme::subheading(ui, "Shift closed — Z report");
            theme::muted(ui, format!("Opened {} by {}", z.opened_at, z.opened_by));
            shift_totals(ui, "z", z);
        });
        ui.add_space(12.0);
    }
    theme::panel(ui, |ui| {
        theme::subheading(ui, "Open the shift");
        theme::muted(
            ui,
            "Everyone who logs in at this till sells into the open shift. \
             What each person sells and takes is added up separately until the shift is closed.",
        );
        if theme::big_button(ui, "Open shift").clicked() {
            match db::open_shift(conn, s) {
                Ok(()) => *st = PosState::default(),
                Err(e) => st.notice = Notice::error(e),
            }
        }
        st.notice.show(ui);
    });
}
