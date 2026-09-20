use eframe::egui;

/// A form's status message: nothing to show, a success, or an error. Using
/// an enum instead of a plain `String` (which every screen used before)
/// makes "no message" and "message" mutually exclusive by construction,
/// instead of relying on checking for an empty string, and lets every
/// screen render its messages the same color-coded way.
#[derive(Default)]
pub enum Notice {
    #[default]
    None,
    Success(String),
    Error(String),
}

impl Notice {
    pub fn success(msg: impl Into<String>) -> Self {
        Notice::Success(msg.into())
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Notice::Error(msg.into())
    }

    /// Borrows `&self` — drawing a message doesn't require consuming or
    /// resetting it; each screen decides when to clear it (typically: the
    /// next time the user edits the form or a new action succeeds).
    pub fn show(&self, ui: &mut egui::Ui) {
        match self {
            Notice::None => {}
            Notice::Success(msg) => {
                ui.colored_label(egui::Color32::from_rgb(46, 125, 50), format!("Success: {msg}"));
            }
            Notice::Error(msg) => {
                ui.colored_label(egui::Color32::from_rgb(198, 40, 40), format!("Error: {msg}"));
            }
        }
    }
}