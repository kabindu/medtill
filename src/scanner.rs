//! Reading a medicine box with the camera, fully offline, in three layers:
//!
//! 1. **Reading the letters.** Up to 3 photos go through PaddleOCR's
//!    PP-OCRv6 text reader (the "small" model if its files are there, else
//!    "tiny"), run on this computer by the `ocr-rs` crate.
//! 2. **Understanding the box.** Every line read gets scored on features
//!    (is it a known medicine? big print? a maker's name? an instruction?),
//!    and the best line becomes the name, with its strength and form. The
//!    category comes from the built-in medicine list (`medicines`), in the
//!    shop's own wording when it already has that category.
//! 3. **Learning** (`Taught`, `train`). Each time a scanned medicine is
//!    saved, the box's words are remembered for that medicine (the same box
//!    is recognised next time); when the reader picked the wrong line, that
//!    line's words are remembered as "not a name" (slogans like "Strong
//!    Formula") and the feature weights move a little towards the right
//!    line, never far from where they started, so one odd box can't spoil
//!    the rest.
//!
//! No UI and no database here. The camera and the reader each run on their
//! own thread and stop (freeing their memory) when the scan panel closes,
//! so nothing is loaded while nobody is scanning.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{ApiBackend, CameraIndex, RequestedFormat, RequestedFormatType};

use crate::medicines::{self, FORMS, INSTRUCTIONS, MAKERS, SLOGANS};
use crate::models::Product;
use crate::validate;

/// One picture as RGB bytes (3 per pixel, row by row).
#[derive(Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

// ---------- The camera ----------

/// If the scan panel hasn't been drawn for this long (the user went to
/// another screen), the camera turns itself off.
const IDLE_SHUTDOWN: Duration = Duration::from_millis(1500);

/// What the camera thread and the screen share.
struct Shared {
    latest: Mutex<Option<Frame>>,
    error: Mutex<Option<String>>,
    last_seen: Mutex<Instant>,
    stop: AtomicBool,
    running: AtomicBool,
}

/// The first camera Windows can see, running on its own thread. Dropping
/// it turns the camera off.
pub struct Camera {
    pub name: String,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Camera {
    pub fn start() -> Result<Self, String> {
        let cameras = nokhwa::query(ApiBackend::Auto).map_err(|e| format!("Could not look for cameras: {e}"))?;
        let Some(info) = cameras.first() else {
            return Err("No camera found. Plug in a USB camera and try again.".into());
        };
        let shared = Arc::new(Shared {
            latest: Mutex::new(None),
            error: Mutex::new(None),
            last_seen: Mutex::new(Instant::now()),
            stop: AtomicBool::new(false),
            running: AtomicBool::new(true),
        });
        let (index, inner) = (info.index().clone(), Arc::clone(&shared));
        // The nokhwa camera is made inside its thread, so it never has to move between threads.
        let thread = std::thread::spawn(move || {
            camera_loop(index, &inner);
            inner.running.store(false, Ordering::Relaxed);
        });
        Ok(Self { name: info.human_name(), shared, thread: Some(thread) })
    }

    /// The newest picture since the last call, if any.
    pub fn take_frame(&self) -> Option<Frame> {
        self.shared.latest.lock().ok()?.take()
    }

    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().ok()?.clone()
    }

    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::Relaxed)
    }

    /// Call each time the panel is drawn; without it the camera turns off.
    pub fn heartbeat(&self) {
        if let Ok(mut seen) = self.shared.last_seen.lock() {
            *seen = Instant::now();
        }
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn camera_loop(index: CameraIndex, shared: &Shared) {
    let report = |message: String| {
        if let Ok(mut slot) = shared.error.lock() {
            *slot = Some(message);
        }
    };
    let format = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution);
    let mut camera = match nokhwa::Camera::new(index, format) {
        Ok(camera) => camera,
        Err(e) => return report(format!("Could not open the camera: {e}")),
    };
    if let Err(e) = camera.open_stream() {
        return report(format!("Could not start the camera: {e}"));
    }
    while !shared.stop.load(Ordering::Relaxed) {
        if shared.last_seen.lock().map_or(true, |seen| seen.elapsed() > IDLE_SHUTDOWN) {
            break;
        }
        match camera.frame().and_then(|buffer| buffer.decode_image::<RgbFormat>()) {
            Ok(image) => {
                let (width, height) = image.dimensions();
                if let Ok(mut slot) = shared.latest.lock() {
                    *slot = Some(Frame { width, height, rgb: image.into_raw() });
                }
            }
            Err(e) => {
                report(format!("The camera stopped: {e}"));
                break;
            }
        }
    }
    let _ = camera.stop_stream();
}

// ---------- The text reader ----------

/// The reader's model files, in a `models` folder next to MedTill.exe, in
/// the folder MedTill was started from, or in its data folder. The "small"
/// set reads print noticeably better; the "tiny" set (about 3 MB) is used
/// when small isn't there.
pub const MODEL_SETS: [[&str; 3]; 2] = [
    ["PP-OCRv6_small_det.mnn", "PP-OCRv6_small_rec.mnn", "ppocr_keys_v6_small.txt"],
    ["PP-OCRv6_tiny_det.mnn", "PP-OCRv6_tiny_rec.mnn", "ppocr_keys_v6_tiny.txt"],
];

/// Photos are shrunk to this many pixels on their long side before reading.
const READ_SIDE: u32 = 1600;

/// The size the text finder looks at: small print (batch, expiry) gets lost
/// at the reader's default of 960.
const FIND_SIDE: u32 = 1280;

/// The best model set found in a `models` folder.
fn model_files() -> Option<[PathBuf; 3]> {
    let exe = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join("models")));
    let started = Some(crate::storage::start_dir().join("models"));
    let dirs: Vec<PathBuf> = [exe, started, Some(PathBuf::from("models"))].into_iter().flatten().collect();
    MODEL_SETS.iter().find_map(|set| {
        let dir = dirs.iter().find(|d| set.iter().all(|f| d.join(f).is_file()))?;
        Some(set.map(|f| dir.join(f)))
    })
}

/// One line of text found on a photo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub text: String,
    /// Height of the print in pixels: big print is usually the name.
    pub height: f32,
    /// Distance from the top of its photo, in pixels.
    pub top: f32,
    /// How sure the reader is (0–1). Logo lettering often reads low.
    pub confidence: f32,
    /// Which photo it is on (0, 1, 2).
    pub photo: usize,
}

type Reading = Result<(Vec<Line>, Duration), String>;

/// The text reader on its own thread. It starts loading the model at once,
/// so it is ready by the time the photos are taken; dropping it ends the
/// thread and frees the model's memory.
pub struct Reader {
    jobs: Sender<Vec<Frame>>,
    done: Receiver<Reading>,
}

impl Reader {
    pub fn start() -> Self {
        let (jobs, inbox) = mpsc::channel::<Vec<Frame>>();
        let (outbox, done) = mpsc::channel();
        std::thread::spawn(move || {
            let engine = match load_engine() {
                Ok(engine) => engine,
                Err(e) => return drop(outbox.send(Err(e))),
            };
            // Ends when the Reader (holding `jobs`) is dropped.
            for photos in inbox {
                let started = Instant::now();
                let lines: Result<Vec<Vec<Line>>, String> =
                    photos.iter().enumerate().map(|(i, p)| read_photo(&engine, p, i)).collect();
                let _ = outbox.send(lines.map(|lines| (lines.concat(), started.elapsed())));
            }
        });
        Self { jobs, done }
    }

    pub fn read(&self, photos: Vec<Frame>) {
        let _ = self.jobs.send(photos);
    }

    /// The text of the photos once read (or why it couldn't be).
    pub fn poll(&self) -> Option<Reading> {
        self.done.try_recv().ok()
    }
}

fn load_engine() -> Result<ocr_rs::OcrEngine, String> {
    let Some([det, rec, keys]) = model_files() else {
        return Err(format!(
            "The text reader's files are missing: put {} in a \"models\" folder next to MedTill.",
            MODEL_SETS[1].join(", ")
        ));
    };
    // A few threads: quick, without making the rest of the computer wait.
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().min(4)) as i32;
    // Keep unsure readings: brand names in logo lettering often score 0.3–0.5,
    // and the scoring below weighs how sure the reader was.
    let config = ocr_rs::OcrEngineConfig::new()
        .with_threads(threads)
        .with_min_result_confidence(0.3)
        .with_det_options(ocr_rs::DetOptions::new().with_max_side_len(FIND_SIDE));
    ocr_rs::OcrEngine::new(det, rec, keys, Some(config)).map_err(|e| format!("Could not start the text reader: {e}"))
}

fn read_photo(engine: &ocr_rs::OcrEngine, photo: &Frame, index: usize) -> Result<Vec<Line>, String> {
    let rgb =
        image::RgbImage::from_raw(photo.width, photo.height, photo.rgb.clone()).ok_or("A photo was incomplete.")?;
    let mut picture = image::DynamicImage::ImageRgb8(rgb);
    if photo.width.max(photo.height) > READ_SIDE {
        picture = picture.resize(READ_SIDE, READ_SIDE, image::imageops::FilterType::Triangle);
    }
    // Text printed sideways along the box's edge (often the batch and expiry) is turned and read too.
    let options = ocr_rs::RecognizeOptions::new().with_rotated_text_mode(ocr_rs::RotatedTextMode::DetectedOnly);
    let found =
        engine.recognize_with_options(&picture, &options).map_err(|e| format!("Could not read the photo: {e}"))?;
    Ok(found
        .into_iter()
        .map(|r| Line {
            text: r.text,
            // The short side of the box is the print's height, also for sideways text.
            height: r.bbox.rect.height().min(r.bbox.rect.width()) as f32,
            top: r.bbox.rect.top() as f32,
            confidence: r.confidence,
            photo: index,
        })
        .collect())
}

// ---------- Understanding the box ----------

/// What was read from a box, ready to fill a form. The user always checks it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    /// A medicine already listed that the box matches.
    pub product_id: Option<String>,
    pub name: String,
    pub category: String,
    pub batch: String,
    pub expiry: Option<NaiveDate>,
    /// The box's distinctive words, remembered for this medicine once saved.
    pub words: Vec<String>,
    /// Other lines that could be the name, best first (offered as one-tap fixes).
    pub alternatives: Vec<String>,
    /// Every line that could be the name, best first, with what the reader
    /// saw in it; `train` learns from these.
    pub candidates: Vec<Candidate>,
    /// The lines as read, kept as a training example.
    pub lines: Vec<Line>,
}

/// A line that could be the name.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub name: String,
    pub features: [f32; FEATURES],
    pub score: f32,
}

/// What the reader looks at to decide which line is the name. Each feature
/// has a weight: they start at `START_WEIGHTS` and move a little every time
/// someone saves a scan under a name the reader didn't pick (`train`).
pub const FEATURES: usize = 11;

/// In order: a known medicine name, one of this shop's medicines, words
/// learned from earlier scans, print size, how near the top, a strength on
/// the line, maker words, instruction, slogan or taught not-a-name words,
/// an unsure reading, mostly letters, a short line (names are 1–3 words;
/// ingredient lists are longer).
pub const START_WEIGHTS: [f32; FEATURES] = [3.0, 3.0, 1.5, 2.5, 0.6, 0.5, -3.0, -3.0, -1.0, 1.0, 1.0];

/// How far one correction moves the weights, and how far from
/// `START_WEIGHTS` they may ever go.
const RATE: f32 = 0.25;
const MAX_DRIFT: f32 = 1.0;

/// How many not-a-name words are kept (the oldest go first).
const MAX_NOT_NAMES: usize = 300;

/// What this shop has taught the reader (kept in the database).
#[derive(Clone, Debug, PartialEq)]
pub struct Taught {
    /// (medicine id, word) pairs remembered from earlier scans.
    pub words: Vec<(String, String)>,
    pub weights: [f32; FEATURES],
    /// Words the reader once took for a name that wasn't ("STRONG", "FORMULA").
    pub not_names: Vec<String>,
}

impl Default for Taught {
    fn default() -> Self {
        Taught { words: Vec::new(), weights: START_WEIGHTS, not_names: Vec::new() }
    }
}

/// Words on most boxes that say nothing about which medicine it is.
const COMMON: [&str; 16] = [
    "TABLET",
    "TABLETS",
    "CAPSULE",
    "CAPSULES",
    "SYRUP",
    "SUSPENSION",
    "ORAL",
    "FILM",
    "COATED",
    "INJECTION",
    "CREAM",
    "OINTMENT",
    "DROPS",
    "MEDICINE",
    "STRIPS",
    "BLISTERS",
];

const MONTHS: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];

/// The words of `text` in capitals, split on anything that isn't a letter or digit.
fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_uppercase)
}

/// Words that could tell medicines apart: 4+ letters, not on every box.
/// A misread 0 or 1 inside a word becomes O or I ("AM0XIL" -> "AMOXIL").
fn distinctive(text: &str) -> impl Iterator<Item = String> + '_ {
    words(text)
        .filter(|w| w.chars().filter(|c| c.is_alphabetic()).count() * 4 >= w.chars().count() * 3)
        .map(|w| w.replace('0', "O").replace('1', "I"))
        .filter(|w| {
            let w = w.as_str();
            w.len() >= 4
                && w.chars().all(char::is_alphabetic)
                && !COMMON.contains(&w)
                && !MAKERS.contains(&w)
                && !INSTRUCTIONS.contains(&w)
        })
}

/// Equal, or one letter off for longer words (the reader sometimes misreads one).
fn close(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.len().min(b.len()) < 5 || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    // Edit distance, stopping early once it is past 1.
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut next = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            next.push((row[j] + usize::from(ca != cb)).min(row[j + 1] + 1).min(next[j] + 1));
        }
        if next.iter().min().is_some_and(|&m| m > 1) {
            return false;
        }
        row = next;
    }
    row[b.len()] <= 1
}

/// The share of `wanted` words found (allowing a misread letter) among `seen`.
fn hit_ratio(wanted: &[String], seen: &[String]) -> f32 {
    let hits = wanted.iter().filter(|w| seen.iter().any(|s| close(s, w))).count();
    hits as f32 / wanted.len().max(1) as f32
}

/// A medicine from the built-in list named on this line. Short or
/// multi-word names must appear exactly; longer ones may have a misread letter.
fn known_medicine(upper: &str, seen: &[String]) -> Option<&'static str> {
    medicines::GROUPS.iter().flat_map(|g| g.names.iter().copied()).find(|name| {
        if name.len() < 5 || name.contains([' ', '-']) {
            medicines::contains_phrase(upper, name)
        } else {
            seen.iter().any(|w| close(w, name))
        }
    })
}

/// Everything the scoring needs to know beyond the line itself.
struct Context<'a> {
    product_words: Vec<(&'a Product, Vec<String>)>,
    learned: HashSet<&'a str>,
    not_names: HashSet<&'a str>,
    /// The biggest print on each photo.
    biggest: HashMap<usize, f32>,
}

/// What the reader sees in one line (see `FEATURES`).
fn features(line: &Line, near_top: f32, cx: &Context) -> [f32; FEATURES] {
    let upper = line.text.to_uppercase();
    let seen: Vec<String> = distinctive(&line.text).collect();
    let all: Vec<String> = words(&line.text).collect();
    let count = all.len().max(1) as f32;
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    let letters = line.text.chars().filter(|c| c.is_alphabetic()).count();
    let printed = line.text.chars().filter(|c| !c.is_whitespace()).count().max(1);
    [
        flag(known_medicine(&upper, &seen).is_some()),
        cx.product_words.iter().map(|(_, name)| hit_ratio(name, &seen)).fold(0.0, f32::max),
        (seen.iter().filter(|w| cx.learned.contains(w.as_str())).count() as f32 / 2.0).min(1.0),
        line.height / cx.biggest.get(&line.photo).copied().unwrap_or(1.0).max(1.0),
        near_top,
        flag(strength(&upper).is_some()),
        all.iter().filter(|w| MAKERS.contains(&w.as_str())).count() as f32 / count,
        flag(
            all.iter().any(|w| INSTRUCTIONS.contains(&w.as_str()) || SLOGANS.contains(&w.as_str()))
                || seen.iter().any(|w| cx.not_names.contains(w.as_str())),
        ),
        1.0 - line.confidence.clamp(0.0, 1.0),
        flag(letters * 10 >= printed * 6),
        flag(all.len() <= 3),
    ]
}

/// A line as a name: maker and instruction words, strengths and dosage
/// forms dropped (they're added back tidily), the rest in Title Case.
fn tidy(text: &str) -> String {
    let title = |w: &str| {
        let mut chars = w.chars();
        match chars.next() {
            Some(first) if !w.chars().any(char::is_lowercase) => first.to_string() + &chars.as_str().to_lowercase(),
            _ => w.to_string(),
        }
    };
    text.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-'))
        .filter(|w| {
            let upper = w.to_uppercase();
            !w.is_empty()
                && w.chars().any(char::is_alphabetic)
                && !MAKERS.contains(&upper.as_str())
                && !INSTRUCTIONS.contains(&upper.as_str())
                && !FORMS.iter().any(|(form, _)| *form == upper)
                && strength(&upper).is_none()
                && !matches!(upper.as_str(), "MG" | "ML" | "MCG" | "G" | "IU")
                && !matches!(upper.as_str(), "BP" | "USP" | "IP" | "EP" | "PH" | "EUR") // pharmacopoeia marks
        })
        .map(title)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Turns the text of the photos into details, using what this shop has
/// taught the reader.
pub fn read_box(lines: &[Line], products: &[Product], taught: &Taught, today: NaiveDate) -> Found {
    let seen: Vec<String> =
        lines.iter().flat_map(|l| distinctive(&l.text)).collect::<HashSet<_>>().into_iter().collect();
    let all_text = lines.iter().map(|l| l.text.to_uppercase()).collect::<Vec<_>>().join(" \n ");

    // A medicine already listed: most of its name's words on the box, plus learned words.
    let mut learned_by_id: HashMap<&str, Vec<&str>> = HashMap::new();
    for (id, word) in &taught.words {
        learned_by_id.entry(id.as_str()).or_default().push(word.as_str());
    }
    let product_words: Vec<(&Product, Vec<String>)> =
        products.iter().map(|p| (p, distinctive(&p.name).collect())).collect();
    let on_box = |w: &str| seen.iter().any(|s| close(s, w));
    let score = |(p, name): &(&Product, Vec<String>)| {
        let named = name.iter().filter(|w| on_box(w)).count() as f32 / name.len().max(1) as f32;
        let remembered = learned_by_id.get(p.id.as_str()).map_or(0, |ws| ws.iter().filter(|w| on_box(w)).count());
        named * 2.0 + remembered.min(5) as f32 * 0.6
    };
    let matched = product_words.iter().map(|pw| (pw.0, score(pw))).filter(|(_, s)| *s >= 1.6);
    let matched = matched.max_by(|a, b| a.1.total_cmp(&b.1)).map(|(p, _)| p);

    // Score every line that could be the name.
    let mut biggest: HashMap<usize, f32> = HashMap::new();
    for l in lines {
        let size = biggest.entry(l.photo).or_default();
        *size = size.max(l.height);
    }
    let cx = Context {
        product_words,
        learned: taught.words.iter().map(|(_, w)| w.as_str()).collect(),
        not_names: taught.not_names.iter().map(String::as_str).collect(),
        biggest,
    };
    let mut candidates: Vec<Candidate> = lines
        .iter()
        .filter(|l| !tidy(&l.text).is_empty())
        .map(|l| {
            let on_photo = lines.iter().filter(|o| o.photo == l.photo);
            let (above, total) = on_photo.fold((0, 0), |(a, n), o| (a + usize::from(o.top < l.top), n + 1));
            let near_top = 1.0 - above as f32 / total.max(1) as f32;
            let features = features(l, near_top, &cx);
            let score = features.iter().zip(&taught.weights).map(|(f, w)| f * w).sum();
            Candidate { name: tidy(&l.text), features, score }
        })
        .collect();
    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    candidates.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));

    // The name: a listed medicine as the shop calls it, or the best line
    // with its strength and form ("Panadol Extra 500mg Tablets").
    let strength = strength(&all_text);
    let form = FORMS.iter().find(|(printed, _)| medicines::contains_phrase(&all_text, printed)).map(|(_, f)| *f);
    let full_name = |base: &str| {
        let mut name = base.to_string();
        for extra in [strength.as_deref(), form].into_iter().flatten() {
            if !name.to_lowercase().contains(&extra.to_lowercase()) {
                name = format!("{name} {extra}");
            }
        }
        name
    };
    let name = match matched {
        Some(p) => p.name.clone(),
        None => candidates.first().map(|c| full_name(&c.name)).unwrap_or_default(),
    };
    let alternatives = candidates.iter().map(|c| full_name(&c.name)).filter(|n| *n != name).take(3).collect();

    // The category: the matched medicine's, else from the medicine list
    // (the name's line first), in the shop's own wording if it has one.
    let category = match matched.map(|p| p.category.trim()).filter(|c| !c.is_empty()) {
        Some(c) => c.to_string(),
        None => list_category(&[&name, &all_text], products).unwrap_or_default(),
    };

    Found {
        product_id: matched.map(|p| p.id.clone()),
        name,
        category,
        batch: batch(lines),
        expiry: expiry(lines, today),
        words: seen.iter().take(30).cloned().collect(),
        alternatives,
        candidates,
        lines: lines.to_vec(),
    }
}

/// The category the built-in medicine list gives a box's `texts` (the name
/// first, then the rest), using the shop's
/// own category name when it has a matching one ("Malaria" for Antimalarial).
pub fn list_category(texts: &[&str], products: &[Product]) -> Option<String> {
    let upper: Vec<String> = texts.iter().map(|t| t.to_uppercase()).collect();
    let upper: Vec<&str> = upper.iter().map(String::as_str).collect();
    // Exact words first; then a name with one misread letter ("PANAD0L", "AMOXlL").
    let fuzzy = || {
        upper
            .iter()
            .find_map(|t| known_medicine(t, &distinctive(t).collect::<Vec<_>>()).and_then(medicines::group_named))
    };
    let group = medicines::group_of(&upper).or_else(fuzzy)?;
    let shop: Vec<String> = products
        .iter()
        .map(|p| p.category.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    Some(medicines::shop_category(group, &shop).cloned().unwrap_or_else(|| group.category.to_string()))
}

/// Learns from a scan saved as `saved_name`. When the reader picked the
/// wrong line: that line's words become not-a-name words, and the weights
/// move towards what the right line looked like (a perceptron step, kept
/// within `MAX_DRIFT` of the start). Returns true if anything changed.
/// (The box's words for the medicine are remembered separately, in the database.)
pub fn train(taught: &mut Taught, found: &Found, saved_name: &str) -> bool {
    let wanted: Vec<String> = distinctive(saved_name).collect();
    if wanted.is_empty() {
        return false;
    }
    // Words of the saved name are names, whatever was taught before.
    let before = taught.not_names.len();
    taught.not_names.retain(|w| !wanted.iter().any(|n| close(n, w)));
    let mut changed = taught.not_names.len() != before;

    let overlap = |c: &Candidate| hit_ratio(&wanted, &distinctive(&c.name).collect::<Vec<_>>());
    let Some(picked) = found.candidates.first() else { return changed };
    if overlap(picked) >= 0.5 {
        return changed; // the reader's line was right (the user may have tidied it)
    }
    for word in distinctive(&picked.name) {
        let a_name = known_medicine(&word, std::slice::from_ref(&word)).is_some();
        if !a_name && !wanted.iter().any(|n| close(n, &word)) && !taught.not_names.contains(&word) {
            taught.not_names.push(word);
            changed = true;
        }
    }
    let excess = taught.not_names.len().saturating_sub(MAX_NOT_NAMES);
    taught.not_names.drain(..excess);

    let best = found.candidates.iter().max_by(|a, b| overlap(a).total_cmp(&overlap(b)));
    if let Some(right) = best.filter(|c| overlap(c) >= 0.5) {
        for (k, w) in taught.weights.iter_mut().enumerate() {
            let start = START_WEIGHTS[k];
            let moved = *w + RATE * (right.features[k] - picked.features[k]);
            *w = moved.clamp(start - MAX_DRIFT, start + MAX_DRIFT);
        }
        changed = true;
    }
    changed
}

/// One saved scan as a line of JSON for `training/scans.jsonl`: every line
/// the reader saw (text, print height, place, confidence, photo), what it
/// guessed and what was saved. These files are what a better reader is
/// tuned from later; nothing is sent anywhere.
pub fn training_example(found: &Found, saved_name: &str, saved_category: &str, when: &str) -> String {
    let lines: Vec<String> = found
        .lines
        .iter()
        .map(|l| {
            format!(
                "{{\"text\":{},\"h\":{:.1},\"top\":{:.1},\"conf\":{:.3},\"photo\":{}}}",
                json_text(&l.text),
                l.height,
                l.top,
                l.confidence,
                l.photo
            )
        })
        .collect();
    format!(
        "{{\"when\":{},\"read_name\":{},\"read_category\":{},\"saved_name\":{},\"saved_category\":{},\"lines\":[{}]}}",
        json_text(when),
        json_text(&found.name),
        json_text(&found.category),
        json_text(saved_name),
        json_text(saved_category),
        lines.join(",")
    )
}

/// `text` as a JSON string, quotes and control characters escaped.
fn json_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Weights saved as text ("3.00,3.00,...") and read back; anything
/// unreadable gives the starting weights.
pub fn weights_to_text(weights: &[f32; FEATURES]) -> String {
    weights.iter().map(|w| format!("{w:.3}")).collect::<Vec<_>>().join(",")
}

pub fn weights_from_text(text: &str) -> [f32; FEATURES] {
    let parsed: Vec<f32> = text.split(',').filter_map(|v| v.trim().parse().ok()).collect();
    parsed.try_into().unwrap_or(START_WEIGHTS)
}

/// "500MG", "500 MG" or "250MG/5ML" -> "500mg" / "250mg/5ml".
fn strength(text: &str) -> Option<String> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    tokens.iter().enumerate().find_map(|(i, t)| {
        let digits: String = t.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        if digits.is_empty() {
            return None;
        }
        let rest = &t[digits.len()..];
        if rest.starts_with('%') {
            return Some(format!("{digits}%")); // creams and drops: "1%", "0.5%"
        }
        let unit = if rest.is_empty() { tokens.get(i + 1).copied().unwrap_or("") } else { rest };
        let unit = unit.trim_matches(|c: char| !c.is_alphanumeric() && c != '/').to_lowercase();
        // A lone "100 ml" is the bottle's size, not the strength ("250mg/5ml" is).
        ["mg", "g", "ml", "mcg", "iu"]
            .iter()
            .any(|u| (unit == *u && unit != "ml") || unit.starts_with(&format!("{u}/")))
            .then(|| format!("{digits}{unit}"))
    })
}

/// The batch number: the code after "BATCH", "B.NO", "LOT" or "BN" (on the
/// same line or the next one), which has a digit in it.
fn batch(lines: &[Line]) -> String {
    let keys = ["BATCH", "BNO", "B/NO", "LOT", "BN", "B/N"];
    for (i, line) in lines.iter().enumerate() {
        let upper = line.text.to_uppercase();
        let tokens: Vec<&str> = upper.split([' ', ':', '#']).filter(|t| !t.is_empty()).collect();
        let Some(at) = tokens.iter().position(|t| keys.contains(&t.replace('.', "").as_str())) else { continue };
        let next_line = lines.get(i + 1).map(|l| l.text.to_uppercase()).unwrap_or_default();
        let after = tokens[at + 1..].iter().copied().chain(next_line.split_whitespace());
        let code = after
            .map(|t| t.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
            .filter(|t| !matches!(*t, "NO" | "NUMBER" | "NUM"))
            .find(|t| {
                t.len() >= 3
                    && t.chars().any(|c| c.is_ascii_digit())
                    && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            });
        if let Some(code) = code {
            return code.to_string();
        }
    }
    String::new()
}

/// The expiry: a date on a line saying "EXP" (or just after one), else the
/// latest future date on the box (the manufacture date is in the past).
fn expiry(lines: &[Line], today: NaiveDate) -> Option<NaiveDate> {
    let marked = |i: usize| {
        let says_exp = |l: &Line| words(&l.text).any(|w| w.starts_with("EXP") || w == "BEFORE");
        says_exp(&lines[i]) || (i > 0 && says_exp(&lines[i - 1]) && dates(&lines[i - 1].text, today).is_empty())
    };
    let on_exp_line = (0..lines.len()).filter(|&i| marked(i)).flat_map(|i| dates(&lines[i].text, today)).min();
    on_exp_line.or_else(|| lines.iter().flat_map(|l| dates(&l.text, today)).max())
}

/// Future dates written on a line: "06/2027", "06-27", "30/06/2027",
/// "2027-06", "JUN 2027", "JUN.27" or "JUNE-2027".
fn dates(line: &str, today: NaiveDate) -> Vec<NaiveDate> {
    let upper = line.to_uppercase();
    let tokens: Vec<&str> = upper.split([' ', ':', ',']).filter(|t| !t.is_empty()).collect();
    let mut found = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        // "EXP.DEC.27" or "EXP:06/27" -> "DEC.27" / "06/27".
        let token = token.strip_prefix("EXPIRY").or_else(|| token.strip_prefix("EXP")).unwrap_or(token);
        let token = token.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        let text = match MONTHS.iter().position(|m| token.starts_with(m)) {
            Some(month) => {
                let rest = token.trim_start_matches(|c: char| c.is_ascii_alphabetic() || matches!(c, '.' | '-' | '/'));
                let year = if rest.is_empty() { tokens.get(i + 1).copied().unwrap_or("") } else { rest };
                format!("{:02}/{}", month + 1, year.trim_matches(|c: char| !c.is_ascii_digit()))
            }
            None => token.replace('.', "/"),
        };
        if let Ok(date) = validate::parse_expiry(&text, today) {
            found.push(date);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, height: f32) -> Line {
        Line { text: text.into(), height, top: 0.0, confidence: 0.9, photo: 0 }
    }

    /// Lines one under the other on one photo.
    fn stacked(rows: &[(&str, f32)]) -> Vec<Line> {
        rows.iter().enumerate().map(|(i, (t, h))| Line { top: i as f32 * 40.0, ..line(t, *h) }).collect()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 16).unwrap()
    }

    #[test]
    fn reads_a_new_box() {
        let lines = stacked(&[
            ("GSK", 70.0),
            ("PANADOL EXTRA", 64.0),
            ("Paracetamol 500 mg, Caffeine 65 mg", 18.0),
            ("Each tablet contains", 14.0),
            ("MFG: 01/2025", 12.0),
            ("EXP: JUN 2028", 12.0),
            ("B.No. KX2291", 12.0),
        ]);
        let found = read_box(&lines, &[], &Taught::default(), today());
        assert_eq!(found.name, "Panadol Extra 500mg Tablets", "the maker's big logo isn't the name");
        assert_eq!(found.category, "Painkiller");
        assert_eq!(found.batch, "KX2291");
        assert_eq!(found.expiry, NaiveDate::from_ymd_opt(2028, 6, 30));
        assert!(found.words.contains(&"PANADOL".to_string()) && !found.words.contains(&"TABLET".to_string()));
        assert!(found.alternatives.iter().any(|a| a.starts_with("Paracetamol")), "{:?}", found.alternatives);
        assert_eq!(found.product_id, None);
    }

    #[test]
    fn a_known_name_beats_bigger_print() {
        // A big slogan and the maker in big print; the brand smaller and read unsure.
        let lines = stacked(&[
            ("FAST RELIEF", 80.0),
            ("Cosmos Limited", 60.0),
            ("Lonart", 40.0),
            ("Artemether 20mg / Lumefantrine 120mg", 16.0),
            ("Dispersible tablets", 16.0),
        ]);
        let mut lines = lines;
        lines[2].confidence = 0.45;
        let shop = vec![Product { name: "Old stock".into(), category: "Malaria".into(), ..Default::default() }];
        let found = read_box(&lines, &shop, &Taught::default(), today());
        assert_eq!(found.name, "Lonart 20mg Tablets");
        assert_eq!(found.category, "Malaria", "the shop's own word for the category");
    }

    #[test]
    fn matches_a_known_medicine_even_misread() {
        let known = Product {
            id: "p1".into(),
            name: "Amoxil 500mg".into(),
            category: "Antibiotic".into(),
            ..Default::default()
        };
        let other = Product { id: "p2".into(), name: "Panadol".into(), ..Default::default() };
        let lines =
            stacked(&[("AM0XIL", 50.0), ("Amoxicillin capsules", 20.0), ("Lot 7731A", 10.0), ("Exp. 11/27", 10.0)]);
        let found = read_box(&lines, &[other.clone(), known.clone()], &Taught::default(), today());
        assert_eq!((found.product_id.as_deref(), found.name.as_str()), (Some("p1"), "Amoxil 500mg"));
        assert_eq!((found.batch.as_str(), found.expiry), ("7731A", NaiveDate::from_ymd_opt(2027, 11, 30)));
        // A box with only learned words (the brand isn't on this side).
        let words = ["OPTIZORB", "CAPLETS", "RELEASE"].map(|w| ("p2".to_string(), w.to_string())).to_vec();
        let taught = Taught { words, ..Taught::default() };
        let side =
            stacked(&[("GlaxoSmithKline", 20.0), ("Optizorb caplets, fast release", 20.0), ("EXP 2029-01", 10.0)]);
        let found = read_box(&side, &[other, known], &taught, today());
        assert_eq!(found.product_id.as_deref(), Some("p2"));
        assert_eq!(found.expiry, NaiveDate::from_ymd_opt(2029, 1, 31));
    }

    #[test]
    fn learns_from_corrections() {
        // An unknown brand in small print; a big unknown tagline wins at first.
        let lines = stacked(&[("Zentra Max Care", 70.0), ("Zentramol", 30.0), ("500mg tablets", 14.0)]);
        let mut taught = Taught::default();
        let first = read_box(&lines, &[], &taught, today());
        assert_eq!(first.name, "Zentra Max Care 500mg Tablets");
        // The user saves it as "Zentramol 500mg" once: the tagline's words
        // are now known not to be a name, and the weights moved a little.
        assert!(train(&mut taught, &first, "Zentramol 500mg"));
        assert_eq!(taught.not_names, ["ZENTRA", "CARE"], "short words aren't kept");
        assert_ne!(taught.weights, START_WEIGHTS);
        let after = read_box(&lines, &[], &taught, today());
        assert_eq!(after.name, "Zentramol 500mg Tablets");
        // A right guess changes nothing.
        assert!(!train(&mut taught, &after, "Zentramol 500mg"));
        // Many corrections can't spoil the other boxes: the weights stay near the start.
        for _ in 0..50 {
            train(&mut taught, &first, "Zentramol 500mg");
        }
        assert!(taught.weights.iter().zip(START_WEIGHTS).all(|(w, s)| (w - s).abs() <= MAX_DRIFT + 1e-4));
        let still_right = BOXES
            .iter()
            .filter(|(rows, name, _)| read_box(&stacked(rows), &[], &taught, today()).name.starts_with(name))
            .count();
        assert!(still_right == BOXES.len(), "{still_right} of {} still read right", BOXES.len());
        // Saving a name brings its words back as names.
        let mut renamed = taught.clone();
        assert!(train(&mut renamed, &first, "Zentra Care"));
        assert!(renamed.not_names.is_empty());
        // Weights survive being saved as text; rubbish gives the start.
        let w = taught.weights;
        assert_eq!(weights_from_text(&weights_to_text(&w)), w.map(|w| (w * 1000.0).round() / 1000.0));
        assert_eq!(weights_from_text("rubbish"), START_WEIGHTS);
    }

    #[test]
    fn slogans_are_not_names() {
        let lines = stacked(&[("Strong Formula", 70.0), ("Zentramol", 30.0), ("500mg tablets", 14.0)]);
        assert_eq!(read_box(&lines, &[], &Taught::default(), today()).name, "Zentramol 500mg Tablets");
    }

    type Rows = &'static [(&'static str, f32)];

    /// Boxes as the reader returns them (text, print height, top to bottom):
    /// the name it should pick (the start of it) and the category.
    #[rustfmt::skip]
    const BOXES: &[(Rows, &str, &str)] = &[
        (&[("GSK", 60.0), ("Augmentin", 55.0), ("625mg", 30.0), ("Amoxicillin/Clavulanic acid", 16.0),
            ("14 film-coated tablets", 14.0), ("Rx only", 12.0)], "Augmentin 625mg Tablets", "Antibiotic"),
        (&[("NOVARTIS", 40.0), ("Coartem", 70.0), ("20/120 mg", 24.0), ("artemether/lumefantrine", 16.0),
            ("24 Dispersible Tablets", 14.0)], "Coartem", "Antimalarial"),
        (&[("Piriton", 60.0), ("Chlorphenamine Maleate 4mg", 18.0), ("Allergy tablets", 20.0),
            ("30 tablets", 14.0), ("GSK", 30.0)], "Piriton 4mg Tablets", "Allergy"),
        (&[("SANOFI", 30.0), ("FLAGYL", 50.0), ("Metronidazole 400mg", 22.0), ("Tablets", 16.0)],
            "Flagyl 400mg Tablets", "Antibiotic"),
        (&[("Amoxicillin Capsules BP 500mg", 40.0), ("Each capsule contains:", 14.0),
            ("Amoxicillin trihydrate BP equivalent to Amoxicillin 500mg", 12.0),
            ("Manufactured by: Dawa Limited, Nairobi Kenya", 12.0), ("100 Capsules", 18.0)],
            "Amoxicillin 500mg Capsules", "Antibiotic"),
        (&[("BENYLIN", 55.0), ("Original", 30.0), ("Cough syrup", 22.0), ("100 ml", 16.0)], "Benylin", "Cough & cold"),
        (&[("Chloramphenicol", 40.0), ("Eye Drops 0.5% w/v", 24.0), ("10 ml", 14.0), ("Sterile", 14.0)],
            "Chloramphenicol", "Eye & ear"),
        (&[("MARA MOJA", 60.0), ("Pain relief tablets", 20.0), ("Aspirin, Paracetamol, Caffeine", 14.0),
            ("Reckitt", 20.0)], "Mara Moja Tablets", "Painkiller"),
        (&[("Macleods", 30.0), ("TLD", 40.0), ("Tenofovir/Lamivudine/Dolutegravir", 20.0),
            ("300mg/300mg/50mg", 16.0), ("30 Tablets", 14.0)], "", "Antiretroviral (ARV)"),
        (&[("Merck", 30.0), ("Glucophage", 50.0), ("Metformin hydrochloride", 20.0), ("500 mg", 24.0),
            ("Film-coated tablets", 16.0)], "Glucophage 500mg Tablets", "Diabetes"),
        (&[("Cosmos Limited", 45.0), ("AMLODIPINE TABLETS 5MG", 35.0), ("Keep out of reach of children", 14.0),
            ("28 Tablets", 14.0)], "Amlodipine 5mg Tablets", "Blood pressure & heart"),
        (&[("Zentel", 60.0), ("Albendazole 400 mg", 24.0), ("1 Chewable tablet", 16.0), ("GSK", 30.0)],
            "Zentel 400mg Tablets", "Dewormer"),
        (&[("Ventolin", 50.0), ("Evohaler", 30.0), ("Salbutamol 100 micrograms", 16.0),
            ("200 metered actuations", 14.0)], "Ventolin", "Asthma"),
        (&[("Panad0l", 60.0), ("Advance", 30.0), ("Paracetamol 500mg", 18.0), ("EXP 08/2028", 12.0)],
            "Panad0l", "Painkiller"),
        (&[("Fluconazole", 36.0), ("Capsules USP 150 mg", 24.0), ("1 Capsule", 14.0), ("Regal Pharmaceuticals", 20.0)],
            "Fluconazole 150mg Capsules", "Antifungal"),
    ];

    #[test]
    fn reads_common_boxes() {
        let mut wrong = Vec::new();
        for (rows, name, category) in BOXES {
            let found = read_box(&stacked(rows), &[], &Taught::default(), today());
            if !found.name.starts_with(name) || found.category != *category {
                wrong.push(format!("{:?} -> {:?} / {:?}", rows[0].0, found.name, found.category));
            }
        }
        assert!(wrong.is_empty(), "{} of {} boxes read wrong:\n{}", wrong.len(), BOXES.len(), wrong.join("\n"));
        // The shop's own category wording wins.
        let shop = vec![Product { name: "Septrin".into(), category: "Antibiotics".into(), ..Default::default() }];
        let flagyl = read_box(&stacked(BOXES[3].0), &shop, &Taught::default(), today());
        assert_eq!(flagyl.category, "Antibiotics");
    }

    #[test]
    fn dates_and_strengths() {
        let d = |s| dates(s, today());
        assert_eq!(d("EXP 30/06/2027"), [NaiveDate::from_ymd_opt(2027, 6, 30).unwrap()]);
        assert_eq!(d("Expiry: JUNE-2027"), [NaiveDate::from_ymd_opt(2027, 6, 30).unwrap()]);
        assert_eq!(d("EXP.DEC.27"), [NaiveDate::from_ymd_opt(2027, 12, 31).unwrap()]);
        assert!(d("MFD 03/2024").is_empty(), "past dates are not an expiry");
        assert_eq!(strength("AMOXIL 250MG/5ML SUSPENSION"), Some("250mg/5ml".into()));
        assert_eq!(strength("tablets 500 mg"), Some("500mg".into()));
        assert_eq!(strength("pack of 20"), None);
        assert_eq!(strength("Syrup 100 ml"), None);
        assert_eq!(strength("Eye Drops 0.5% w/v"), Some("0.5%".into()));
        assert!(close("AMOXIL", "AM0XIL") && !close("PANADOL", "PIRITON"));
        assert_eq!(tidy("COSMOS LTD PANADOL 500MG TABLETS"), "Panadol");
    }
}
