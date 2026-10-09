use eframe::egui;

// Design tokens for MedTill's look. Centralizing these as named constants —
// rather than scattering hex literals through every screen — is what lets
// the whole app's appearance change in one place instead of a dozen.

pub const PAGE_BG: egui::Color32 = egui::Color32::from_rgb(0xf2, 0xf4, 0xf7);
pub const PANEL_BG: egui::Color32 = egui::Color32::WHITE;
/// Background of text-entry boxes, so they read as fields inside white cards.
pub const FIELD_BG: egui::Color32 = egui::Color32::from_rgb(0xf7, 0xf9, 0xfc);
pub const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(0x0b, 0x0b, 0x0b);
pub const TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(0x52, 0x51, 0x4e);
pub const GRIDLINE: egui::Color32 = egui::Color32::from_rgb(0xe1, 0xe0, 0xd9);
pub const BASELINE: egui::Color32 = egui::Color32::from_rgb(0xc3, 0xc2, 0xb7);

pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x2a, 0x78, 0xd6);
pub const ACCENT_STRONG: egui::Color32 = egui::Color32::from_rgb(0x18, 0x4f, 0x95);
pub const ACCENT_WASH: egui::Color32 = egui::Color32::from_rgb(0xe7, 0xef, 0xfb);
pub const STRIPE_BG: egui::Color32 = egui::Color32::from_rgb(0xf1, 0xf5, 0xfb);

// Status colors are reserved for alerts (low stock, expiring soon) and never
// reused as a "just another color" — each one always ships next to a text
// label, never standing alone.
pub const STATUS_GOOD: egui::Color32 = egui::Color32::from_rgb(0x0c, 0xa3, 0x0c);
pub const STATUS_WARNING: egui::Color32 = egui::Color32::from_rgb(0xfa, 0xb2, 0x19);
pub const STATUS_SERIOUS: egui::Color32 = egui::Color32::from_rgb(0xec, 0x83, 0x5a);
pub const STATUS_CRITICAL: egui::Color32 = egui::Color32::from_rgb(0xd0, 0x3b, 0x3b);

/// Loads Windows' own UI font (Segoe UI) so MedTill matches the rest of the
/// shop PC's interface instead of egui's built-in fallback font. Fails
/// quietly (keeps egui's default font) if the file can't be read.
fn load_windows_fonts(ctx: &egui::Context) {
    let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") else {
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "segoe_ui".to_owned(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "segoe_ui".to_owned());

    ctx.set_fonts(fonts);
}

/// Applies MedTill's visual theme once, at startup. Every widget in the app
/// inherits these values from the active `Style`/`Visuals` — no screen needs
/// to know a single hex code, font, or size.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Light);
    load_windows_fonts(ctx);

    let mut visuals = egui::Visuals::light();

    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.window_fill = PAGE_BG;
    visuals.panel_fill = PAGE_BG;
    visuals.faint_bg_color = PANEL_BG;
    visuals.extreme_bg_color = egui::Color32::WHITE;
    visuals.text_edit_bg_color = Some(FIELD_BG);

    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = STATUS_SERIOUS;
    visuals.error_fg_color = STATUS_CRITICAL;

    visuals.widgets.noninteractive.bg_fill = PANEL_BG;
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::from(6);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, GRIDLINE);

    visuals.widgets.inactive.weak_bg_fill = PANEL_BG;
    visuals.widgets.inactive.bg_fill = PANEL_BG;
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::from(6);
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BASELINE);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT_SECONDARY);

    visuals.widgets.hovered.weak_bg_fill = ACCENT_WASH;
    visuals.widgets.hovered.bg_fill = ACCENT_WASH;
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::from(6);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, ACCENT_STRONG);

    // "Active" = a button while it's being pressed. Its text color also
    // doubles as egui's color for *all* bold (`.strong()`) text, so it must
    // stay dark: when it was white, every bold heading ("Add supplier",
    // "Log a delivery", "Cart", "Total") rendered white-on-white and
    // vanished.
    visuals.widgets.active.weak_bg_fill = ACCENT_WASH;
    visuals.widgets.active.bg_fill = ACCENT_WASH;
    visuals.widgets.active.corner_radius = egui::CornerRadius::from(6);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT_STRONG);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, TEXT_PRIMARY);

    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        // Text sizes. Body is the one that matters most: it sets form
        // labels and the text you type into boxes. To make the whole app a
        // touch bigger or smaller, nudge Body and Button together
        // (e.g. 18 -> 20) and leave the rest. Lists and tables use the
        // smaller sizes in `compact_list` below instead.
        style.text_styles = [
            (egui::TextStyle::Small, egui::FontId::new(14.0, egui::FontFamily::Proportional)),
            (egui::TextStyle::Body, egui::FontId::new(18.0, egui::FontFamily::Proportional)),
            (egui::TextStyle::Monospace, egui::FontId::new(16.0, egui::FontFamily::Monospace)),
            (egui::TextStyle::Button, egui::FontId::new(18.0, egui::FontFamily::Proportional)),
            (egui::TextStyle::Heading, egui::FontId::new(26.0, egui::FontFamily::Proportional)),
        ]
            .into();

        // Box sizes: comfortable to click (or tap on a touchscreen),
        // without swallowing the screen.
        style.spacing.interact_size = egui::vec2(44.0, 34.0);
        style.spacing.button_padding = egui::vec2(14.0, 8.0);
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.icon_width = 18.0;
        style.spacing.icon_spacing = 8.0;
        style.spacing.window_margin = egui::Margin::same(12);
        style.spacing.text_edit_width = 280.0;
    });
}

/// Makes a list or table a notch smaller than the rest of the app, so more
/// rows fit on screen. Call it at the top of the closure that draws the list
/// (inside a `card`, `ScrollArea`, etc.): it only changes the style of that
/// inner area, never the forms or the rest of the screen.
///
/// To make every list in the app bigger or smaller at once, change the two
/// 14.0 values below.
pub fn compact_list(ui: &mut egui::Ui) {
    let style = ui.style_mut();
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(14.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(14.0, egui::FontFamily::Proportional),
    );
    style.spacing.button_padding = egui::vec2(6.0, 2.0);
    style.spacing.interact_size = egui::vec2(24.0, 22.0);
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
}

/// The section title above a card ("Add product", "Cart", "Log a delivery").
pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong().size(20.0).color(TEXT_PRIMARY));
    ui.add_space(4.0);
}

/// A smaller heading inside a panel ("Delivery details", "Add a product line").
pub fn subheading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong().size(16.0).color(ACCENT_STRONG));
    ui.add_space(2.0);
}

/// Width of the label column in every form, so the input boxes in stacked
/// forms all start at the same line.
pub const LABEL_COL: f32 = 120.0;

/// A form field's label ("Name", "Quantity", ...), muted so it reads as a
/// label rather than as data.
pub fn form_label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(TEXT_SECONDARY));
}

/// A text box that stretches to fill the rest of its form row.
pub fn wide_text_edit(ui: &mut egui::Ui, text: &mut String, hint: &str) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    )
}

/// Like `card`, but always stretches to the full width of where it's placed,
/// so cards stacked in the same column line up on both edges.
pub fn panel<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::NONE
        .fill(PANEL_BG)
        .corner_radius(egui::CornerRadius::from(10))
        .stroke(egui::Stroke::new(1.0, GRIDLINE))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui)
        })
        .inner
}

/// A "card" — a rounded, bordered panel used to group a section (a form, a
/// table, a stat tile) so the app reads as panels rather than a flat pile of
/// widgets. Same shape as `ui.group`, just styled to MedTill's look.
pub fn card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::NONE
        .fill(PANEL_BG)
        .corner_radius(egui::CornerRadius::from(10))
        .stroke(egui::Stroke::new(1.0, GRIDLINE))
        .inner_margin(egui::Margin::same(16))
        .show(ui, add_contents)
        .inner
}

/// The main button of a form ("Add product", "Complete sale", ...): always
/// filled with the accent color so it's obvious which button commits.
pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE)).fill(ACCENT),
    )
}

/// The "Yes" button in a "Delete? Yes / No" confirmation.
pub fn danger_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE))
            .fill(STATUS_CRITICAL),
    )
}

/// Zebra striping for a list `Grid`: skips the header row (0) and tints
/// every other data row. Pass to `Grid::with_row_color`.
pub fn stripe_rows(row: usize, _style: &egui::Style) -> Option<egui::Color32> {
    if row > 0 && row % 2 == 0 {
        Some(STRIPE_BG)
    } else {
        None
    }
}

/// A bold, muted column header in a list `Grid`.
pub fn column_header(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong().color(TEXT_SECONDARY));
}

/// A number, right-aligned inside its table cell so columns of figures line
/// up on the decimal point.
pub fn number_cell(ui: &mut egui::Ui, text: String) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(text);
    });
}

/// One stat tile: a big number over a label, optionally tinted with a status
/// color. `label`/`value` are borrowed `&str` — the tile only ever displays
/// them for the current frame and stores nothing, so there's no reason to
/// take ownership of either.
pub fn stat_tile(ui: &mut egui::Ui, label: &str, value: &str, accent: egui::Color32) {
    card(ui, |ui| {
        ui.set_min_width(220.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(value).size(34.0).strong().color(accent));
            ui.label(egui::RichText::new(label).size(15.0).color(TEXT_SECONDARY));
        });
    });
}