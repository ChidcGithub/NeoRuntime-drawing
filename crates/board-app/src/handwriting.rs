//! Local manual samples and bounded numeric style learning; no models or recognition.
//! Strict `render` requires labelled samples. `render_adaptive` styles only missing
//! glyphs supplied by the caller (e.g. in-repo single-character HWR templates, then
//! renderer font outlines). Synthesized ink never becomes a stored/learned sample.
//!
//! Samples use a 128-square canvas, cap top 24 and baseline 96. Only horizontal
//! side bearings are trimmed; vertical positions (including dots/minus signs) stay intact.
//! The 72-unit cap height maps to the source object's size (normally 26). Layout
//! reserves the whole sample canvas plus any pen overhang, with 16 units between
//! lines. Advances are ink width (including the final clamped pen's bounded joins
//! on both sides) plus 12 units, with an 18-unit minimum ink width. Vertical padding moves
//! the whole line and its baseline together. This is not font shaping:
//! no ligatures, kerning, bidi, combining-mark attachment or automatic line wrapping.
//! Fractions and radicals alone use synthetic structural ink, at the profile's
//! median pen width. Whitespace-only answers are rejected (core requires ink).
//! Callers must explicitly load/save, and may keep the original object on any error.

use board_core::{
    Color, HandwritingStroke, MAX_BRUSH_WIDTH, MAX_HANDWRITING_COORD, MAX_HANDWRITING_POINTS,
    MAX_HANDWRITING_STROKES, MAX_HANDWRITING_TEXT_BYTES, MAX_HANDWRITING_TIME, MIN_BRUSH_WIDTH,
    MathLayout, ObjectKind, Point, StrokePoint, Style,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

#[path = "handwriting_style.rs"]
mod handwriting_style;
use handwriting_style::LearnedStyle;

const VERSION: u32 = 2;
const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;
const MAX_GLYPHS: usize = 96;
const MAX_VARIANTS: usize = 3;
const MAX_SAMPLE_STROKES: usize = 16;
const MAX_SAMPLE_POINTS: usize = 2048;
const MAX_PROFILE_POINTS: usize = 65536;
const MAX_INPUT_CHARS: usize = 4096;
const CAP_HEIGHT: f32 = 72.0;
// Shared only by this synthesis pipeline, not the standard Math/font layout.
// Arbitrarily positioned manual samples retain their authored baseline offsets.
pub(crate) const MATH_AXIS: f32 = 0.5;

#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profile {
    // A sorted list also lets validation reject duplicate labels on import,
    // unlike deserializing directly into a map (which silently overwrites keys).
    glyphs: Vec<Glyph>,
    #[serde(default, skip_serializing_if = "LearnedStyle::is_default")]
    style: LearnedStyle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Glyph {
    label: char,
    variants: Vec<Vec<HandwritingStroke>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileFile<T> {
    version: u32,
    profile: T,
}

impl Profile {
    pub fn add_sample(
        &mut self,
        label: char,
        strokes: Vec<HandwritingStroke>,
    ) -> Result<(), String> {
        let total = self.validate()?;
        validate_label(label)?;
        let points = validate_sample(&strokes)?;
        if total + points > MAX_PROFILE_POINTS {
            return Err("Profile exceeds 65536 points".into());
        }
        match self.glyphs.binary_search_by_key(&label, |g| g.label) {
            Ok(index) => {
                if self.glyphs[index].variants.len() >= MAX_VARIANTS {
                    return Err(
                        "At most 3 variants per character; delete existing samples first".into(),
                    );
                }
                self.glyphs[index].variants.push(strokes);
            }
            Err(index) => {
                if self.glyphs.len() >= MAX_GLYPHS {
                    return Err("Profile exceeds 96 characters".into());
                }
                self.glyphs.insert(
                    index,
                    Glyph {
                        label,
                        variants: vec![strokes],
                    },
                );
            }
        }
        Ok(())
    }

    /// Removes all variants for this character.
    pub fn remove(&mut self, label: char) {
        self.glyphs.retain(|glyph| glyph.label != label);
    }

    /// Observe one newly committed local real pen stroke, with no confirmation or
    /// opt-in in the engine. The caller MUST gate sources and call once only:
    /// never replay undo/load, imports, generated answers or remote write-back.
    /// Returns false without changing state for invalid/non-writing-like ink.
    /// Only bounded numeric statistics are retained, not unlabelled points.
    pub fn observe_stroke(&mut self, points: &[StrokePoint], style: Style) -> bool {
        self.style.observe(points, style)
    }

    /// Accepted local observations (saturating); manual/generated glyphs do not count.
    pub fn learned_strokes(&self) -> u32 {
        self.style.count()
    }

    /// Human-readable counters and normalized metrics; explicitly marks defaults.
    pub fn style_summary(&self) -> String {
        self.style.summary()
    }

    pub fn clear(&mut self) {
        self.glyphs.clear();
        self.style = LearnedStyle::default();
    }

    /// (Distinct characters, total sample variants).
    pub fn counts(&self) -> (usize, usize) {
        (
            self.glyphs.len(),
            self.glyphs.iter().map(|g| g.variants.len()).sum(),
        )
    }

    /// Unicode scalar order, without separators.
    pub fn labels(&self) -> String {
        self.glyphs.iter().map(|g| g.label).collect()
    }

    pub fn sample_count(&self, label: char) -> usize {
        self.glyph(label).map_or(0, |g| g.variants.len())
    }

    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;
        let mut output = LimitedJson(Vec::new());
        serde_json::to_writer(
            &mut output,
            &ProfileFile {
                version: VERSION,
                profile: self,
            },
        )
        .map_err(|error| error.to_string())?;
        String::from_utf8(output.0).map_err(|error| error.to_string())
    }

    /// Returns a fully validated replacement; never mutates the current profile.
    /// Direct serde deserialization is not an import API; render/add/save also
    /// validate so it cannot bypass the profile's invariants.
    /// Unknown fields are rejected in the envelope, profile, glyph and learned
    /// statistics. Nested core strokes/points/styles/colors keep their existing
    /// serde compatibility policy (unknown fields ignored, not preserved on save).
    pub fn from_json(json: &str) -> Result<Self, String> {
        if json.len() > MAX_JSON_BYTES {
            return Err("Profile JSON exceeds 8 MiB".into());
        }
        let mut file: ProfileFile<Self> = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if file.version != 1 && file.version != VERSION {
            return Err(format!(
                "Unsupported handwriting profile version: {}",
                file.version
            ));
        }
        // v1 had only labelled samples, never learned style. Do not silently
        // interpret a v2 style payload as v1 or let version downgrades hide it.
        if file.version == 1 {
            if !file.profile.style.is_default() {
                return Err("Version 1 profiles cannot contain learned style".into());
            }
            file.profile.style = LearnedStyle::default();
        }
        file.profile.validate()?;
        Ok(file.profile)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|e| e.to_string())?
            .take(MAX_JSON_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_JSON_BYTES {
            return Err("Profile JSON exceeds 8 MiB".into());
        }
        Self::from_json(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)
    }

    /// Same-directory atomic replacement, without deleting the old target first.
    /// No automatic persistence, directory creation or background I/O.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = self.to_json()?;
        if path.file_name().is_none() {
            return Err("Profile path needs a file name".into());
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let temporary = parent.join(format!(".handwriting-{}.tmp", board_core::new_id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        let result = (|| -> std::io::Result<()> {
            file.write_all(json.as_bytes())?;
            file.sync_all()
        })();
        // Close even on write failure, so cleanup works on Windows as well.
        drop(file);
        let result = result.and_then(|()| fs::rename(&temporary, path));
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|e| e.to_string())
    }

    /// Freezes ink in local coordinates. `text` is the exact original semantic
    /// text for Math; for Text it must equal the object's text (no silent mismatch).
    /// Variant counters reset per call, and are shared across all layout leaves.
    pub fn render(&self, kind: &ObjectKind, text: &str) -> Result<ObjectKind, String> {
        self.validate()?;
        validate_text(text)?;
        let (position, size, color, layout, plain) = match kind {
            ObjectKind::Text {
                position,
                size,
                color,
                text: original,
            } => {
                if original != text {
                    return Err("Semantic text does not match the Text object".into());
                }
                (*position, *size, *color, None, Some(original.as_str()))
            }
            ObjectKind::Math {
                position,
                size,
                color,
                layout,
            } => {
                // Iterative core validation bounds depth, nodes and leaf bytes
                // before our recursion, cloning, or allocating child layouts.
                layout.validate().map_err(|e| e.to_string())?;
                (*position, *size, *color, Some(layout), None)
            }
            _ => return Err("Handwriting generation accepts only Text or Math".into()),
        };
        if !valid_coord(position.x)
            || !valid_coord(position.y)
            || !size.is_finite()
            || size <= 0.0
            || size > MAX_HANDWRITING_COORD
            || size / CAP_HEIGHT == 0.0
        {
            return Err("Invalid handwriting position or font size".into());
        }
        let mut missing = BTreeSet::new();
        if let Some(layout) = layout {
            self.check_layout_chars(layout, &mut missing)?;
        } else if let Some(plain) = plain {
            self.check_chars(plain, &mut missing)?;
        }
        if !missing.is_empty() {
            return Err(format!(
                "Missing handwriting samples: {}",
                missing.into_iter().collect::<String>()
            ));
        }
        let mut planner = Planner {
            profile: self,
            occurrences: BTreeMap::new(),
            strokes: 0,
            points: 0,
            pen_width: self.median_width(),
        };
        let plan = if let Some(layout) = layout {
            planner.layout(layout, size)?
        } else {
            planner.text(plain.unwrap_or_default(), size)?
        };
        if planner.strokes == 0 {
            return Err("Answer contains no handwriting strokes".into());
        }
        let mut output = Output {
            position,
            color,
            strokes: Vec::new(),
            points: 0,
        };
        plan.emit(Point::default(), &mut output)?;
        Ok(ObjectKind::Handwritten {
            position,
            text: text.to_owned(),
            layout: layout.cloned(),
            strokes: output.strokes,
        })
    }

    /// Synthesize missing glyphs in a temporary profile, never modifying `self`.
    /// `fallback` is called once per missing non-whitespace character (Unicode
    /// order). Supply 1..16 strokes / <=2048 points in the 0..128 sample canvas,
    /// cap top y=24, baseline y=96, including punctuation's vertical placement.
    /// All missing characters go through the provider; no font/model dependency
    /// or implicit replacement glyph is introduced here. Provider errors and any
    /// budget failure reject the whole answer so callers can retain standard text.
    /// Manual variants retain their exact geometry, widths, pressure and timing
    /// through the existing layout path. Only synthesized samples receive style.
    pub fn render_adaptive(
        &self,
        kind: &ObjectKind,
        text: &str,
        mut fallback: impl FnMut(char) -> Result<Vec<HandwritingStroke>, String>,
    ) -> Result<ObjectKind, String> {
        self.validate()?;
        validate_text(text)?;
        let mut needed = BTreeMap::new();
        let (mut output_strokes, mut output_points) = (0, 0);
        let (position, size) = match kind {
            ObjectKind::Text {
                position,
                size,
                text: original,
                ..
            } => {
                if original != text {
                    return Err("Semantic text does not match the Text object".into());
                }
                count_chars(original, &mut needed)?;
                (position, *size)
            }
            ObjectKind::Math {
                position,
                size,
                layout,
                ..
            } => {
                layout.validate().map_err(|e| e.to_string())?;
                count_layout(layout, &mut needed, &mut output_strokes, &mut output_points)?;
                (position, *size)
            }
            _ => return Err("Handwriting generation accepts only Text or Math".into()),
        };
        if !valid_coord(position.x)
            || !valid_coord(position.y)
            || !size.is_finite()
            || size <= 0.0
            || size > MAX_HANDWRITING_COORD
            || size / CAP_HEIGHT == 0.0
        {
            return Err("Invalid handwriting position or font size".into());
        }
        if needed.len() > MAX_GLYPHS {
            return Err(
                "Adaptive handwriting exceeds 96 distinct characters; use standard text".into(),
            );
        }
        if needed.keys().all(|ch| self.glyph(*ch).is_some()) {
            return self.render(kind, text);
        }
        // Do not clone the entire user profile: unrelated manual glyphs must not
        // consume the transient quota and block a small answer on a full profile.
        let mut scratch = Self {
            glyphs: Vec::with_capacity(needed.len()),
            style: self.style.clone(),
        };
        let mut profile_points = 0;
        // Count every occurrence, including cycling manual variants across leaves,
        // before requesting fallbacks. Previously each add_sample revalidated all
        // preceding samples, and output limits were checked only after synthesis.
        for (&ch, &count) in &needed {
            if let Some(glyph) = self.glyph(ch) {
                for (i, sample) in glyph.variants.iter().enumerate() {
                    let points: usize = sample.iter().map(|s| s.points.len()).sum();
                    profile_points += points;
                    let uses = count / glyph.variants.len()
                        + usize::from(i < count % glyph.variants.len());
                    reserve(
                        &mut output_strokes,
                        &mut output_points,
                        uses * sample.len(),
                        uses * points,
                    )?;
                }
            }
        }
        for (ch, count) in needed {
            let glyph = if let Some(glyph) = self.glyph(ch) {
                glyph.clone()
            } else {
                let mut sample = fallback(ch)
                    .map_err(|error| format!("Handwriting fallback for {ch}: {error}"))?;
                let points = validate_sample(&sample)?;
                reserve(
                    &mut output_strokes,
                    &mut output_points,
                    count * sample.len(),
                    count * points,
                )?;
                if points > MAX_PROFILE_POINTS.saturating_sub(profile_points) {
                    return Err("Profile exceeds 65536 points".into());
                }
                profile_points += points;
                self.style.apply(&mut sample);
                validate_sample(&sample)?;
                Glyph {
                    label: ch,
                    variants: vec![sample],
                }
            };
            // Keys are unique/sorted, glyph count was bounded above, and manual
            // samples were validated once at entry. Admit only the new sample.
            scratch.glyphs.push(glyph);
        }
        scratch.render(kind, text)
    }

    fn glyph(&self, label: char) -> Option<&Glyph> {
        self.glyphs
            .binary_search_by_key(&label, |g| g.label)
            .ok()
            .map(|i| &self.glyphs[i])
    }

    fn validate(&self) -> Result<usize, String> {
        self.style.validate()?;
        if self.glyphs.len() > MAX_GLYPHS {
            return Err("Profile exceeds 96 characters".into());
        }
        let mut previous = None;
        let mut points = 0;
        for glyph in &self.glyphs {
            validate_label(glyph.label)?;
            if previous.is_some_and(|label| label >= glyph.label) {
                return Err("Profile labels must be unique and sorted".into());
            }
            previous = Some(glyph.label);
            if glyph.variants.is_empty() || glyph.variants.len() > MAX_VARIANTS {
                return Err("Each character needs 1..3 sample variants".into());
            }
            for sample in &glyph.variants {
                points += validate_sample(sample)?;
                if points > MAX_PROFILE_POINTS {
                    return Err("Profile exceeds 65536 points".into());
                }
            }
        }
        Ok(points)
    }

    fn check_chars(&self, text: &str, missing: &mut BTreeSet<char>) -> Result<(), String> {
        validate_text(text)?;
        for ch in text.chars().filter(|ch| !ch.is_whitespace()) {
            if self.glyph(ch).is_none() {
                missing.insert(ch);
            }
        }
        Ok(())
    }

    fn check_layout_chars(
        &self,
        layout: &MathLayout,
        missing: &mut BTreeSet<char>,
    ) -> Result<(), String> {
        match layout {
            MathLayout::Text(text) => self.check_chars(text, missing)?,
            MathLayout::Row(children) => {
                for child in children {
                    self.check_layout_chars(child, missing)?;
                }
            }
            MathLayout::Fraction(a, b) => {
                self.check_layout_chars(a, missing)?;
                self.check_layout_chars(b, missing)?;
            }
            MathLayout::Radical(child) => self.check_layout_chars(child, missing)?,
        }
        Ok(())
    }

    fn median_width(&self) -> f32 {
        let mut widths: Vec<f32> = self
            .glyphs
            .iter()
            .flat_map(|g| &g.variants)
            .flatten()
            .map(|s| s.style.width)
            .collect();
        if widths.is_empty() {
            return Style::default().width;
        }
        widths.sort_by(f32::total_cmp);
        let middle = widths.len() / 2;
        if widths.len() % 2 == 0 {
            (widths[middle - 1] + widths[middle]) / 2.0
        } else {
            widths[middle]
        }
    }
}

fn validate_label(label: char) -> Result<(), String> {
    if label.is_whitespace() || label.is_control() {
        Err("Sample label must be one non-whitespace, non-control character".into())
    } else {
        Ok(())
    }
}

fn validate_text(text: &str) -> Result<(), String> {
    if text.len() > MAX_HANDWRITING_TEXT_BYTES
        || text.chars().take(MAX_INPUT_CHARS + 1).count() > MAX_INPUT_CHARS
    {
        return Err("Handwriting input text exceeds its budget".into());
    }
    if text
        .chars()
        .any(|ch| ch.is_control() && !ch.is_whitespace())
    {
        return Err("Handwriting input contains unsupported control characters".into());
    }
    Ok(())
}

fn count_chars(text: &str, counts: &mut BTreeMap<char, usize>) -> Result<(), String> {
    validate_text(text)?;
    for ch in text.chars().filter(|ch| !ch.is_whitespace()) {
        *counts.entry(ch).or_default() += 1;
    }
    Ok(())
}

// Called only after core layout validation has bounded recursion and leaf bytes.
fn count_layout(
    layout: &MathLayout,
    counts: &mut BTreeMap<char, usize>,
    strokes: &mut usize,
    points: &mut usize,
) -> Result<(), String> {
    match layout {
        MathLayout::Text(text) => count_chars(text, counts)?,
        MathLayout::Row(children) => {
            for child in children {
                count_layout(child, counts, strokes, points)?;
            }
        }
        MathLayout::Fraction(a, b) => {
            reserve(strokes, points, 1, 2)?;
            count_layout(a, counts, strokes, points)?;
            count_layout(b, counts, strokes, points)?;
        }
        MathLayout::Radical(child) => {
            reserve(strokes, points, 1, 5)?;
            count_layout(child, counts, strokes, points)?;
        }
    }
    Ok(())
}

fn validate_sample(strokes: &[HandwritingStroke]) -> Result<usize, String> {
    if strokes.is_empty() || strokes.len() > MAX_SAMPLE_STROKES {
        return Err("A sample needs 1..16 strokes".into());
    }
    let mut count = 0;
    for stroke in strokes {
        count += stroke.points.len();
        if stroke.points.is_empty() || count > MAX_SAMPLE_POINTS {
            return Err("A sample needs nonempty strokes and at most 2048 points".into());
        }
        if !(MIN_BRUSH_WIDTH..=MAX_BRUSH_WIDTH).contains(&stroke.style.width) {
            return Err("Invalid sample pen width".into());
        }
        let mut previous = 0.0;
        for point in &stroke.points {
            if !(0.0..=128.0).contains(&point.x)
                || !(0.0..=128.0).contains(&point.y)
                || !(0.0..=1.0).contains(&point.pressure)
                || !(previous..=MAX_HANDWRITING_TIME).contains(&point.time)
            {
                return Err("Invalid sample coordinates, pressure or non-monotonic time".into());
            }
            previous = point.time;
        }
    }
    Ok(count)
}

struct LimitedJson(Vec<u8>);
impl Write for LimitedJson {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_JSON_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("Profile JSON exceeds 8 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn ink_extent(width: f32, scale: f32) -> f32 {
    (width * scale).clamp(MIN_BRUSH_WIDTH, MAX_BRUSH_WIDTH) * std::f32::consts::FRAC_1_SQRT_2
}

fn valid_coord(value: f32) -> bool {
    value.is_finite() && value.abs() <= MAX_HANDWRITING_COORD
}

fn reserve(
    strokes: &mut usize,
    points: &mut usize,
    more_strokes: usize,
    more_points: usize,
) -> Result<(), String> {
    if more_strokes > MAX_HANDWRITING_STROKES.saturating_sub(*strokes)
        || more_points > MAX_HANDWRITING_POINTS.saturating_sub(*points)
    {
        return Err("Generated handwriting exceeds stroke/point budget".into());
    }
    *strokes += more_strokes;
    *points += more_points;
    Ok(())
}

struct PlacedGlyph<'a> {
    sample: &'a [HandwritingStroke],
    offset: Point,
    left: f32,
}

struct InkBox<'a> {
    width: f32,
    height: f32,
    baseline: f32,
    scale: f32,
    glyphs: Vec<PlacedGlyph<'a>>,
    children: Vec<(Point, InkBox<'a>)>,
    structure: Vec<Point>,
    pen_width: f32,
}

struct Planner<'a> {
    profile: &'a Profile,
    occurrences: BTreeMap<char, usize>,
    strokes: usize,
    points: usize,
    pen_width: f32,
}

impl<'a> Planner<'a> {
    fn empty(&self, size: f32) -> InkBox<'a> {
        let scale = size / CAP_HEIGHT;
        InkBox {
            width: 0.0,
            height: 128.0 * scale,
            baseline: 96.0 * scale,
            scale,
            glyphs: Vec::new(),
            children: Vec::new(),
            structure: Vec::new(),
            pen_width: self.pen_width,
        }
    }

    fn text(&mut self, text: &str, size: f32) -> Result<InkBox<'a>, String> {
        let mut result = self.empty(size);
        let mut x = 0.0_f32;
        let mut y = 0.0;
        let mut line_start = 0;
        let mut top = 0.0_f32;
        let mut bottom = result.height;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                result.width = result.width.max(x);
                x = 0.0;
                result.finish_line(line_start, y, top, bottom);
                y = result.height + 16.0 * result.scale;
                line_start = result.glyphs.len();
                top = 0.0;
                bottom = 128.0 * result.scale;
            } else if ch.is_whitespace() {
                x += if ch == '\t' { 144.0 } else { 36.0 } * result.scale;
            } else {
                let glyph = self
                    .profile
                    .glyph(ch)
                    .ok_or_else(|| format!("Missing handwriting sample: {ch}"))?;
                let occurrence = self.occurrences.entry(ch).or_default();
                let sample = &glyph.variants[*occurrence % glyph.variants.len()];
                *occurrence += 1;
                reserve(
                    &mut self.strokes,
                    &mut self.points,
                    sample.len(),
                    sample.iter().map(|s| s.points.len()).sum(),
                )?;
                let (mut left, mut right) = (f32::INFINITY, f32::NEG_INFINITY);
                for stroke in sample {
                    // Use emitted width, not raw sample width: clamping matters
                    // especially at small sizes. Renderer joins reach width/sqrt(2),
                    // not width/2. This bounds x AND y, pressure/speed and round caps.
                    let radius = ink_extent(stroke.style.width, result.scale);
                    for point in &stroke.points {
                        let px = point.x * result.scale;
                        let py = point.y * result.scale;
                        left = left.min(px - radius);
                        right = right.max(px + radius);
                        top = top.min(py - radius);
                        bottom = bottom.max(py + radius);
                    }
                }
                result.glyphs.push(PlacedGlyph {
                    sample,
                    offset: Point { x, y },
                    left,
                });
                x += (right - left).max(18.0 * result.scale) + 12.0 * result.scale;
            }
            if !valid_coord(x) || !valid_coord(y) {
                return Err("Handwriting layout exceeds coordinate budget".into());
            }
        }
        result.width = result.width.max(x);
        result.finish_line(line_start, y, top, bottom);
        result.check()?;
        Ok(result)
    }

    fn layout(&mut self, layout: &MathLayout, size: f32) -> Result<InkBox<'a>, String> {
        let mut result = self.empty(size);
        match layout {
            MathLayout::Text(text) => return self.text(text, size),
            MathLayout::Row(children) => {
                let mut descent = result.height - result.baseline;
                for child in children {
                    let child = self.layout(child, size)?;
                    result.baseline = result.baseline.max(child.baseline);
                    descent = descent.max(child.height - child.baseline);
                    result.children.push((Point::default(), child));
                }
                result.height = result.baseline + descent;
                for (offset, child) in &mut result.children {
                    *offset = Point {
                        x: result.width,
                        y: result.baseline - child.baseline,
                    };
                    result.width += child.width;
                }
            }
            MathLayout::Fraction(a, b) => {
                let a = self.layout(a, size * 0.9)?;
                let b = self.layout(b, size * 0.9)?;
                let gap = size * 0.15;
                let radius = ink_extent(result.pen_width, result.scale);
                result.width = a.width.max(b.width) + size * 0.4 + 2.0 * radius;
                let bar_y = a.height + gap + radius;
                let denominator_y = bar_y + radius + gap;
                result.height = denominator_y + b.height;
                result.baseline = bar_y + size * MATH_AXIS;
                reserve(&mut self.strokes, &mut self.points, 1, 2)?;
                result.structure = vec![
                    Point {
                        x: radius,
                        y: bar_y,
                    },
                    Point {
                        x: result.width - radius,
                        y: bar_y,
                    },
                ];
                result.children.push((
                    Point {
                        x: (result.width - a.width) / 2.0,
                        y: 0.0,
                    },
                    a,
                ));
                result.children.push((
                    Point {
                        x: (result.width - b.width) / 2.0,
                        y: denominator_y,
                    },
                    b,
                ));
            }
            MathLayout::Radical(child) => {
                let child = self.layout(child, size)?;
                let radius = ink_extent(result.pen_width, result.scale);
                let padding = size * 0.18 + 2.0 * radius;
                let left = size * 0.65 + 2.0 * radius;
                result.width = left + child.width + padding;
                result.height = child.height + padding;
                result.baseline = child.baseline + padding;
                reserve(&mut self.strokes, &mut self.points, 1, 5)?;
                result.structure = vec![
                    Point {
                        x: radius,
                        y: (result.height * 0.55).clamp(radius, result.height - radius),
                    },
                    Point {
                        x: size * 0.18 + radius,
                        y: (result.height * 0.48).clamp(radius, result.height - radius),
                    },
                    Point {
                        x: size * 0.35 + radius,
                        y: result.height - radius,
                    },
                    Point {
                        x: left - size * 0.05,
                        y: radius,
                    },
                    Point {
                        x: result.width - radius,
                        y: radius,
                    },
                ];
                result.children.push((
                    Point {
                        x: left,
                        y: padding,
                    },
                    child,
                ));
            }
        }
        result.check()?;
        Ok(result)
    }
}

impl InkBox<'_> {
    fn finish_line(&mut self, start: usize, y: f32, top: f32, bottom: f32) {
        for glyph in &mut self.glyphs[start..] {
            glyph.offset.y -= top;
        }
        if y == 0.0 {
            self.baseline -= top;
        }
        self.height = y + bottom - top;
    }

    fn check(&self) -> Result<(), String> {
        if !valid_coord(self.width)
            || !valid_coord(self.height)
            || !valid_coord(self.baseline)
            || !self.scale.is_finite()
            || self.scale <= 0.0
        {
            Err("Handwriting layout exceeds coordinate budget".into())
        } else {
            Ok(())
        }
    }

    fn emit(&self, offset: Point, output: &mut Output) -> Result<(), String> {
        for glyph in &self.glyphs {
            for stroke in glyph.sample {
                output.begin(stroke.points.len())?;
                let mut points = Vec::with_capacity(stroke.points.len());
                // Each independent pen-down sequence starts at zero. Scaling time
                // and distance together retains speed-dependent width modulation.
                let start = stroke.points[0].time;
                for p in &stroke.points {
                    let point = StrokePoint {
                        x: offset.x + glyph.offset.x + p.x * self.scale - glyph.left,
                        y: offset.y + glyph.offset.y + p.y * self.scale,
                        time: (p.time - start) * f64::from(self.scale),
                        pressure: p.pressure,
                    };
                    output.check_point(&point)?;
                    points.push(point);
                }
                output.strokes.push(HandwritingStroke {
                    points,
                    style: Style {
                        color: output.color,
                        width: (stroke.style.width * self.scale)
                            .clamp(MIN_BRUSH_WIDTH, MAX_BRUSH_WIDTH),
                        dashed: stroke.style.dashed,
                    },
                });
            }
        }
        for (child_offset, child) in &self.children {
            child.emit(
                Point {
                    x: offset.x + child_offset.x,
                    y: offset.y + child_offset.y,
                },
                output,
            )?;
        }
        if !self.structure.is_empty() {
            output.begin(self.structure.len())?;
            let mut points: Vec<StrokePoint> = Vec::with_capacity(self.structure.len());
            let mut time = 0.0;
            for p in &self.structure {
                let mut point = StrokePoint {
                    x: p.x + offset.x,
                    y: p.y + offset.y,
                    pressure: 1.0,
                    time,
                };
                if let Some(previous) = points.last() {
                    time += f64::from(point.x - previous.x).hypot(f64::from(point.y - previous.y))
                        / 600.0;
                    point.time = time;
                }
                output.check_point(&point)?;
                points.push(point);
            }
            output.strokes.push(HandwritingStroke {
                points,
                style: Style {
                    color: output.color,
                    width: (self.pen_width * self.scale).clamp(MIN_BRUSH_WIDTH, MAX_BRUSH_WIDTH),
                    dashed: false,
                },
            });
        }
        Ok(())
    }
}

struct Output {
    position: Point,
    color: Color,
    strokes: Vec<HandwritingStroke>,
    points: usize,
}
impl Output {
    fn begin(&mut self, points: usize) -> Result<(), String> {
        let mut stroke_count = self.strokes.len();
        reserve(&mut stroke_count, &mut self.points, 1, points)
    }

    fn check_point(&self, point: &StrokePoint) -> Result<(), String> {
        if !valid_coord(point.x)
            || !valid_coord(point.y)
            || !valid_coord(self.position.x + point.x)
            || !valid_coord(self.position.y + point.y)
            || !(0.0..=MAX_HANDWRITING_TIME).contains(&point.time)
        {
            Err("Generated handwriting exceeds coordinate/time budget".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "handwriting_tests.rs"]
mod tests;
