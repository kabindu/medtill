//! MedTill's look, plus the small building blocks every screen shares
//! (cards, forms, lists, messages), so screens stay short and consistent.

use eframe::egui;

use crate::models::Table;

pub const PAGE_BG: egui::Color32 = egui::Color32::from_rgb(0xf2, 0xf4, 0xf7);
pub const PANEL_BG: egui::Color32 = egui::Color32::WHITE;
pub const FIELD_BG: egui::Color32 = egui::Color32::from_rgb(0xf7, 0xf9, 0xfc);
pub const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(0x0b, 0x0b, 0x0b);
pub const TEXT_SECONDARY: egui::Color32 = egui::Color32::from_rgb(0x52, 0x51, 0x4e);
pub const GRIDLINE: egui::Color32 = egui::Color32::from_rgb(0xe1, 0xe0, 0xd9);
pub const BASELINE: egui::Color32 = egui::Color32::from_rgb(0xc3, 0xc2, 0xb7);
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x2a, 0x78, 0xd6);
pub const ACCENT_STRONG: egui::Color32 = egui::Color32::from_rgb(0x18, 0x4f, 0x95);
pub const ACCENT_WASH: egui::Color32 = egui::Color32::from_rgb(0xe7, 0xef, 0xfb);
const ACCENT_PRESSED: egui::Color32 = egui::Color32::from_rgb(0xd3, 0xe2, 0xf7);
const DANGER_STRONG: egui::Color32 = egui::Color32::from_rgb(0xa8, 0x2c, 0x2c);
pub const STRIPE_BG: egui::Color32 = egui::Color32::from_rgb(0xf1, 0xf5, 0xfb);

// Status colours are only for alerts, and always sit next to a text label.
pub const STATUS_GOOD: egui::Color32 = egui::Color32::from_rgb(0x0c, 0xa3, 0x0c);
pub const STATUS_SERIOUS: egui::Color32 = egui::Color32::from_rgb(0xe6, 0x77, 0x00);
pub const STATUS_CRITICAL: egui::Color32 = egui::Color32::from_rgb(0xd0, 0x3b, 0x3b);

/// Use Windows' own UI font (Segoe UI); keep egui's font if it can't be read.
fn load_windows_font(ctx: &egui::Context) {
    let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") else { return };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("segoe_ui".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "segoe_ui".into());
    }
    ctx.set_fonts(fonts);
}

/// Applied once at startup; every widget inherits it.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Light);
    load_windows_font(ctx);

    let mut v = egui::Visuals::light();
    v.override_text_color = Some(TEXT_PRIMARY);
    v.window_fill = PAGE_BG;
    v.panel_fill = PAGE_BG;
    v.faint_bg_color = PANEL_BG;
    v.extreme_bg_color = egui::Color32::WHITE;
    v.text_edit_bg_color = Some(FIELD_BG);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = STATUS_SERIOUS;
    v.error_fg_color = STATUS_CRITICAL;
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);

    // Plain `ui.button`s: white with a grey border, blue under the pointer.
    let round = egui::CornerRadius::from(8);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL_BG;
    w.noninteractive.bg_stroke = egui::Stroke::new(1.0, GRIDLINE);
    w.inactive.weak_bg_fill = PANEL_BG;
    w.inactive.bg_fill = PANEL_BG;
    w.inactive.bg_stroke = egui::Stroke::new(1.0, BASELINE);
    w.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT_SECONDARY);
    w.hovered.weak_bg_fill = ACCENT_WASH;
    w.hovered.bg_fill = ACCENT_WASH;
    w.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    w.hovered.fg_stroke = egui::Stroke::new(1.0, ACCENT_STRONG);
    w.active.weak_bg_fill = ACCENT_PRESSED;
    w.active.bg_fill = ACCENT_WASH;
    w.active.bg_stroke = egui::Stroke::new(1.0, ACCENT_STRONG);
    // egui also uses this colour for all bold text, so it must stay dark.
    w.active.fg_stroke = egui::Stroke::new(1.0, TEXT_PRIMARY);
    for state in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active] {
        state.corner_radius = round;
    }
    ctx.set_visuals(v);

    ctx.all_styles_mut(|style| {
        // To make the whole app bigger or smaller, change Body and Button together.
        let font = |size| egui::FontId::new(size, egui::FontFamily::Proportional);
        style.text_styles = [
            (egui::TextStyle::Small, font(14.0)),
            (egui::TextStyle::Body, font(18.0)),
            (egui::TextStyle::Monospace, egui::FontId::new(16.0, egui::FontFamily::Monospace)),
            (egui::TextStyle::Button, font(18.0)),
            (egui::TextStyle::Heading, font(26.0)),
        ]
        .into();
        let s = &mut style.spacing;
        s.interact_size = egui::vec2(44.0, 34.0);
        s.button_padding = egui::vec2(16.0, 8.0);
        s.item_spacing = egui::vec2(10.0, 10.0);
        s.icon_width = 18.0;
        s.icon_spacing = 8.0;
        s.window_margin = egui::Margin::same(12);
        s.text_edit_width = 280.0;
    });
}

/// Lists and tables are a notch smaller than forms, so more rows fit.
pub fn compact_list(ui: &mut egui::Ui) {
    let style = ui.style_mut();
    for text in [egui::TextStyle::Body, egui::TextStyle::Button] {
        style.text_styles.insert(text, egui::FontId::new(14.0, egui::FontFamily::Proportional));
    }
    style.spacing.button_padding = egui::vec2(6.0, 2.0);
    style.spacing.interact_size = egui::vec2(24.0, 22.0);
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
}

// ---------- Messages ----------

/// A form's message: nothing, a success, or an error.
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

    pub fn error(msg: impl ToString) -> Self {
        Notice::Error(msg.to_string())
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Notice::Success(_))
    }

    pub fn show(&self, ui: &mut egui::Ui) {
        match self {
            Notice::None => {}
            Notice::Success(msg) => {
                ui.colored_label(STATUS_GOOD, msg);
            }
            Notice::Error(msg) => {
                ui.colored_label(STATUS_CRITICAL, msg);
            }
        }
    }
}

/// The result of a button's action: `Ok` holds the success message.
impl From<Result<String, String>> for Notice {
    fn from(result: Result<String, String>) -> Self {
        match result {
            Ok(msg) => Notice::Success(msg),
            Err(msg) => Notice::Error(msg),
        }
    }
}

// ---------- Layout pieces ----------

pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong().size(20.0).color(TEXT_PRIMARY));
    ui.add_space(4.0);
}

pub fn subheading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong().size(16.0).color(ACCENT_STRONG));
}

/// A numbered step inside a panel ("1. Delivery details"), with a divider
/// above every step after the first, so a form reads top to bottom.
pub fn step(ui: &mut egui::Ui, number: usize, text: &str) {
    if number > 1 {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(4.0);
    }
    subheading(ui, &format!("{number}. {text}"));
}

/// A large figure such as a total.
pub fn big_total(ui: &mut egui::Ui, text: impl Into<String>, colour: egui::Color32) {
    ui.label(egui::RichText::new(text.into()).strong().size(22.0).color(colour));
}

pub fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text.into()).color(TEXT_SECONDARY));
}

/// A white rounded box that fills the width it's given.
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

/// Splits the page into a fixed-width left column and a right column that
/// takes the rest; each scrolls on its own. Both sides get the screen's state.
pub fn two_columns<T>(
    ui: &mut egui::Ui,
    id: &str,
    left_width: f32,
    state: &mut T,
    left: impl FnOnce(&mut egui::Ui, &mut T),
    right: impl FnOnce(&mut egui::Ui, &mut T),
) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(left_width);
            egui::ScrollArea::vertical().id_salt(format!("{id}_left")).show(ui, |ui| left(ui, state));
        });
        ui.separator();
        ui.vertical(|ui| {
            egui::ScrollArea::vertical().id_salt(format!("{id}_right")).show(ui, |ui| right(ui, state));
        });
    });
}

/// A two-column form: labels on the left, inputs filling the rest.
pub fn form<R>(ui: &mut egui::Ui, id: &str, add_rows: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Grid::new(id).num_columns(2).min_col_width(120.0).spacing(egui::vec2(12.0, 10.0)).show(ui, add_rows).inner
}

/// One labelled text box inside a `form`.
pub fn field(ui: &mut egui::Ui, label: &str, text: &mut String, hint: &str) -> egui::Response {
    ui.label(egui::RichText::new(label).color(TEXT_SECONDARY));
    let response = ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(f32::INFINITY));
    ui.end_row();
    response
}

/// True when Enter was pressed in this text box (it then loses focus).
pub fn enter_pressed(response: &egui::Response) -> bool {
    response.lost_focus() && response.ctx.input(|i| i.key_pressed(egui::Key::Enter))
}

/// A labelled drop-down inside a `form`, filling the row. `options` are
/// (value, label); `placeholder` shows while nothing in the list is chosen.
pub fn choice<T: PartialEq + Clone>(
    ui: &mut egui::Ui,
    label: &str,
    id: &str,
    value: &mut T,
    options: &[(T, &str)],
    placeholder: &str,
) {
    ui.label(egui::RichText::new(label).color(TEXT_SECONDARY));
    let selected = options.iter().find(|(v, _)| v == value).map_or(placeholder, |(_, l)| l);
    egui::ComboBox::from_id_salt(id).width(ui.available_width()).selected_text(selected).show_ui(ui, |ui| {
        for (v, l) in options {
            ui.selectable_value(value, v.clone(), *l);
        }
    });
    ui.end_row();
}

/// Like `field`, but the typing is hidden (PINs).
pub fn pin_field(ui: &mut egui::Ui, label: &str, text: &mut String) -> egui::Response {
    ui.label(egui::RichText::new(label).color(TEXT_SECONDARY));
    let response = ui.add(egui::TextEdit::singleline(text).password(true).hint_text("4–8 digits"));
    ui.end_row();
    response
}

pub fn search_box(ui: &mut egui::Ui, text: &mut String, hint: &str) {
    ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(f32::INFINITY));
}

/// True if `haystack` contains `query` ignoring case (an empty query matches).
pub fn matches(query: &str, haystack: &str) -> bool {
    query.is_empty() || haystack.to_lowercase().contains(&query.trim().to_lowercase())
}

// ---------- Buttons ----------

/// A colour always means the same thing on a button: blue is the main
/// action (one per form), white (a plain `ui.button`) is any other action,
/// red removes something for good, and plain blue text backs out (Cancel).
#[derive(Clone, Copy, PartialEq)]
enum Look {
    Primary,
    Danger,
    Quiet,
}

fn button(ui: &mut egui::Ui, look: Look, text: impl Into<String>) -> egui::Response {
    use egui::Color32 as C;
    // (fill at rest, fill under the pointer), (border at rest, under the pointer), text colour.
    let (fills, borders, ink) = match look {
        Look::Primary => ([ACCENT, ACCENT_STRONG], [ACCENT, ACCENT_STRONG], C::WHITE),
        Look::Danger => ([STATUS_CRITICAL, DANGER_STRONG], [STATUS_CRITICAL, DANGER_STRONG], C::WHITE),
        Look::Quiet => ([C::TRANSPARENT, ACCENT_WASH], [C::TRANSPARENT, ACCENT_WASH], ACCENT_STRONG),
    };
    let text = egui::RichText::new(text).color(ink);
    let (main, s) = (look != Look::Quiet, ui.spacing());
    let shape = Shape {
        radius: 8,
        padding: s.button_padding + egui::vec2(if main { 6.0 } else { 0.0 }, 0.0),
        height: s.interact_size.y,
    };
    styled_button(ui, fills, borders, if main { text.strong() } else { text }, shape)
}

/// A blue main action ("Save", "Complete sale").
pub fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    button(ui, Look::Primary, text)
}

/// A red action that removes something ("Yes, delete", "Void sale").
pub fn danger_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    button(ui, Look::Danger, text)
}

/// Plain text that lights up under the pointer ("Cancel", "Remove").
pub fn quiet_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    button(ui, Look::Quiet, text)
}

/// The one big action that finishes a screen's job, the full width of its
/// panel ("Complete sale", "Check in stock").
pub fn big_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add_space(4.0);
    ui.vertical_centered_justified(|ui| {
        ui.spacing_mut().interact_size.y = 46.0;
        ui.style_mut().text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(19.0));
        button(ui, Look::Primary, text)
    })
    .inner
}

/// A rounded pill for choosing one of several (login names, the payment
/// type, a date range) or for one-tap picks; filled blue when selected.
/// Its text never wraps, and long names are cut to 24 characters.
pub fn chip(ui: &mut egui::Ui, text: impl Into<String>, selected: bool) -> egui::Response {
    let (fills, ink) = match selected {
        true => ([ACCENT, ACCENT_STRONG], egui::Color32::WHITE),
        false => ([ACCENT_WASH, ACCENT_PRESSED], ACCENT_STRONG),
    };
    pill(ui, text.into(), fills, ink)
}

fn pill(ui: &mut egui::Ui, mut text: String, fills: [egui::Color32; 2], ink: egui::Color32) -> egui::Response {
    if let Some((cut, _)) = text.char_indices().nth(24) {
        text = format!("{}…", &text[..cut]);
    }
    let shape = Shape { radius: 15, padding: egui::vec2(14.0, 4.0), height: 30.0 };
    styled_button(ui, fills, fills, egui::RichText::new(text).color(ink), shape)
}

/// A small status pill: a coloured dot and a word or two on a pale tint of
/// the same colour ("● Main · 2 online"). Not a button; hover for details.
pub fn status_badge(ui: &mut egui::Ui, colour: egui::Color32, text: &str) -> egui::Response {
    egui::Frame::NONE
        .fill(colour.gamma_multiply(0.12))
        .stroke(egui::Stroke::new(1.0, colour.gamma_multiply(0.45)))
        .corner_radius(12)
        .inner_margin(egui::Margin { left: 10, right: 12, top: 3, bottom: 3 })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("●").size(12.0).color(colour));
                ui.label(egui::RichText::new(text).size(14.0).strong().color(colour));
            });
        })
        .response
        .interact(egui::Sense::hover())
}

/// A small label over a wrapping row of pick chips ("Popular": Panadol,
/// Amoxil, ...). `items` are (name, can be picked); returns the one clicked.
/// One that can't be picked (out of stock) is grey.
pub fn chip_row(ui: &mut egui::Ui, label: &str, items: &[(&str, bool)]) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    ui.label(egui::RichText::new(label).size(14.0).color(TEXT_SECONDARY));
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for (i, &(name, enabled)) in items.iter().enumerate() {
            let chip = match enabled {
                true => chip(ui, name, false),
                false => pill(ui, name.to_string(), [GRIDLINE, GRIDLINE], TEXT_SECONDARY),
            };
            if chip.on_hover_text(name).clicked() && enabled {
                clicked = Some(i);
            }
        }
    });
    ui.add_space(4.0);
    clicked
}

/// A menu entry (the screens, the report list): plain text that is filled
/// blue when it is the one showing.
pub fn tab(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let (fills, ink) = match selected {
        true => ([ACCENT, ACCENT_STRONG], egui::Color32::WHITE),
        false => ([egui::Color32::TRANSPARENT, ACCENT_WASH], TEXT_PRIMARY),
    };
    let shape = Shape { radius: 8, padding: egui::vec2(14.0, 6.0), height: ui.spacing().interact_size.y };
    styled_button(ui, fills, fills, egui::RichText::new(text).color(ink), shape)
}

/// A row of buttons under a form: the main action first, "Cancel" last.
pub fn actions<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        add(ui)
    })
    .inner
}

/// A button's corner radius, padding and height.
struct Shape {
    radius: u8,
    padding: egui::Vec2,
    height: f32,
}

/// Draws a button with these colours. The style is changed on this same
/// `ui` and put back afterwards, not in a child scope: egui never moves a
/// child scope onto the next line, so pills in a wrapping row would not wrap.
fn styled_button(
    ui: &mut egui::Ui,
    fills: [egui::Color32; 2],
    borders: [egui::Color32; 2],
    text: egui::RichText,
    shape: Shape,
) -> egui::Response {
    let saved = ui.style().clone();
    let style = ui.style_mut();
    style.wrap_mode = Some(egui::TextWrapMode::Extend);
    style.spacing.button_padding = shape.padding;
    style.spacing.interact_size.y = shape.height;
    let w = &mut style.visuals.widgets;
    for (state, i) in [(&mut w.inactive, 0), (&mut w.hovered, 1), (&mut w.active, 1)] {
        state.weak_bg_fill = fills[i];
        state.bg_stroke = egui::Stroke::new(1.0, borders[i]);
        state.corner_radius = egui::CornerRadius::from(shape.radius);
        state.expansion = 0.0;
    }
    let response = ui.add(egui::Button::new(text));
    ui.set_style(saved);
    response
}

// ---------- Lists ----------

/// A striped, scrollable list with a header row. `rows` adds the cells,
/// calling `ui.end_row()` after each row.
pub fn list<R>(ui: &mut egui::Ui, id: &str, headers: &[&str], rows: impl FnOnce(&mut egui::Ui) -> R) -> R {
    panel(ui, |ui| table(ui, id, headers, rows))
}

/// Like `list`, without its own box (for a table inside a panel).
pub fn table<R>(ui: &mut egui::Ui, id: &str, headers: &[&str], rows: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        compact_list(ui);
        egui::ScrollArea::both()
            .id_salt(id)
            .show(ui, |ui| {
                egui::Grid::new(format!("{id}_grid"))
                    .spacing(egui::vec2(16.0, 6.0))
                    .with_row_color(|row, _| (row > 0 && row % 2 == 0).then_some(STRIPE_BG))
                    .show(ui, |ui| {
                        for header in headers {
                            ui.label(egui::RichText::new(*header).strong().color(TEXT_SECONDARY));
                        }
                        ui.end_row();
                        rows(ui)
                    })
                    .inner
            })
            .inner
    })
    .inner
}

/// Height of one `long_list` row.
pub const ROW_HEIGHT: f32 = 34.0;

/// A long list that scrolls smoothly: the header stays put, rows have one
/// height and only the rows on screen are drawn, so even thousands of
/// medicines scroll without stutter. `widths` are the column widths; the
/// column marked `0.0` takes the space left. `row(ui, i, widths)` draws row
/// `i` with `cell`.
pub fn long_list(
    ui: &mut egui::Ui,
    id: &str,
    headers: &[&str],
    widths: &[f32],
    rows: usize,
    mut row: impl FnMut(&mut egui::Ui, usize, &[f32]),
) {
    panel(ui, |ui| {
        let style = ui.style_mut();
        style.text_styles.insert(egui::TextStyle::Body, egui::FontId::new(16.0, egui::FontFamily::Proportional));
        style.text_styles.insert(egui::TextStyle::Button, egui::FontId::new(15.0, egui::FontFamily::Proportional));
        style.spacing.button_padding = egui::vec2(12.0, 3.0);
        style.spacing.interact_size.y = 28.0;
        let spacing = 8.0 * (widths.len() as f32 - 1.0);
        let fixed: f32 = widths.iter().sum();
        let flex = (ui.available_width() - fixed - spacing).max(120.0);
        let widths: Vec<f32> = widths.iter().map(|&w| if w == 0.0 { flex } else { w }).collect();
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
        ui.horizontal(|ui| {
            for (header, &w) in headers.iter().zip(&widths) {
                // A header starting with '>' is right-aligned, over a column of numbers.
                let (right, header) = header.strip_prefix('>').map_or((false, *header), |h| (true, h));
                let text = egui::RichText::new(header).strong().size(14.0).color(TEXT_SECONDARY);
                cell(ui, w, 24.0, |ui| match right {
                    true => number_cell(ui, text),
                    false => cell_text(ui, text),
                });
            }
        });
        ui.separator();
        // Use the height left on screen, so this list scrolls rather than the page.
        let height = (ui.clip_rect().bottom() - ui.cursor().top() - 24.0).max(240.0);
        egui::ScrollArea::vertical().id_salt(id).max_height(height).auto_shrink([false, true]).show_rows(
            ui,
            ROW_HEIGHT,
            rows,
            |ui, range| {
                for i in range {
                    let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), ROW_HEIGHT));
                    if i % 2 == 1 {
                        ui.painter().rect_filled(rect, 4.0, STRIPE_BG);
                    }
                    ui.horizontal(|ui| {
                        ui.set_height(ROW_HEIGHT);
                        row(ui, i, &widths)
                    });
                }
            },
        );
    });
}

/// One cell of a `long_list` row (or header), `width` wide, vertically centred.
pub fn cell(ui: &mut egui::Ui, width: f32, height: f32, add: impl FnOnce(&mut egui::Ui)) {
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    ui.allocate_ui_with_layout(egui::vec2(width, height), layout, |ui| {
        ui.set_width(width);
        add(ui);
    });
}

/// Text that is cut with "…" if it doesn't fit its cell.
pub fn cell_text(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) {
    ui.add(egui::Label::new(text).truncate());
}

/// How much stock is left: red below 10, orange below 50, green from 50.
pub fn stock_status(units: i64) -> (egui::Color32, &'static str) {
    match units {
        ..10 => (STATUS_CRITICAL, "Low — reorder"),
        10..50 => (STATUS_SERIOUS, "Running low"),
        _ => (STATUS_GOOD, "Good"),
    }
}

/// The coloured stock dot on its own.
pub fn stock_dot(ui: &mut egui::Ui, units: i64) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 5.5, stock_status(units).0);
}

/// The coloured stock dot and its word ("● Running low").
pub fn stock_level(ui: &mut egui::Ui, units: i64) {
    stock_dot(ui, units);
    let (colour, word) = stock_status(units);
    ui.label(egui::RichText::new(word).color(colour).strong());
}

/// A number, right-aligned so columns of figures line up.
pub fn number_cell(ui: &mut egui::Ui, text: impl Into<egui::RichText>) {
    let text = text.into();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(text);
    });
}

/// Any report as a smooth long list. Number columns are right-aligned, the
/// widest text column takes the spare width, and `action` (when
/// `action_width` > 0) adds buttons at the end of each row.
pub fn report_table(
    ui: &mut egui::Ui,
    id: &str,
    table: &Table,
    action_width: f32,
    mut action: impl FnMut(&mut egui::Ui, usize),
) {
    if table.rows.is_empty() {
        return panel(ui, |ui| muted(ui, "Nothing to show for these dates."));
    }
    let cols = table.headers.len();
    let is_number = |text: &str| text.is_empty() || text.replace(',', "").parse::<f64>().is_ok();
    let numeric: Vec<bool> = (0..cols).map(|c| table.rows.iter().all(|r| is_number(&r[c]))).collect();
    // About 8 px a character; the first 300 rows are enough to size a column.
    let mut widths: Vec<f32> = (0..cols)
        .map(|c| {
            let longest = table.rows.iter().take(300).map(|r| r[c].chars().count()).max().unwrap_or(0);
            (longest.max(table.headers[c].chars().count()) as f32 * 8.0 + 20.0).clamp(64.0, 280.0)
        })
        .collect();
    let flex = (0..cols).filter(|&c| !numeric[c]).max_by(|&a, &b| widths[a].total_cmp(&widths[b]));
    if let Some(c) = flex {
        widths[c] = 0.0;
    }
    let mut headers: Vec<String> =
        table.headers.iter().zip(&numeric).map(|(h, &n)| if n { format!(">{h}") } else { h.clone() }).collect();
    if action_width > 0.0 {
        headers.push(String::new());
        widths.push(action_width);
    }
    // Narrow window: shrink the fixed columns so the row still fits.
    let room = ui.available_width() - 40.0 - 8.0 * widths.len() as f32 - 160.0;
    let fixed: f32 = widths.iter().sum();
    if fixed > room {
        widths.iter_mut().for_each(|w| *w = if *w == 0.0 { 0.0 } else { (*w * room / fixed).max(48.0) });
    }
    let headers: Vec<&str> = headers.iter().map(String::as_str).collect();
    long_list(ui, id, &headers, &widths, table.rows.len(), |ui, i, w| {
        for (c, text) in table.rows[i].iter().enumerate() {
            cell(ui, w[c], ROW_HEIGHT, |ui| match numeric[c] {
                true => number_cell(ui, text.as_str()),
                false => cell_text(ui, text.as_str()),
            });
        }
        if action_width > 0.0 {
            cell(ui, w[cols], ROW_HEIGHT, |ui| action(ui, i));
        }
    });
}

/// What was clicked in a row's "Edit / Delete" buttons.
#[derive(PartialEq)]
pub enum RowClick {
    None,
    Edit,
    Delete,
}

/// "Edit" and "Delete" with a "Delete? Yes / No" step, so one stray click
/// can't delete anything. `confirming` holds the row waiting for "Yes".
pub fn edit_delete(ui: &mut egui::Ui, row_id: &str, confirming: &mut Option<String>) -> RowClick {
    let mut click = RowClick::None;
    ui.horizontal(|ui| {
        if confirming.as_deref() == Some(row_id) {
            ui.colored_label(STATUS_CRITICAL, "Delete?");
            if danger_button(ui, "Yes").clicked() {
                click = RowClick::Delete;
                *confirming = None;
            }
            if quiet_button(ui, "No").clicked() {
                *confirming = None;
            }
        } else {
            if ui.button("Edit").clicked() {
                click = RowClick::Edit;
                *confirming = None;
            }
            if quiet_button(ui, "Delete").clicked() {
                *confirming = Some(row_id.to_string());
            }
        }
    });
    click
}

/// A row of equal-width number cards that fills the width: a label over
/// a big figure, with a coloured edge.
pub fn stat_row(ui: &mut egui::Ui, tiles: &[(&str, String, egui::Color32)]) {
    let gap = 12.0;
    let width = ((ui.available_width() - gap * (tiles.len() as f32 - 1.0)) / tiles.len() as f32).max(140.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (label, value, colour) in tiles {
            let card = egui::Frame::NONE
                .fill(PANEL_BG)
                .corner_radius(egui::CornerRadius::from(10))
                .stroke(egui::Stroke::new(1.0, GRIDLINE))
                .inner_margin(egui::Margin { left: 20, right: 16, top: 12, bottom: 12 })
                .show(ui, |ui| {
                    ui.set_width(width - 38.0);
                    ui.label(egui::RichText::new(*label).size(14.0).color(TEXT_SECONDARY));
                    ui.label(egui::RichText::new(value).size(28.0).strong().color(*colour));
                });
            let edge = egui::Rect::from_min_size(card.response.rect.min, egui::vec2(5.0, card.response.rect.height()));
            let round = egui::CornerRadius { nw: 10, sw: 10, ne: 0, se: 0 };
            ui.painter().rect_filled(edge, round, *colour);
        }
    });
}
