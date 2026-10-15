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
pub const STATUS_SERIOUS: egui::Color32 = egui::Color32::from_rgb(0xec, 0x83, 0x5a);
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
    let main = look != Look::Quiet;
    ui.scope(|ui| {
        if main {
            ui.spacing_mut().button_padding.x += 6.0;
        }
        styled_button(ui, fills, borders, if main { text.strong() } else { text }, 8)
    })
    .inner
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

/// A rounded pill for choosing one of several (screens, report kinds, the
/// payment type) or for one-tap picks; filled blue when selected.
pub fn chip(ui: &mut egui::Ui, text: impl Into<String>, selected: bool) -> egui::Response {
    let (fills, ink) = match selected {
        true => ([ACCENT, ACCENT_STRONG], egui::Color32::WHITE),
        false => ([ACCENT_WASH, ACCENT_PRESSED], ACCENT_STRONG),
    };
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = egui::vec2(14.0, 4.0);
        ui.spacing_mut().interact_size.y = 30.0;
        styled_button(ui, fills, fills, egui::RichText::new(text).color(ink), 15)
    })
    .inner
}

/// A menu entry (the screens, the report list): plain text that is filled
/// blue when it is the one showing.
pub fn tab(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let (fills, ink) = match selected {
        true => ([ACCENT, ACCENT_STRONG], egui::Color32::WHITE),
        false => ([egui::Color32::TRANSPARENT, ACCENT_WASH], TEXT_PRIMARY),
    };
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = egui::vec2(14.0, 6.0);
        styled_button(ui, fills, fills, egui::RichText::new(text).color(ink), 8)
    })
    .inner
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

/// Draws a button with these colours; the changes stay inside the caller's scope.
fn styled_button(
    ui: &mut egui::Ui,
    fills: [egui::Color32; 2],
    borders: [egui::Color32; 2],
    text: egui::RichText,
    radius: u8,
) -> egui::Response {
    let w = &mut ui.visuals_mut().widgets;
    for (state, i) in [(&mut w.inactive, 0), (&mut w.hovered, 1), (&mut w.active, 1)] {
        state.weak_bg_fill = fills[i];
        state.bg_stroke = egui::Stroke::new(1.0, borders[i]);
        state.corner_radius = egui::CornerRadius::from(radius);
        state.expansion = 0.0;
    }
    ui.add(egui::Button::new(text))
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
                cell(ui, w, 24.0, |ui| {
                    ui.label(egui::RichText::new(*header).strong().color(TEXT_SECONDARY));
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

/// A stock alert by units left: under 10 red, under 25 orange, under 50 green.
pub fn stock_alert(units: i64) -> Option<(egui::Color32, &'static str)> {
    match units {
        ..10 => Some((STATUS_CRITICAL, "● Below 10 — reorder now")),
        10..25 => Some((STATUS_SERIOUS, "● Below 25")),
        25..50 => Some((STATUS_GOOD, "● Below 50")),
        _ => None,
    }
}

/// A number, right-aligned so columns of figures line up.
pub fn number_cell(ui: &mut egui::Ui, text: impl Into<String>) {
    let text = text.into();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(text);
    });
}

/// Any report table. `action` adds one widget per row in a last column
/// (e.g. a "Void" button) — add an empty label when there's nothing to add.
pub fn report_table(ui: &mut egui::Ui, id: &str, table: &Table, mut action: impl FnMut(&mut egui::Ui, usize)) {
    const SHOWN: usize = 500;
    let mut headers: Vec<&str> = table.headers.iter().map(String::as_str).collect();
    headers.push("");
    list(ui, id, &headers, |ui| {
        for (i, row) in table.rows.iter().take(SHOWN).enumerate() {
            for cell in row {
                if cell.replace(',', "").parse::<f64>().is_ok() {
                    number_cell(ui, cell.as_str());
                } else {
                    ui.label(cell);
                }
            }
            action(ui, i);
            ui.end_row();
        }
    });
    if table.rows.is_empty() {
        muted(ui, "Nothing to show for these dates.");
    } else if table.rows.len() > SHOWN {
        muted(ui, format!("Showing {SHOWN} of {} rows — export to CSV to see them all.", table.rows.len()));
    }
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

/// A big number over a label.
pub fn stat_tile(ui: &mut egui::Ui, label: &str, value: &str, colour: egui::Color32) {
    egui::Frame::NONE
        .fill(PANEL_BG)
        .corner_radius(egui::CornerRadius::from(10))
        .stroke(egui::Stroke::new(1.0, GRIDLINE))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_min_width(200.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(value).size(30.0).strong().color(colour));
                ui.label(egui::RichText::new(label).size(15.0).color(TEXT_SECONDARY));
            });
        });
}
