//! Checks for everything typed into a form. Each returns the clean value or
//! a message to show the user.

use chrono::NaiveDate;

use crate::models::Cents;

/// Required, and not just digits (catches a phone number in a name field).
pub fn parse_name(label: &str, raw: &str) -> Result<String, String> {
    let text = raw.trim();
    if text.is_empty() {
        return Err(format!("{label} is required."));
    }
    if text.chars().all(|c| c.is_ascii_digit() || c.is_whitespace()) {
        return Err(format!("{label} looks like a number, not a name."));
    }
    Ok(text.to_string())
}

/// An amount like `250`, `250.5` or `1,250.00`, read straight into cents
/// (no floats, so nothing is lost to rounding). Zero is allowed.
pub fn parse_money(label: &str, raw: &str) -> Result<Cents, String> {
    let text = raw.trim().replace(',', "");
    let err = || format!("{label} must be an amount like 250.00");
    let (whole, frac) = text.split_once('.').unwrap_or((&text, ""));
    if (whole.is_empty() && frac.is_empty())
        || frac.len() > 2
        || !whole.chars().chain(frac.chars()).all(|c| c.is_ascii_digit())
    {
        return Err(err());
    }
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| err())? };
    let frac: i64 = format!("{frac:0<2}").parse().map_err(|_| err())?;
    Ok(Cents(whole * 100 + frac))
}

/// Like `parse_money`, but must be more than zero (a selling price).
pub fn parse_price(label: &str, raw: &str) -> Result<Cents, String> {
    let price = parse_money(label, raw)?;
    if price.0 == 0 {
        return Err(format!("{label} must be more than 0."));
    }
    Ok(price)
}

/// A whole number, 0 or more.
pub fn parse_quantity(label: &str, raw: &str) -> Result<i64, String> {
    raw.trim()
        .parse::<i64>()
        .ok()
        .filter(|n| *n >= 0)
        .ok_or_else(|| format!("{label} must be a whole number (0 or more)."))
}

/// A whole number, 1 or more.
pub fn parse_positive_quantity(label: &str, raw: &str) -> Result<i64, String> {
    match parse_quantity(label, raw)? {
        0 => Err(format!("{label} must be more than 0.")),
        n => Ok(n),
    }
}

/// An expiry date the way boxes print it: `06/2027`, `06/27`, `2027-06`
/// (the last day of that month), `30/06/2027` or `2027-06-30`. It must not
/// be in the past (expired stock can't be received).
pub fn parse_expiry(raw: &str, today: NaiveDate) -> Result<NaiveDate, String> {
    let text = raw.trim();
    let date = NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(text, "%d/%m/%Y"))
        .ok()
        .or_else(|| month_end(text))
        .ok_or_else(|| "Expiry must be a date like 06/2027 or 2027-06-30.".to_string())?;
    if date < today {
        return Err("That expiry date has already passed — this stock can't be sold.".to_string());
    }
    Ok(date)
}

/// "06/2027", "6/27", "2027-06" or "2027/06" -> the last day of that month.
fn month_end(text: &str) -> Option<NaiveDate> {
    let parts: Vec<&str> = text.split(['/', '-', '.']).collect();
    let [a, b] = parts[..] else { return None };
    let (year, month) = if a.len() == 4 { (a, b) } else { (b, a) };
    let mut year: i32 = year.parse().ok()?;
    if year < 100 {
        year += 2000;
    }
    let month: u32 = month.parse().ok()?;
    let next =
        if month == 12 { NaiveDate::from_ymd_opt(year + 1, 1, 1) } else { NaiveDate::from_ymd_opt(year, month + 1, 1) };
    next?.pred_opt()
}

/// A medicine's short code: optional (`None` = give it the next number),
/// up to 12 letters, digits or dashes, stored in capitals.
pub fn parse_code(raw: &str) -> Result<Option<String>, String> {
    let code = raw.trim().to_uppercase();
    if code.is_empty() {
        return Ok(None);
    }
    if code.len() > 12 || !code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Code should be up to 12 letters or digits, e.g. 1001 or PAN500.".to_string());
    }
    Ok(Some(code))
}

/// An M-Pesa transaction code (optional): 8–12 letters and digits, e.g. QJH7XK2L9P.
pub fn parse_mpesa_code(raw: &str) -> Result<Option<String>, String> {
    let code = raw.trim().to_uppercase();
    if code.is_empty() {
        return Ok(None);
    }
    if !(8..=12).contains(&code.len()) || !code.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("The M-Pesa code should look like QJH7XK2L9P (from the customer's SMS).".to_string());
    }
    Ok(Some(code))
}

/// A date as YYYY-MM-DD (report ranges).
pub fn parse_date(label: &str, raw: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|_| format!("{label} must be a date like 2026-10-01 (YYYY-MM-DD)."))
}

/// 4 to 8 digits.
pub fn parse_pin(raw: &str) -> Result<String, String> {
    let pin = raw.trim();
    if !(4..=8).contains(&pin.len()) || !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err("The PIN must be 4 to 8 digits.".to_string());
    }
    Ok(pin.to_string())
}

/// Optional (`None` when empty). Standard retail codes (EAN-8, UPC-A,
/// EAN-13, GTIN-14) must pass their check digit, so a misread digit is
/// caught here. Shop-printed codes need 3–40 letters, digits or dashes.
pub fn parse_barcode(raw: &str) -> Result<Option<String>, String> {
    let code = raw.trim();
    if code.is_empty() {
        return Ok(None);
    }
    if code.chars().all(|c| c.is_ascii_digit()) && matches!(code.len(), 8 | 12 | 13 | 14) {
        if !gtin_check_digit_ok(code) {
            return Err(format!("Barcode {code} has a wrong check digit — scan it again."));
        }
        return Ok(Some(code.to_string()));
    }
    let valid = (3..=40).contains(&code.len()) && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !valid {
        return Err("Barcode should be 3–40 letters or digits.".to_string());
    }
    Ok(Some(code.to_string()))
}

/// From the right (without the check digit), weights 3, 1, 3, 1, ...; the
/// check digit brings the sum up to a multiple of 10.
fn gtin_check_digit_ok(code: &str) -> bool {
    let digits: Vec<u32> = code.bytes().map(|b| u32::from(b - b'0')).collect();
    let Some((check, body)) = digits.split_last() else {
        return false;
    };
    let sum: u32 = body.iter().rev().enumerate().map(|(i, d)| if i % 2 == 0 { d * 3 } else { *d }).sum();
    (10 - sum % 10) % 10 == *check
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barcodes() {
        assert_eq!(parse_barcode(" 6162004002795 "), Ok(Some("6162004002795".into())));
        assert!(parse_barcode("6162004002796").is_err());
        assert_eq!(parse_barcode(""), Ok(None));
        assert_eq!(parse_barcode("MT-000123"), Ok(Some("MT-000123".into())));
        assert!(parse_barcode("a b").is_err());
    }

    #[test]
    fn money() {
        assert_eq!(parse_money("x", "250"), Ok(Cents(25000)));
        assert_eq!(parse_money("x", "1,250.5"), Ok(Cents(125050)));
        assert_eq!(parse_money("x", "0.07"), Ok(Cents(7)));
        assert_eq!(parse_money("x", ".5"), Ok(Cents(50)));
        assert!(parse_money("x", "1.234").is_err());
        assert!(parse_money("x", "-5").is_err());
        assert!(parse_money("x", "").is_err());
        assert!(parse_price("x", "0").is_err());
        assert_eq!(Cents(123456).to_string(), "1,234.56");
        assert_eq!(Cents(-5).to_string(), "-0.05");
    }

    #[test]
    fn expiry_formats() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let d = |y, m, d| Ok(NaiveDate::from_ymd_opt(y, m, d).unwrap());
        assert_eq!(parse_expiry("06/2027", today), d(2027, 6, 30));
        assert_eq!(parse_expiry("2/28", today), d(2028, 2, 29));
        assert_eq!(parse_expiry("2027-12", today), d(2027, 12, 31));
        assert_eq!(parse_expiry("15/03/2027", today), d(2027, 3, 15));
        assert_eq!(parse_expiry("2027-03-15", today), d(2027, 3, 15));
        assert!(parse_expiry("09/2026", today).is_err(), "already expired");
        assert!(parse_expiry("13/2027", today).is_err());
        assert_eq!(parse_code(" pan500 "), Ok(Some("PAN500".into())));
        assert_eq!(parse_code(""), Ok(None));
        assert!(parse_code("a b").is_err());
        assert_eq!(parse_mpesa_code(" qjh7xk2l9p"), Ok(Some("QJH7XK2L9P".into())));
        assert!(parse_mpesa_code("12-34").is_err());
    }

    #[test]
    fn pins() {
        assert!(parse_pin("1234").is_ok());
        assert!(parse_pin("123").is_err());
        assert!(parse_pin("12a4").is_err());
    }
}
