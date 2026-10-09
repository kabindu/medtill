use chrono::NaiveDate;

/// Rejects empty input and input that's purely numeric — the "someone
/// typed a phone number into the name field" mistake you asked about.
/// Centralized here so every screen validates names the same way instead
/// of each reimplementing its own check.
pub fn parse_name(field_label: &str, raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!("{field_label} is required."));
    }
    if trimmed.chars().all(|c| c.is_ascii_digit() || c.is_whitespace()) {
        return Err(format!(
            "{field_label} looks like a number, not a name — check what you typed."
        ));
    }
    Ok(trimmed.to_string())
}

/// A price must parse as a positive number — zero or negative prices don't
/// make sense for either a selling price or a delivery cost.
pub fn parse_price(field_label: &str, raw: &str) -> Result<f64, String> {
    let trimmed = raw.trim();
    let value: f64 = trimmed
        .parse()
        .map_err(|_| format!("{field_label} must be a number, e.g. 250.00"))?;
    if value <= 0.0 {
        return Err(format!("{field_label} must be greater than 0."));
    }
    Ok(value)
}

/// Whole numbers, zero or more — used for stock levels, where 0 is valid
/// (a product with no stock yet).
pub fn parse_quantity(field_label: &str, raw: &str) -> Result<u32, String> {
    raw.trim()
        .parse::<u32>()
        .map_err(|_| format!("{field_label} must be a whole number (0 or more)."))
}

/// Same as `parse_quantity`, but rejects zero — used where zero is
/// meaningless, like "how many units arrived in this delivery."
pub fn parse_positive_quantity(field_label: &str, raw: &str) -> Result<u32, String> {
    let value = parse_quantity(field_label, raw)?;
    if value == 0 {
        return Err(format!("{field_label} must be greater than 0."));
    }
    Ok(value)
}

/// Parses a real calendar date in YYYY-MM-DD format. `allow_past` controls
/// whether an already-expired date is accepted — false for adding new
/// sellable stock (an already-expired product shouldn't go on the shelf),
/// true for anywhere historical dates are legitimate.
pub fn parse_expiry_date(raw: &str, today: NaiveDate, allow_past: bool) -> Result<NaiveDate, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Expiry date is required (format: YYYY-MM-DD).".to_string());
    }
    let date = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").map_err(|_| {
        "Expiry date must be a real date in YYYY-MM-DD format, e.g. 2027-06-30.".to_string()
    })?;
    if !allow_past && date < today {
        return Err(
            "Expiry date is already in the past — double check it, or this batch isn't sellable stock."
                .to_string(),
        );
    }
    Ok(date)
}

/// Email is optional everywhere it's used, so empty is fine — but if
/// something was typed, it should at least look like an email.
pub fn parse_email(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if !trimmed.contains('@') || !trimmed.contains('.') {
        return Err("Email doesn't look valid — expected something like name@example.com".to_string());
    }
    Ok(trimmed.to_string())
}

/// Also optional; if present, should only contain digits and common phone
/// punctuation, not letters.
pub fn parse_phone(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let valid = trimmed
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | ' ' | '(' | ')'));
    if !valid {
        return Err("Phone number should contain only digits, spaces, +, -, ( and ).".to_string());
    }
    Ok(trimmed.to_string())
}

/// Barcode is optional (`Ok(None)` when left empty).
///
/// Digits-only codes of a standard retail length (EAN-8, UPC-A, EAN-13,
/// GTIN-14) must pass their check digit: the last digit is calculated from
/// the others, so a misread or mistyped digit is caught here instead of
/// creating a product that no scanner will ever match. Any other code (for
/// example a shop-printed label) just needs 3–40 letters, digits or dashes.
pub fn parse_barcode(raw: &str) -> Result<Option<String>, String> {
    let code = raw.trim();
    if code.is_empty() {
        return Ok(None);
    }

    let all_digits = code.chars().all(|c| c.is_ascii_digit());
    if all_digits && matches!(code.len(), 8 | 12 | 13 | 14) {
        if !gtin_check_digit_ok(code) {
            return Err(format!(
                "Barcode {code} has a wrong check digit — scan it again or check the digits."
            ));
        }
        return Ok(Some(code.to_string()));
    }

    let valid = (3..=40).contains(&code.len())
        && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !valid {
        return Err("Barcode should be 3–40 letters or digits.".to_string());
    }
    Ok(Some(code.to_string()))
}

/// GTIN check digit (EAN-8/UPC-A/EAN-13/GTIN-14): counting from the right
/// of the code without its last digit, digits are weighted 3, 1, 3, 1, ...
/// and the check digit is whatever brings the sum up to a multiple of 10.
fn gtin_check_digit_ok(code: &str) -> bool {
    let digits: Vec<u32> = code.bytes().map(|b| u32::from(b - b'0')).collect();
    let Some((check, body)) = digits.split_last() else {
        return false;
    };
    let sum: u32 = body
        .iter()
        .rev()
        .enumerate()
        .map(|(i, digit)| if i % 2 == 0 { digit * 3 } else { *digit })
        .sum();
    (10 - sum % 10) % 10 == *check
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barcode_check_digits() {
        // The code from a real box (Kenya prefix 616).
        assert_eq!(parse_barcode("6162004002795"), Ok(Some("6162004002795".to_string())));
        assert_eq!(parse_barcode("  6162004002795 "), Ok(Some("6162004002795".to_string())));
        // One digit changed: rejected.
        assert!(parse_barcode("6162004002796").is_err());
        assert!(parse_barcode("6162004002785").is_err());
        // Empty is fine (barcode is optional).
        assert_eq!(parse_barcode(""), Ok(None));
        // Shop-printed codes.
        assert_eq!(parse_barcode("MT-000123"), Ok(Some("MT-000123".to_string())));
        assert!(parse_barcode("a b").is_err());
    }
}