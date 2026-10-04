//! 本地手写识别：可训练的几何模板，以及需独立安装权重的 TexTeller CPU 后端。
//! 下述能力与 confidence 说明仅针对几何模板；神经后端返回原始 LaTeX 和平均对数概率。
//! Best with separated, upright symbols and familiar stroke counts. No rotation invariance,
//! cursive segmentation, matrices or general two-dimensional parsing is claimed.
//! Separated print letters compose sin/cos/tan/log/ln; pi is a printed π template.
//! Simple roofed square roots and clearly separated equation rows are supported.
//! Whole function tokens (including sqrt) can also be learned in up to eight strokes.
//! Confidence is a geometric heuristic, not a calibrated probability. Always ask the user
//! to confirm a candidate; this crate never edits a document or executes recognized text.

mod gate;
mod latex;
mod neural;
mod templates;

pub use gate::{AutoCalculate, CalculationRequest, ContextToken};
pub use latex::{LatexCalculation, latex_to_calculation, latex_to_expression};
pub use neural::{NeuralFormula, NeuralInputPreview, NeuralRecognizer};
pub use templates::InkTemplate;

use board_core::StrokePoint;
use serde::{Deserialize, Serialize};
use std::{fmt, fs, io::Read, path::Path};

const SAMPLES: usize = 32;
const MAX_STROKES: usize = 128;
const MAX_POINTS: usize = 32_768;
const MAX_TEMPLATES: usize = 256;
const MAX_JSON: usize = 4 * 1024 * 1024;
const MAX_CANDIDATES: usize = 5;
const MAX_SYMBOL_STROKES: usize = 8;
const ACCEPT_SCORE: f32 = 0.58;

#[derive(Debug)]
pub enum Error {
    EmptyInput,
    InvalidInput(&'static str),
    Rejected,
    InvalidTemplate(&'static str),
    UnsupportedVersion(u32),
    Io(std::io::Error),
    Json(serde_json::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyInput => write!(f, "没有可识别笔画"),
            Self::InvalidInput(s) => write!(f, "无效笔画：{s}"),
            Self::Rejected => write!(f, "笔画不匹配离线模板，请修改或登记模板"),
            Self::InvalidTemplate(s) => write!(f, "无效模板：{s}"),
            Self::UnsupportedVersion(v) => write!(f, "不支持模板版本 {v}"),
            Self::Io(e) => e.fmt(f),
            Self::Json(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recognition {
    pub text: String,
    pub confidence: f32,
    pub candidates: Vec<Candidate>,
    /// Always true for this template backend, including exact matches.
    pub requires_confirmation: bool,
    pub backend: String,
}

/// Recognize with the built-in offline templates. Reuse InkRecognizer for learned styles.
pub fn recognize(strokes: &[Vec<StrokePoint>]) -> Result<Recognition> {
    InkRecognizer::default().recognize(strokes)
}

#[derive(Debug, Clone)]
pub struct InkRecognizer {
    templates: Vec<InkTemplate>,
    features: Vec<Feature>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateFile {
    version: u32,
    templates: Vec<InkTemplate>,
}

impl Default for InkRecognizer {
    fn default() -> Self {
        let templates = templates::builtins();
        let features = templates.iter().map(|t| Feature::new(&t.strokes)).collect();
        Self {
            templates,
            features,
        }
    }
}

impl InkRecognizer {
    /// A recognizer with no built-ins, useful for a strictly personal symbol vocabulary.
    pub fn empty() -> Self {
        Self {
            templates: Vec::new(),
            features: Vec::new(),
        }
    }

    pub fn templates(&self) -> &[InkTemplate] {
        &self.templates
    }

    /// Add another example (not a replacement). Supports 1..=8 strokes per symbol/token.
    /// Learned multi-letter tokens are matched across adjacent separated glyph groups.
    pub fn register_template(&mut self, text: &str, strokes: &[Vec<StrokePoint>]) -> Result<()> {
        if self.templates.len() >= MAX_TEMPLATES {
            return Err(Error::InvalidTemplate("最多 256 个模板"));
        }
        validate_template(text, strokes)?;
        self.features.push(Feature::new(strokes));
        self.templates.push(InkTemplate {
            text: text.into(),
            strokes: strokes.to_vec(),
        });
        Ok(())
    }

    /// Export the complete vocabulary, including built-ins and learned examples.
    pub fn to_json(&self) -> Result<String> {
        let json = serde_json::to_string_pretty(&TemplateFile {
            version: 1,
            templates: self.templates.clone(),
        })?;
        if json.len() > MAX_JSON {
            return Err(Error::InvalidTemplate("模板文件超过 4 MiB"));
        }
        Ok(json)
    }

    /// Import a complete vocabulary; no platform service, network or model download.
    pub fn from_json(json: &str) -> Result<Self> {
        if json.len() > MAX_JSON {
            return Err(Error::InvalidTemplate("模板文件超过 4 MiB"));
        }
        let file: TemplateFile = serde_json::from_str(json)?;
        if file.version != 1 {
            return Err(Error::UnsupportedVersion(file.version));
        }
        let mut recognizer = Self::empty();
        for template in file.templates {
            recognizer.register_template(&template.text, &template.strokes)?;
        }
        Ok(recognizer)
    }

    /// Validate first, then replace the entire vocabulary atomically in memory.
    pub fn import_json(&mut self, json: &str) -> Result<()> {
        *self = Self::from_json(json)?;
        Ok(())
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        fs::write(path, self.to_json()?)?;
        Ok(())
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let mut json = String::new();
        fs::File::open(path)?
            .take(MAX_JSON as u64 + 1)
            .read_to_string(&mut json)?;
        Self::from_json(&json)
    }

    /// Match a single symbol, ignoring stroke order and stroke direction, but not count.
    /// An empty candidate list means rejection. Raw similarity must reach 0.58;
    /// ambiguity penalties can lower the final displayed confidence further.
    pub fn recognize_candidates(&self, strokes: &[Vec<StrokePoint>]) -> Result<Vec<Candidate>> {
        validate_strokes(strokes)?;
        Ok(self.symbol_candidates(strokes))
    }

    pub fn recognize(&self, strokes: &[Vec<StrokePoint>]) -> Result<Recognition> {
        validate_strokes(strokes)?;
        let candidates = if let Some(rows) = self.equation_rows(strokes)? {
            rows
        } else {
            self.layout(strokes, 0)?
        };
        let best = candidates.first().ok_or(Error::Rejected)?;
        Ok(Recognition {
            text: best.text.clone(),
            confidence: best.confidence,
            candidates,
            requires_confirmation: true,
            backend: "offline-multistroke-template-v1".into(),
        })
    }

    fn symbol_candidates(&self, strokes: &[Vec<StrokePoint>]) -> Vec<Candidate> {
        if strokes.len() > MAX_SYMBOL_STROKES {
            return Vec::new();
        }
        let input = Feature::new(strokes);
        let mut candidates: Vec<Candidate> = Vec::new();
        for (template, feature) in self.templates.iter().zip(&self.features) {
            if input.lines.len() != feature.lines.len() {
                continue;
            }
            let score = (1.0 - input.distance(feature) / 0.28).clamp(0.0, 1.0);
            if score < ACCEPT_SCORE {
                continue;
            }
            if let Some(old) = candidates.iter_mut().find(|c| c.text == template.text) {
                old.confidence = old.confidence.max(score);
            } else {
                candidates.push(Candidate {
                    text: template.text.clone(),
                    confidence: score,
                });
            }
        }
        candidates.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
        candidates.truncate(MAX_CANDIDATES);
        // Exact x/× (or learned duplicate shapes) must not report near-certain confidence.
        if candidates.len() > 1 {
            let gap = candidates[0].confidence - candidates[1].confidence;
            let penalty = (0.12 - gap).max(0.0) * 2.0;
            for candidate in &mut candidates {
                candidate.confidence *= 1.0 - penalty;
            }
        }
        candidates
    }

    fn equation_rows(&self, strokes: &[Vec<StrokePoint>]) -> Result<Option<Vec<Candidate>>> {
        let mut heights: Vec<_> = strokes
            .iter()
            .map(|s| Bounds::line(s).height())
            .filter(|h| *h > 0.0)
            .collect();
        if heights.is_empty() {
            return Ok(None);
        }
        heights.sort_by(f32::total_cmp);
        let gap = heights[heights.len() / 2] * 0.6;
        let mut sorted = strokes.to_vec();
        sorted.sort_by(|a, b| Bounds::line(a).top.total_cmp(&Bounds::line(b).top));
        let mut rows: Vec<Vec<Vec<StrokePoint>>> = Vec::new();
        let mut bottom = f32::NEG_INFINITY;
        for stroke in sorted {
            let bounds = Bounds::line(&stroke);
            if rows.is_empty() || bounds.top > bottom + gap {
                rows.push(Vec::new());
                bottom = bounds.bottom;
            } else {
                bottom = bottom.max(bounds.bottom);
            }
            rows.last_mut().unwrap().push(stroke);
        }
        if rows.len() < 2 {
            return Ok(None);
        }
        let mut equations = Vec::new();
        for row in rows {
            let mut candidates = self.layout(&row, 0).unwrap_or_default();
            candidates.retain(|c| {
                let parts: Vec<_> = c.text.split('=').collect();
                parts.len() == 2 && parts.iter().all(|s| !s.is_empty() && !s.contains(';'))
            });
            equations.push(candidates);
        }
        if equations.iter().all(Vec::is_empty) {
            return Ok(None);
        }
        // Once an equation row is detected, do not silently concatenate an unknown
        // second row into its right-hand side or discard it as stray ink.
        if equations.iter().any(Vec::is_empty) {
            return Err(Error::Rejected);
        }
        let mut result = equations.remove(0);
        for candidates in equations {
            result = combine(&result, &candidates, ";", "", 0.8);
        }
        Ok(Some(result))
    }

    fn layout(&self, strokes: &[Vec<StrokePoint>], depth: usize) -> Result<Vec<Candidate>> {
        if depth > 4 {
            return Err(Error::Rejected);
        }
        let mut sorted = strokes.to_vec();
        sorted.sort_by(|a, b| Bounds::line(a).left.total_cmp(&Bounds::line(b).left));
        let mut groups: Vec<Vec<Vec<StrokePoint>>> = Vec::new();
        let mut right = f32::NEG_INFINITY;
        for stroke in sorted {
            let bounds = Bounds::line(&stroke);
            if groups.is_empty() || bounds.left > right + 0.001 {
                right = bounds.right;
                groups.push(vec![stroke]);
            } else {
                right = right.max(bounds.right);
                groups.last_mut().unwrap().push(stroke);
            }
        }
        // A learned word may contain disjoint letters. Only explicit function-token
        // labels and strong geometric matches may bridge projection groups.
        let mut merged = Vec::new();
        let mut start = 0;
        while start < groups.len() {
            let mut span = Vec::new();
            let mut best = None;
            for (end, group) in groups.iter().enumerate().skip(start) {
                span.extend(group.iter().cloned());
                if span.len() > MAX_SYMBOL_STROKES {
                    break;
                }
                if end > start {
                    let candidates: Vec<_> = self
                        .symbol_candidates(&span)
                        .into_iter()
                        .filter(|c| {
                            matches!(
                                c.text.as_str(),
                                "sin" | "cos" | "tan" | "log" | "ln" | "sqrt"
                            ) && c.confidence >= 0.85
                        })
                        .collect();
                    if !candidates.is_empty() {
                        best = Some((end, span.clone(), candidates));
                    }
                }
            }
            if let Some((end, span, candidates)) = best {
                merged.push((span, Some(candidates)));
                start = end + 1;
            } else {
                merged.push((groups[start].clone(), None));
                start += 1;
            }
        }
        let mut glyphs = Vec::new();
        for (group, learned) in merged {
            let bounds = Bounds::new(&group);
            let candidates = if let Some(candidates) = learned {
                candidates
            } else if let Some(radical) = self.radical(&group, depth)? {
                radical
            } else if let Some(fraction) = self.fraction(&group, depth)? {
                fraction
            } else {
                let matches = self.symbol_candidates(&group);
                if matches.is_empty() {
                    return Err(Error::Rejected);
                }
                matches
            };
            glyphs.push((bounds, candidates));
        }
        let mut result = vec![Candidate {
            text: String::new(),
            confidence: 1.0,
        }];
        let mut base: Option<Bounds> = None;
        let mut exponent = false;
        for (bounds, candidates) in glyphs {
            let raised = base.is_some_and(|b| {
                bounds.height() > 0.0
                    && bounds.height() < b.height() * 0.72
                    && bounds.bottom < b.top + b.height() * 0.55
                    && bounds.left - b.right < b.height() * 0.9
            });
            let prefix = if raised && !exponent {
                "^("
            } else if !raised && exponent {
                ")"
            } else {
                ""
            };
            result = combine(
                &result,
                &candidates,
                prefix,
                "",
                if raised { 0.82 } else { 1.0 },
            );
            if !raised {
                base = Some(bounds);
            }
            exponent = raised;
        }
        if exponent {
            for candidate in &mut result {
                candidate.text.push(')');
            }
        }
        Ok(result)
    }

    fn radical(
        &self,
        strokes: &[Vec<StrokePoint>],
        depth: usize,
    ) -> Result<Option<Vec<Candidate>>> {
        for (index, stroke) in strokes.iter().enumerate() {
            let b = Bounds::line(stroke);
            let h = b.height();
            if h <= 0.0 || b.width() < h * 1.2 {
                continue;
            }
            let mut directed = stroke.clone();
            if directed.first().unwrap().x > directed.last().unwrap().x {
                directed.reverse();
            }
            let Some(roof) = directed
                .iter()
                .find(|p| p.y < b.top + h * 0.1 && p.x > b.left + h * 0.35)
            else {
                continue;
            };
            if roof.x > b.left + h || b.right - roof.x < h * 0.5 {
                continue;
            }
            let reference = vec![vec![
                StrokePoint {
                    x: b.left,
                    y: b.top + h * 0.6,
                    time: 0.0,
                    pressure: 0.5,
                },
                StrokePoint {
                    x: b.left + h * 0.22,
                    y: b.bottom,
                    time: 0.0,
                    pressure: 0.5,
                },
                StrokePoint {
                    x: roof.x,
                    y: b.top,
                    time: 0.0,
                    pressure: 0.5,
                },
                StrokePoint {
                    x: b.right,
                    y: b.top,
                    time: 0.0,
                    pressure: 0.5,
                },
            ]];
            let score = 1.0
                - Feature::new(std::slice::from_ref(stroke)).distance(&Feature::new(&reference))
                    / 0.28;
            // Normalization by a long roof must not hide a malformed radical hook.
            let hook: Vec<_> = directed
                .iter()
                .copied()
                .take_while(|p| p.x <= roof.x)
                .collect();
            let hook_score = 1.0
                - Feature::new(&[hook]).distance(&Feature::new(&[reference[0][..3].to_vec()]))
                    / 0.28;
            if score < 0.85 || hook_score < 0.75 {
                continue;
            }
            let body: Vec<_> = strokes
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .map(|(_, s)| s.clone())
                .collect();
            if body.is_empty()
                || body.iter().any(|s| {
                    let a = Bounds::line(s);
                    a.left < roof.x
                        || a.right > b.right + h * 0.05
                        || a.top <= b.top + h * 0.08
                        || a.bottom > b.bottom + h * 0.15
                })
            {
                return Err(Error::Rejected);
            }
            let mut candidates = self.layout(&body, depth + 1)?;
            for candidate in &mut candidates {
                candidate.text = format!("sqrt({})", candidate.text);
                candidate.confidence = candidate.confidence.min(score).min(hook_score).min(0.8);
            }
            return Ok(Some(candidates));
        }
        Ok(None)
    }

    fn fraction(
        &self,
        strokes: &[Vec<StrokePoint>],
        depth: usize,
    ) -> Result<Option<Vec<Candidate>>> {
        for (index, bar) in strokes.iter().enumerate() {
            let b = Bounds::line(bar);
            if b.width() <= 0.0 || b.height() > b.width() * 0.06 {
                continue;
            }
            // Reject a backtracking scribble that merely has a thin bounding box.
            if path_length(bar) > b.width() as f64 * 1.15 {
                continue;
            }
            let mut above = Vec::new();
            let mut below = Vec::new();
            let mut valid = true;
            for (other, line) in strokes.iter().enumerate() {
                if other == index {
                    continue;
                }
                let a = Bounds::line(line);
                if a.left < b.left - b.width() * 0.1 || a.right > b.right + b.width() * 0.1 {
                    valid = false;
                    break;
                }
                if a.bottom < b.top - b.width() * 0.03 {
                    above.push(line.clone());
                } else if a.top > b.bottom + b.width() * 0.03 {
                    below.push(line.clone());
                } else {
                    valid = false;
                    break;
                }
            }
            if !valid || above.is_empty() || below.is_empty() {
                continue;
            }
            // A division sign has dots, not a numerator and denominator.
            if Bounds::new(&above).height() < b.width() * 0.1
                || Bounds::new(&below).height() < b.width() * 0.1
            {
                continue;
            }
            let numerator = self.layout(&above, depth + 1)?;
            let denominator = self.layout(&below, depth + 1)?;
            let numerator: Vec<_> = numerator
                .into_iter()
                .map(|mut c| {
                    c.text = format!("({})", c.text);
                    c
                })
                .collect();
            return Ok(Some(combine(&numerator, &denominator, "/(", ")", 0.82)));
        }
        Ok(None)
    }
}

fn combine(
    left: &[Candidate],
    right: &[Candidate],
    prefix: &str,
    suffix: &str,
    cap: f32,
) -> Vec<Candidate> {
    let mut result = Vec::new();
    for a in left {
        for b in right {
            result.push(Candidate {
                text: format!("{}{prefix}{}{suffix}", a.text, b.text),
                confidence: a.confidence.min(b.confidence).min(cap),
            });
        }
    }
    result.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    result.dedup_by(|a, b| a.text == b.text);
    result.truncate(MAX_CANDIDATES);
    result
}

fn validate_strokes(strokes: &[Vec<StrokePoint>]) -> Result<()> {
    if strokes.is_empty() || strokes.iter().all(Vec::is_empty) {
        return Err(Error::EmptyInput);
    }
    if strokes.len() > MAX_STROKES || strokes.iter().map(Vec::len).sum::<usize>() > MAX_POINTS {
        return Err(Error::InvalidInput("笔画或采样点过多"));
    }
    if strokes.iter().any(Vec::is_empty) {
        return Err(Error::InvalidInput("包含空笔画"));
    }
    if strokes.iter().flatten().any(|p| {
        !p.x.is_finite()
            || !p.y.is_finite()
            || p.x.abs() > 1.0e7
            || p.y.abs() > 1.0e7
            || !p.time.is_finite()
            || p.time < 0.0
            || !(0.0..=1.0).contains(&p.pressure)
    }) {
        return Err(Error::InvalidInput("坐标、时间或压力无效"));
    }
    Ok(())
}

fn validate_template(text: &str, strokes: &[Vec<StrokePoint>]) -> Result<()> {
    if text.is_empty()
        || text.chars().count() > 16
        || text.chars().any(char::is_whitespace)
        || text.chars().any(char::is_control)
    {
        return Err(Error::InvalidTemplate(
            "标签必须是 1 至 16 个非空白可打印字符",
        ));
    }
    validate_strokes(strokes)?;
    if strokes.len() > MAX_SYMBOL_STROKES || strokes.iter().map(Vec::len).sum::<usize>() > 4096 {
        return Err(Error::InvalidTemplate(
            "每个符号或 token 最多 8 笔、4096 点",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct Bounds {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}
impl Bounds {
    fn new(strokes: &[Vec<StrokePoint>]) -> Self {
        Self::points(strokes.iter().flatten())
    }
    fn line(line: &[StrokePoint]) -> Self {
        Self::points(line.iter())
    }
    fn points<'a>(points: impl Iterator<Item = &'a StrokePoint>) -> Self {
        let mut b = Self {
            left: f32::INFINITY,
            right: f32::NEG_INFINITY,
            top: f32::INFINITY,
            bottom: f32::NEG_INFINITY,
        };
        for p in points {
            b.left = b.left.min(p.x);
            b.right = b.right.max(p.x);
            b.top = b.top.min(p.y);
            b.bottom = b.bottom.max(p.y);
        }
        b
    }
    fn width(self) -> f32 {
        self.right - self.left
    }
    fn height(self) -> f32 {
        self.bottom - self.top
    }
}

#[derive(Debug, Clone)]
struct Feature {
    lines: Vec<Vec<[f32; 2]>>,
    lengths: Vec<f32>,
}
fn path_length(line: &[StrokePoint]) -> f64 {
    line.windows(2)
        .map(|p| (p[1].x as f64 - p[0].x as f64).hypot(p[1].y as f64 - p[0].y as f64))
        .sum()
}
impl Feature {
    fn new(strokes: &[Vec<StrokePoint>]) -> Self {
        let bounds = Bounds::new(strokes);
        let scale = bounds.width().max(bounds.height()).max(0.0001) as f64;
        let mut lines = Vec::new();
        let mut lengths = Vec::new();
        for stroke in strokes {
            let length = path_length(stroke);
            lengths.push((length / scale) as f32);
            let mut cumulative = vec![0.0];
            for pair in stroke.windows(2) {
                cumulative.push(
                    cumulative.last().unwrap()
                        + (pair[1].x as f64 - pair[0].x as f64)
                            .hypot(pair[1].y as f64 - pair[0].y as f64),
                );
            }
            let mut sampled = Vec::with_capacity(SAMPLES);
            let mut segment = 1;
            for sample in 0..SAMPLES {
                let target = length * sample as f64 / (SAMPLES - 1) as f64;
                while segment < cumulative.len() && cumulative[segment] < target {
                    segment += 1;
                }
                let (x, y) = if segment >= stroke.len() || length < 1.0e-9 {
                    (stroke[0].x as f64, stroke[0].y as f64)
                } else {
                    let span = cumulative[segment] - cumulative[segment - 1];
                    let t = if span > 0.0 {
                        (target - cumulative[segment - 1]) / span
                    } else {
                        0.0
                    };
                    let a = stroke[segment - 1];
                    let b = stroke[segment];
                    (
                        a.x as f64 + (b.x as f64 - a.x as f64) * t,
                        a.y as f64 + (b.y as f64 - a.y as f64) * t,
                    )
                };
                sampled.push([
                    ((x - bounds.left as f64) / scale) as f32,
                    ((y - bounds.top as f64) / scale) as f32,
                ]);
            }
            lines.push(sampled);
        }
        Self { lines, lengths }
    }

    fn distance(&self, other: &Self) -> f32 {
        let n = self.lines.len();
        let mut cost = vec![vec![0.0; n]; n];
        for (i, row) in cost.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                let a = &self.lines[i];
                let b = &other.lines[j];
                let forward: f32 = a
                    .iter()
                    .zip(b)
                    .map(|(p, q)| (p[0] - q[0]).hypot(p[1] - q[1]))
                    .sum();
                let reverse: f32 = a
                    .iter()
                    .zip(b.iter().rev())
                    .map(|(p, q)| (p[0] - q[0]).hypot(p[1] - q[1]))
                    .sum();
                *value = forward.min(reverse) / SAMPLES as f32
                    + (self.lengths[i] - other.lengths[j]).abs() * 0.12;
            }
        }
        // Exact minimum assignment for at most eight strokes; bounded to 256 states.
        let mut dp = vec![f32::INFINITY; 1 << n];
        dp[0] = 0.0;
        for mask in 0usize..(1 << n) - 1 {
            let i = mask.count_ones() as usize;
            for (j, distance) in cost[i].iter().enumerate() {
                if mask & (1 << j) == 0 {
                    let next = mask | (1 << j);
                    dp[next] = dp[next].min(dp[mask] + distance);
                }
            }
        }
        dp[(1 << n) - 1] / n as f32
    }
}

#[cfg(test)]
mod tests;
