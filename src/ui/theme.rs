use eframe::egui;

// Design tokens for MedTill's look. Centralizing these as named constants —
// rather than scattering hex literals through every screen — is what lets
// the whole app's appearance change in one place instead of a dozen.

pub const PAGE_BG: egui::Color32 = egui::Color32::from_rgb(0xf9, 0xf9, 0xf7);
pub const PANEL_BG: egui::Color32 = egui::Color32::from_rgb(0xfc, 0xfc, 0xfb);
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

/// Applies MedTill's visual theme once, at startup. Every widget in the app
/// inherits these values from the active `Style`/`Visuals` — no screen needs
/// to know a single hex code.
pub fn apply(ctx: &egui::Context) {
    // Lock the app to light mode regardless of the shop PC's OS theme —
    // otherwise a system dark-mode setting would silently override the
    // Visuals set below the next time the OS theme is read.
    ctx.set_theme(egui::ThemePreference::Light);

    let mut visuals = egui::Visuals::light();

    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.window_fill = PAGE_BG;
    visuals.panel_fill = PAGE_BG;
    visuals.faint_bg_color = PANEL_BG;
    visuals.extreme_bg_color = egui::Color32::WHITE;

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

    visuals.widgets.active.weak_bg_fill = ACCENT;
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.corner_radius = egui::CornerRadius::from(6);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT_STRONG);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);

    ctx.set_visuals(visuals);

    // `Context` has no `style()`/`set_style()` in this egui version — style
    // is split per light/dark theme instead. `all_styles_mut` mutates both
    // at once, which is what we want since the theme is locked to light.
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.window_margin = egui::Margin::same(12);
    });
}

/// A "card" — a rounded, bordered panel used to group a section (a form, a
/// table, a stat tile) so the app reads as panels rather than a flat pile of
/// widgets. Same shape as `ui.group`, just styled to MedTill's look.
pub fn card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::NONE
        .fill(PANEL_BG)
        .corner_radius(egui::CornerRadius::from(10))
        .stroke(egui::Stroke::new(1.0, GRIDLINE))
        .inner_margin(egui::Margin::same(14))
        .show(ui, add_contents)
        .inner
}

/// One stat tile: a big number over a label, optionally tinted with a status
/// color. `label`/`value` are borrowed `&str` — the tile only ever displays
/// them for the current frame and stores nothing, so there's no reason to
/// take ownership of either.
pub fn stat_tile(ui: &mut egui::Ui, label: &str, value: &str, accent: egui::Color32) {
    card(ui, |ui| {
        ui.set_min_width(150.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(value).size(26.0).strong().color(accent));
            ui.label(egui::RichText::new(label).size(13.0).color(TEXT_SECONDARY));
        });
    });
}