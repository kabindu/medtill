pub mod admin;
pub mod pos;
pub mod products;
pub mod reports;
pub mod scan;
pub mod stock;
pub mod theme;

use std::time::{Duration, Instant};

/// Slow-changing data read from the database, and when it was read.
pub type Cache<T> = Option<(Instant, T)>;

/// Reads slow-changing data (the quick picks) from the database at most
/// every 30 s instead of on every frame, so screens stay quick.
pub fn cached<T: Clone>(slot: &mut Cache<T>, load: impl FnOnce() -> crate::db::Result<T>) -> crate::db::Result<T> {
    match slot {
        Some((at, value)) if at.elapsed() < Duration::from_secs(30) => Ok(value.clone()),
        _ => {
            let value = load()?;
            *slot = Some((Instant::now(), value.clone()));
            Ok(value)
        }
    }
}
