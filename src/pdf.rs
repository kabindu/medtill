
use crate::models::{Cents, Table};

/// What goes at the top of every page.
pub struct Header<'a> {
    pub shop: &'a str,
    pub title: &'a str,
    pub period: &'a str,
    pub made_by: &'a str,
    pub made_at: &'a str,
    pub code: &'a str,
}

// A4 landscape, in points (1/72 inch); y runs from the bottom up.
const W: f32 = 842.0;
const H: f32 = 595.0;
const MARGIN: f32 = 36.0;
const TABLE_TOP: f32 = H - 142.0;
const TABLE_BOTTOM: f32 = 60.0;

type Rgb = [f32; 3];
const ACCENT: Rgb = [0.165, 0.471, 0.839];
const ACCENT_STRONG: Rgb = [0.094, 0.310, 0.584];
const ON_ACCENT: Rgb = [0.84, 0.90, 1.0];
const WASH: Rgb = [0.906, 0.937, 0.984];
const STRIPE: Rgb = [0.965, 0.973, 0.988];
const GRID: Rgb = [0.86, 0.86, 0.84];
const INK: Rgb = [0.043, 0.043, 0.043];
const MUTED: Rgb = [0.40, 0.40, 0.38];
const WHITE: Rgb = [1.0, 1.0, 1.0];

/// Widths (1/1000 of the font size) of the PDF's built-in Helvetica and
/// Helvetica-Bold, for the characters space (32) to '~' (126).
#[rustfmt::skip]
const REGULAR: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556,
    556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667,
    556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556,
    556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722,
    500, 500, 500, 334, 260, 334, 584,
];
#[rustfmt::skip]
const BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556,
    556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722,
    611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, 333, 556,
    611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778,
    556, 556, 500, 389, 280, 389, 584,
];

/// A font: size in points, bold or not, colour.
#[derive(Clone, Copy)]
struct Font(f32, bool, Rgb);

/// Text in the fonts' WinAnsi encoding; dashes and dots are kept, other
/// symbols become '?'.
fn encode(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            ' '..='~' => c as u8,
            '–' => 0x96,
            '—' => 0x97,
            '·' => 0xB7,
            '•' => 0x95,
            _ => b'?',
        })
        .collect()
}

fn width(text: &[u8], Font(size, bold, _): Font) -> f32 {
    let glyphs = if bold { &BOLD } else { &REGULAR };
    let units: u32 = text
        .iter()
        .map(|&b| match b {
            32..=126 => u32::from(glyphs[usize::from(b - 32)]),
            0x97 => 1000,
            0xB7 => 278,
            0x95 => 350,
            _ => 556,
        })
        .sum();
    units as f32 * size / 1000.0
}

/// Cuts `text` with "..." so it fits in `max` points.
fn fit(mut text: Vec<u8>, max: f32, font: Font) -> Vec<u8> {
    if width(&text, font) <= max {
        return text;
    }
    while !text.is_empty() && width(&text, font) + width(b"...", font) > max {
        text.pop();
    }
    text.extend_from_slice(b"...");
    text
}

/// The drawing commands of one page.
#[derive(Default)]
struct Page(Vec<u8>);

impl Page {
    fn op(&mut self, command: &str) {
        self.0.extend_from_slice(command.as_bytes());
        self.0.push(b'\n');
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, [r, g, b]: Rgb) {
        self.op(&format!("{r} {g} {b} rg {x:.2} {y:.2} {w:.2} {h:.2} re f"));
    }

    fn line(&mut self, y: f32, thickness: f32, [r, g, b]: Rgb) {
        self.op(&format!("{r} {g} {b} RG {thickness} w {MARGIN} {y:.2} m {:.2} {y:.2} l S", W - MARGIN));
    }

    /// `(…) Tj` with the PDF string escapes.
    fn show(&mut self, text: &[u8]) {
        self.0.push(b'(');
        for &b in text {
            if matches!(b, b'(' | b')' | b'\\') {
                self.0.push(b'\\');
            }
            self.0.push(b);
        }
        self.op(") Tj ET");
    }

    /// Text starting at x, or ending at x when `right` (for numbers).
    fn text(&mut self, x: f32, y: f32, text: &[u8], font: Font, right: bool) {
        let Font(size, bold, [r, g, b]) = font;
        let x = if right { x - width(text, font) } else { x };
        self.op(&format!("{r} {g} {b} rg BT /{} {size:.2} Tf {x:.2} {y:.2} Td", if bold { "F2" } else { "F1" }));
        self.show(text);
    }

    /// See-through text turned by `angle` degrees (the watermark).
    fn faint(&mut self, x: f32, y: f32, text: &[u8], font: Font, angle: f32) {
        let Font(size, _, [r, g, b]) = font;
        let (sin, cos) = angle.to_radians().sin_cos();
        self.op(&format!(
            "q /GS1 gs {r} {g} {b} rg BT /F2 {size} Tf {cos:.4} {sin:.4} {:.4} {cos:.4} {x:.2} {y:.2} Tm",
            -sin
        ));
        self.show(text);
        self.op("Q");
    }
}

/// The report as a PDF file.
pub fn report(h: &Header, table: &Table) -> Vec<u8> {
    let cols = table.headers.len();
    let is_number = |s: &str| s.is_empty() || s.replace(',', "").parse::<f64>().is_ok();
    let numeric: Vec<bool> = (0..cols).map(|c| table.rows.iter().all(|r| is_number(&r[c]))).collect();

    // Totals of the money columns ("... (KSh)"), leaving out voided receipts.
    let counted: Vec<&Vec<String>> = table.rows.iter().filter(|r| r.last().is_none_or(|s| s != "void")).collect();
    let totals: Vec<Option<Cents>> = (0..cols)
        .map(|c| {
            let money = numeric[c] && table.headers[c].contains("KSh");
            let cents = |s: &str| (s.replace(',', "").parse::<f64>().unwrap_or(0.0) * 100.0).round() as i64;
            money.then(|| Cents(counted.iter().map(|r| cents(&r[c])).sum()))
        })
        .collect();
    let has_totals = !table.rows.is_empty() && totals.iter().any(Option::is_some);

    // Column widths from the widest text (headings and totals in bold),
    // then stretched or shrunk to the page width; a very wide table also
    // gets a smaller font.
    let room = W - 2.0 * MARGIN;
    let mut size = 8.5;
    let natural: Vec<f32> = (0..cols)
        .map(|c| {
            let bold = |s: &str| width(&encode(s), Font(size, true, INK));
            let total = totals[c].map_or(0.0, |sum| bold(&sum.to_string()));
            let body = table.rows.iter().take(500).map(|r| width(&encode(&r[c]), Font(size, false, INK)));
            (body.fold(bold(&table.headers[c]).max(total), f32::max) + 16.0).min(230.0)
        })
        .collect();
    let total: f32 = natural.iter().sum::<f32>().max(1.0);
    if total > room {
        size = (size * room / total).max(6.5);
    }
    let widths: Vec<f32> = natural.iter().map(|w| w * room / total).collect();
    let row_h = size * 1.9;
    let per_page = (((TABLE_TOP - 20.0 - TABLE_BOTTOM - row_h) / row_h) as usize).max(1);
    let chunks: Vec<&[Vec<String>]> = match table.rows.is_empty() {
        true => vec![&[]],
        false => table.rows.chunks(per_page).collect(),
    };

    let mut pages = Vec::new();
    for (number, rows) in chunks.iter().enumerate() {
        let mut p = Page::default();
        frame(&mut p, h, number + 1, chunks.len(), table.rows.len());
        // The column headings.
        p.rect(MARGIN, TABLE_TOP - 20.0, room, 20.0, WASH);
        let mut x = MARGIN;
        let head = Font(size, true, ACCENT_STRONG);
        for c in 0..cols {
            let text = fit(encode(&table.headers[c]), widths[c] - 12.0, head);
            let left = if numeric[c] { x + widths[c] - 6.0 } else { x + 6.0 };
            p.text(left, TABLE_TOP - 13.5, &text, head, numeric[c]);
            x += widths[c];
        }
        // The rows, striped, with a hairline under each.
        let mut y = TABLE_TOP - 20.0;
        for (i, row) in rows.iter().enumerate() {
            y -= row_h;
            if i % 2 == 1 {
                p.rect(MARGIN, y, room, row_h, STRIPE);
            }
            let void = row.last().is_some_and(|s| s == "void");
            let body = Font(size, false, if void { MUTED } else { INK });
            draw_row(&mut p, row.iter().map(|s| encode(s)), &widths, &numeric, y + row_h * 0.32, body);
            p.line(y, 0.4, GRID);
        }
        if number + 1 == chunks.len() && has_totals {
            y -= row_h + 4.0;
            p.rect(MARGIN, y, room, row_h + 2.0, WASH);
            p.line(y + row_h + 2.0, 1.2, ACCENT_STRONG);
            let label = if counted.len() < table.rows.len() { "Total (voids left out)" } else { "Total" };
            let cells = (0..cols).map(|c| match (&totals[c], c) {
                (Some(sum), _) => encode(&sum.to_string()),
                (None, 0) => encode(label),
                (None, _) => Vec::new(),
            });
            draw_row(&mut p, cells, &widths, &numeric, y + row_h * 0.32, Font(size, true, INK));
        }
        if table.rows.is_empty() {
            p.text(
                MARGIN + 6.0,
                TABLE_TOP - 40.0,
                b"Nothing to show for these dates.",
                Font(10.0, false, MUTED),
                false,
            );
        }
        // Small diagonal "ORIGINAL" stamps over the whole page.
        let stamp = encode(&format!("ORIGINAL  ·  {}  ·  {}", h.shop.to_uppercase(), h.code));
        for row in 0..7 {
            for col in 0..5 {
                let (x, y) = (col as f32 * 200.0 - 40.0 + (row % 2) as f32 * 100.0, row as f32 * 90.0 + 10.0);
                p.faint(x, y, &stamp, Font(8.0, true, MUTED), 25.0);
            }
        }
        pages.push(p.0);
    }
    assemble(h, pages)
}

/// One table row: text left-aligned, numbers right-aligned, cut to fit.
fn draw_row(p: &mut Page, cells: impl Iterator<Item = Vec<u8>>, widths: &[f32], numeric: &[bool], y: f32, font: Font) {
    let mut x = MARGIN;
    for ((text, &w), &number) in cells.zip(widths).zip(numeric) {
        let text = fit(text, w - 12.0, font);
        p.text(if number { x + w - 6.0 } else { x + 6.0 }, y, &text, font, number);
        x += w;
    }
}

/// The parts every page shares: the big watermark, the header band, the
/// details line and the footer.
fn frame(p: &mut Page, h: &Header, number: usize, pages: usize, rows: usize) {
    // The shop's name across the middle, sized to stay on the page.
    let shop = encode(&h.shop.to_uppercase());
    let natural = width(&shop, Font(1.0, true, ACCENT)).max(1.0);
    let big = Font((620.0 / natural).min(72.0), true, ACCENT);
    let half = width(&shop, big) / 2.0;
    let (sin, cos) = 28f32.to_radians().sin_cos();
    p.faint(W / 2.0 - half * cos, H / 2.0 - 60.0 - half * sin, &shop, big, 28.0);

    p.rect(0.0, H - 86.0, W, 86.0, ACCENT);
    p.rect(0.0, H - 90.0, W, 4.0, ACCENT_STRONG);
    p.text(MARGIN, H - 46.0, &fit(encode(h.shop), 420.0, Font(22.0, true, WHITE)), Font(22.0, true, WHITE), false);
    p.text(MARGIN, H - 66.0, b"PHARMACY REPORT  \xB7  MEDTILL", Font(8.5, true, ON_ACCENT), false);
    p.text(W - MARGIN, H - 44.0, &fit(encode(h.title), 330.0, Font(16.0, true, WHITE)), Font(16.0, true, WHITE), true);
    p.text(W - MARGIN, H - 64.0, &encode(h.period), Font(10.5, false, ON_ACCENT), true);

    let details = [
        ("GENERATED", h.made_at.to_string()),
        ("BY", h.made_by.to_string()),
        ("ROWS", rows.to_string()),
        ("VERIFICATION CODE", h.code.to_string()),
    ];
    for (i, (label, value)) in details.iter().enumerate() {
        let x = MARGIN + i as f32 * 192.0;
        p.text(x, H - 110.0, label.as_bytes(), Font(7.0, true, MUTED), false);
        p.text(x, H - 124.0, &fit(encode(value), 180.0, Font(10.0, true, INK)), Font(10.0, true, INK), false);
    }

    p.line(44.0, 0.5, GRID);
    let note = format!(
        "{}  ·  Confidential  ·  Made with MedTill. To check this report, find code {} in Reports > Audit log.",
        h.shop, h.code
    );
    p.text(MARGIN, 32.0, &fit(encode(&note), 640.0, Font(7.5, false, MUTED)), Font(7.5, false, MUTED), false);
    p.text(W - MARGIN, 32.0, &encode(&format!("Page {number} of {pages}")), Font(8.0, true, INK), true);
}

/// Wraps the pages into a PDF file: numbered objects, then the table of
/// their byte offsets that PDF readers use to find them.
fn assemble(h: &Header, pages: Vec<Vec<u8>>) -> Vec<u8> {
    // Document properties use another encoding, so keep them plain ASCII.
    let literal = |s: &str| {
        let mut out = vec![b'('];
        for b in encode(s).into_iter().map(|b| if b > 126 { b'-' } else { b }) {
            if matches!(b, b'(' | b')' | b'\\') {
                out.push(b'\\');
            }
            out.push(b);
        }
        out.push(b')');
        out
    };
    let font = |name: &str| format!("<< /Type /Font /Subtype /Type1 /BaseFont /{name} /Encoding /WinAnsiEncoding >>");
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", 7 + 2 * i)).collect();
    let created = chrono::Local::now().format("D:%Y%m%d%H%M%S");
    let mut info = b"<< /Title ".to_vec();
    info.extend(literal(&format!("{} - {}", h.title, h.period)));
    info.extend(b" /Author ");
    info.extend(literal(h.shop));
    info.extend(b" /Subject ");
    info.extend(literal(&format!("Verification code {}", h.code)));
    info.extend(format!(" /Creator (MedTill) /Producer (MedTill) /CreationDate ({created}) >>").into_bytes());

    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Count {} /Kids [{}] >>", pages.len(), kids.join(" ")).into_bytes(),
        font("Helvetica").into_bytes(),
        font("Helvetica-Bold").into_bytes(),
        b"<< /Type /ExtGState /ca 0.07 /CA 0.07 >>".to_vec(),
        info,
    ];
    for (i, content) in pages.into_iter().enumerate() {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {W} {H}] /Contents {} 0 R \
                 /Resources << /Font << /F1 3 0 R /F2 4 0 R >> /ExtGState << /GS1 5 0 R >> >> >>",
                8 + 2 * i
            )
            .into_bytes(),
        );
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend(content);
        stream.extend(b"\nendstream");
        objects.push(stream);
    }
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).into_bytes());
        out.extend(body);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).into_bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").into_bytes());
    }
    let trailer =
        format!("trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1);
    out.extend(trailer.into_bytes());
    out
}
