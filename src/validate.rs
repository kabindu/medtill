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