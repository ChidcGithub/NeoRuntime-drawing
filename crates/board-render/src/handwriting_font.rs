//! Bounded font-derived centerlines, not glyph outlines or learned pen trajectories.
//! Skeleton junctions and stroke order only approximate the printed glyph's topology.

use crate::{RenderError, Result};
use ab_glyph::{Font, FontArc, ScaleFont};
use board_core::{HandwritingStroke, StrokePoint, Style};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

const MAX_CACHED_CHARS: usize = 128;
const MAX_CACHED_POINTS: usize = 65_536;
const MAX_CACHED_BYTES: usize = 2 * 1024 * 1024;

type Sample = Vec<HandwritingStroke>;

// Sampling only produces these failures; keeping them cloneable also caches negative
// results without expanding RenderError's public trait contract.
#[derive(Clone)]
enum SampleFailure {
    MissingGlyph(char),
    InvalidFont,
    InvalidObject(String),
    Limit(&'static str),
}
impl From<RenderError> for SampleFailure {
    fn from(error: RenderError) -> Self {
        match error {
            RenderError::MissingGlyph(ch) => Self::MissingGlyph(ch),
            RenderError::InvalidFont => Self::InvalidFont,
            RenderError::ResourceLimit(reason) => Self::Limit(reason),
            RenderError::InvalidObject(reason) => Self::InvalidObject(reason),
            other => Self::InvalidObject(other.to_string()),
        }
    }
}
impl From<SampleFailure> for RenderError {
    fn from(error: SampleFailure) -> Self {
        match error {
            SampleFailure::MissingGlyph(ch) => Self::MissingGlyph(ch),
            SampleFailure::InvalidFont => Self::InvalidFont,
            SampleFailure::Limit(reason) => Self::ResourceLimit(reason),
            SampleFailure::InvalidObject(reason) => Self::InvalidObject(reason),
        }
    }
}

struct SampleEntry {
    ch: char,
    result: std::result::Result<Sample, SampleFailure>,
    points: usize,
    bytes: usize,
}
impl SampleEntry {
    fn new(ch: char, result: Result<Sample>) -> Self {
        let result = result.map_err(SampleFailure::from);
        let (points, payload) = match &result {
            Ok(strokes) => (
                strokes.iter().map(|s| s.points.len()).sum(),
                strokes.capacity() * std::mem::size_of::<HandwritingStroke>()
                    + strokes
                        .iter()
                        .map(|s| s.points.capacity() * std::mem::size_of::<StrokePoint>())
                        .sum::<usize>(),
            ),
            Err(SampleFailure::InvalidObject(reason)) => (0, reason.capacity()),
            Err(_) => (0, 0),
        };
        Self {
            ch,
            result,
            points,
            bytes: std::mem::size_of::<Self>() + payload,
        }
    }

    fn sample(&self) -> Result<Sample> {
        self.result.clone().map_err(RenderError::from)
    }
}

#[derive(Default)]
struct SampleCache {
    entries: VecDeque<SampleEntry>,
    points: usize,
    bytes: usize,
    #[cfg(test)]
    builds: usize,
}
impl SampleCache {
    fn insert(&mut self, entry: SampleEntry) {
        if entry.points > MAX_CACHED_POINTS || entry.bytes > MAX_CACHED_BYTES {
            return;
        }
        while self.entries.len() >= MAX_CACHED_CHARS
            || self.points + entry.points > MAX_CACHED_POINTS
            || self.bytes + entry.bytes > MAX_CACHED_BYTES
        {
            let old = self
                .entries
                .pop_front()
                .expect("nonempty cache over budget");
            self.points -= old.points;
            self.bytes -= old.bytes;
        }
        self.points += entry.points;
        self.bytes += entry.bytes;
        self.entries.push_back(entry);
    }
}

struct SharedFont {
    font: FontArc,
    scale: OnceLock<Option<f32>>,
    samples: Mutex<SampleCache>,
}

const SIDE: usize = 96;
const PIXELS: usize = SIDE * SIDE;
const MAX_STROKES: usize = 16;
const MAX_POINTS: usize = 2048;
const THINNING_ITERATIONS: usize = SIDE;
const RASTER_CAP_HEIGHT: f32 = 54.0;
const SAMPLE_SCALE: f32 = 72.0 / RASTER_CAP_HEIGHT;
const BASELINE: f32 = 96.0;
// Clockwise, starting north; opposite directions differ by four.
const DIRECTIONS: [(isize, isize); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// A cheap, thread-safe snapshot of the caller's injected font only.
/// Clones and snapshots from the same resource font generation share an LRU cache:
/// at most 128 characters, 65,536 points and 2 MiB of accounted entry payload.
/// Keys contain only characters; no user style is retained. The injected face is
/// assumed upright; italic source outlines are not automatically de-slanted.
/// No font discovery, shaping, replacement glyphs, or user-style learning occurs here.
#[derive(Clone)]
pub struct HandwritingFont {
    shared: Arc<SharedFont>,
}

impl HandwritingFont {
    pub(crate) fn new(font: FontArc) -> Self {
        Self {
            shared: Arc::new(SharedFont {
                font,
                scale: OnceLock::new(),
                samples: Mutex::new(SampleCache::default()),
            }),
        }
    }

    /// Approximate one glyph with centerline ink on a 128-square sample canvas.
    /// Cap top is 24 and baseline is 96; punctuation retains its font-relative size
    /// and vertical placement. Width is 3, pressure is 1, and speed is 600 units/s.
    /// Missing glyphs and empty, oversized, or over-budget samples fail atomically.
    /// Callers should prefer their single-character recognition templates first.
    ///
    /// Run on a worker: misses are serialized per font generation to avoid duplicate
    /// expensive work and bound concurrent raster scratch memory. Hits may wait for
    /// one miss. Snapshot creation, set_font and clear_font never acquire this lock.
    /// Returned strokes are owned copies; styling them cannot mutate cached ink.
    pub fn sample(&self, ch: char) -> Result<Vec<HandwritingStroke>> {
        let mut cache = self
            .shared
            .samples
            .lock()
            .map_err(|_| RenderError::ResourceLimit("Glyph sample cache poisoned"))?;
        if let Some(index) = cache.entries.iter().position(|entry| entry.ch == ch) {
            let entry = cache.entries.remove(index).unwrap();
            let result = entry.sample();
            cache.entries.push_back(entry);
            return result;
        }
        #[cfg(test)]
        {
            cache.builds += 1;
        }
        let entry = SampleEntry::new(ch, self.sample_uncached(ch));
        let result = entry.sample();
        cache.insert(entry);
        result
    }

    fn reference_scale(&self) -> Result<f32> {
        self.shared
            .scale
            .get_or_init(|| {
                let font = &self.shared.font;
                // ab_glyph has no cap-height accessor. Never fit punctuation to its own
                // height; use the same reference for every character of this font.
                let reference_scale = 256.0;
                let zero = font.glyph_id('0');
                let height = (zero.0 != 0)
                    .then(|| font.outline_glyph(zero.with_scale(reference_scale)))
                    .flatten()
                    .map(|outline| outline.px_bounds().height())
                    .unwrap_or_else(|| font.as_scaled(reference_scale).ascent());
                let scale = reference_scale * RASTER_CAP_HEIGHT / height;
                (height.is_finite() && height > 0.0 && scale.is_finite() && scale > 0.0)
                    .then_some(scale)
            })
            .ok_or(RenderError::InvalidFont)
    }

    fn sample_uncached(&self, ch: char) -> Result<Vec<HandwritingStroke>> {
        let id = self.shared.font.glyph_id(ch);
        if id.0 == 0 {
            return Err(RenderError::MissingGlyph(ch));
        }

        let scale = self.reference_scale()?;
        let outline = self
            .shared
            .font
            .outline_glyph(id.with_scale(scale))
            .ok_or_else(|| RenderError::InvalidObject(format!("No drawable glyph for {ch:?}")))?;
        let bounds = outline.px_bounds();
        let width = bounds.width();
        let height = bounds.height();
        if ![bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y]
            .iter()
            .all(|v| v.is_finite())
            || width <= 0.0
            || height <= 0.0
            || width > (SIDE - 2) as f32
            || height > (SIDE - 2) as f32
            || width * SAMPLE_SCALE > 124.0
            || BASELINE + bounds.min.y * SAMPLE_SCALE < 2.0
            || BASELINE + bounds.max.y * SAMPLE_SCALE > 126.0
        {
            return Err(RenderError::ResourceLimit(
                "Glyph exceeds handwriting canvas",
            ));
        }

        // A one-pixel empty border makes thinning and neighbor lookups bounded.
        let mut mask = vec![false; PIXELS];
        outline.draw(|x, y, coverage| {
            if coverage >= 0.5 {
                mask[(y as usize + 1) * SIDE + x as usize + 1] = true;
            }
        });
        if !mask.iter().any(|&pixel| pixel) {
            return Err(RenderError::InvalidObject(format!(
                "Empty raster for {ch:?}"
            )));
        }
        thin(&mut mask)?;
        let paths = trace(&mask)?;
        make_strokes(&paths, width, bounds.min.y)
    }
}

fn neighbor(index: usize, direction: usize) -> Option<usize> {
    let (dx, dy) = DIRECTIONS[direction];
    let x = (index % SIDE).checked_add_signed(dx)?;
    let y = (index / SIDE).checked_add_signed(dy)?;
    (x < SIDE && y < SIDE).then_some(y * SIDE + x)
}

// Foreground uses eight-connectivity; the complementary background uses four.
// Counting its enclosed components detects holes without diagonal ambiguity.
fn topology(mask: &[bool]) -> (usize, usize) {
    let mut seen = vec![false; PIXELS];
    let mut stack = Vec::new();
    let (mut components, mut holes) = (0, 0);
    for start in 0..PIXELS {
        if seen[start] {
            continue;
        }
        let ink = mask[start];
        let mut border = false;
        seen[start] = true;
        stack.push(start);
        while let Some(index) = stack.pop() {
            let (x, y) = (index % SIDE, index / SIDE);
            border |= x == 0 || y == 0 || x == SIDE - 1 || y == SIDE - 1;
            for direction in (0..8).step_by(if ink { 1 } else { 2 }) {
                if let Some(next) = neighbor(index, direction)
                    && mask[next] == ink
                    && !seen[next]
                {
                    seen[next] = true;
                    stack.push(next);
                }
            }
        }
        if ink {
            components += 1;
        } else if !border {
            holes += 1;
        }
    }
    (components, holes)
}

fn thin(mask: &mut [bool]) -> Result<()> {
    let before = topology(mask);
    let endpoints: Vec<bool> = (0..PIXELS)
        .map(|index| mask[index] && edges(mask, index).count_ones() == 1)
        .collect();
    // Zhang-Suen can erase a tiny solid component (notably a 2x2 dot) in a
    // simultaneous deletion pass. Retain at least one pixel per input component.
    let mut labels = vec![usize::MAX; PIXELS];
    let mut remaining = Vec::new();
    let mut stack = Vec::new();
    for start in 0..PIXELS {
        if !mask[start] || labels[start] != usize::MAX {
            continue;
        }
        let label = remaining.len();
        remaining.push(0usize);
        labels[start] = label;
        stack.push(start);
        while let Some(index) = stack.pop() {
            remaining[label] += 1;
            for direction in 0..8 {
                if let Some(next) = neighbor(index, direction)
                    && mask[next]
                    && labels[next] == usize::MAX
                {
                    labels[next] = label;
                    stack.push(next);
                }
            }
        }
    }

    let mut remove = Vec::new();
    for _ in 0..THINNING_ITERATIONS {
        let mut changed = false;
        for second_pass in [false, true] {
            remove.clear();
            for index in 0..PIXELS {
                if !mask[index] || endpoints[index] {
                    continue;
                }
                let p: [bool; 8] =
                    std::array::from_fn(|d| neighbor(index, d).is_some_and(|next| mask[next]));
                let count = p.iter().filter(|&&v| v).count();
                let transitions = (0..8).filter(|&i| !p[i] && p[(i + 1) % 8]).count();
                let preserves_connection = if second_pass {
                    !(p[0] && p[2] && p[6] || p[0] && p[4] && p[6])
                } else {
                    !(p[0] && p[2] && p[4] || p[2] && p[4] && p[6])
                };
                if (2..=6).contains(&count) && transitions == 1 && preserves_connection {
                    remove.push(index);
                }
            }
            for &index in &remove {
                let count = &mut remaining[labels[index]];
                if *count > 1 {
                    *count -= 1;
                    mask[index] = false;
                    changed = true;
                }
            }
        }
        if !changed {
            return if topology(mask) == before {
                Ok(())
            } else {
                Err(RenderError::InvalidObject(
                    "Glyph thinning changed topology".into(),
                ))
            };
        }
    }
    Err(RenderError::ResourceLimit(
        "Glyph thinning did not converge",
    ))
}

fn edges(mask: &[bool], index: usize) -> u8 {
    let mut result = 0;
    for (direction, &(dx, dy)) in DIRECTIONS.iter().enumerate() {
        if !neighbor(index, direction).is_some_and(|next| mask[next]) {
            continue;
        }
        // Do not add diagonal shortcuts across existing orthogonal connections:
        // those manufacture triangular loops and duplicate branches at corners.
        if dx != 0 && dy != 0 {
            let horizontal = index.checked_add_signed(dx).is_some_and(|i| mask[i]);
            let vertical = index
                .checked_add_signed(dy * SIDE as isize)
                .is_some_and(|i| mask[i]);
            if horizontal || vertical {
                continue;
            }
        }
        result |= 1 << direction;
    }
    result
}

fn trace(mask: &[bool]) -> Result<Vec<Vec<usize>>> {
    let graph: Vec<u8> = (0..PIXELS)
        .map(|i| if mask[i] { edges(mask, i) } else { 0 })
        .collect();
    let mut visited = vec![0u8; PIXELS];
    let mut paths = Vec::new();
    // First split at endpoints/junctions, then consume all degree-two cycles.
    for cycles in [false, true] {
        for start in 0..PIXELS {
            if !mask[start] || (graph[start].count_ones() == 2) != cycles {
                continue;
            }
            if graph[start] == 0 {
                paths.push(vec![start]);
            }
            while graph[start] & !visited[start] != 0 {
                let mut path = vec![start];
                let mut current = start;
                // An undirected eight-neighbor graph has at most 4 * PIXELS edges.
                for _ in 0..4 * PIXELS {
                    let available = graph[current] & !visited[current];
                    if available == 0 {
                        break;
                    }
                    let direction = available.trailing_zeros() as usize;
                    let next = neighbor(current, direction).expect("validated graph edge");
                    visited[current] |= 1 << direction;
                    visited[next] |= 1 << ((direction + 4) % 8);
                    path.push(next);
                    current = next;
                    if current == start || graph[current].count_ones() != 2 {
                        break;
                    }
                }
                paths.push(path);
                if paths.len() > MAX_STROKES {
                    return Err(RenderError::ResourceLimit("Glyph exceeds 16 strokes"));
                }
            }
            if paths.len() > MAX_STROKES {
                return Err(RenderError::ResourceLimit("Glyph exceeds 16 strokes"));
            }
        }
    }
    Ok(paths)
}

fn pixel(index: usize) -> (f32, f32) {
    ((index % SIDE) as f32, (index / SIDE) as f32)
}

fn simplify(path: &[usize]) -> Vec<usize> {
    if path.len() < 3 {
        return path.to_vec();
    }
    let mut result = vec![path[0]];
    for triple in path.windows(3) {
        let (a, b, c) = (pixel(triple[0]), pixel(triple[1]), pixel(triple[2]));
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let (ex, ey) = (c.0 - b.0, c.1 - b.1);
        // Only remove forward collinear points. An error-tolerance shortcut can
        // cross a nearby branch or close a one-pixel hole. Exact geometry keeps
        // topology, with linear CPU cost; excess points reject the whole sample.
        if dx * ey != dy * ex || dx * ex + dy * ey <= 0.0 {
            result.push(triple[1]);
        }
    }
    result.push(*path.last().unwrap());
    result
}

fn make_strokes(paths: &[Vec<usize>], width: f32, top: f32) -> Result<Vec<HandwritingStroke>> {
    if paths.is_empty() || paths.len() > MAX_STROKES {
        return Err(RenderError::ResourceLimit("Glyph needs 1..16 strokes"));
    }
    let mut strokes = Vec::with_capacity(paths.len());
    let mut total = 0;
    for path in paths {
        let path = simplify(path);
        total += path.len();
        if path.is_empty() || total > MAX_POINTS {
            return Err(RenderError::ResourceLimit("Glyph exceeds 2048 points"));
        }
        let mut points: Vec<StrokePoint> = Vec::with_capacity(path.len());
        let mut time = 0.0;
        for index in path {
            let (x, y) = pixel(index);
            let x = 64.0 + (x - 1.0 + 0.5 - width / 2.0) * SAMPLE_SCALE;
            let y = BASELINE + (top + y - 1.0 + 0.5) * SAMPLE_SCALE;
            if !(0.0..=128.0).contains(&x) || !(0.0..=128.0).contains(&y) {
                return Err(RenderError::ResourceLimit(
                    "Glyph exceeds handwriting canvas",
                ));
            }
            if let Some(previous) = points.last() {
                time += f64::from((x - previous.x).hypot(y - previous.y)) / 600.0;
            }
            points.push(StrokePoint {
                x,
                y,
                time,
                pressure: 1.0,
            });
        }
        strokes.push(HandwritingStroke {
            points,
            style: Style::default(),
        });
    }
    Ok(strokes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderResources;
    use std::collections::BTreeSet;

    fn resources() -> RenderResources {
        let definitions = egui::FontDefinitions::default();
        let data = definitions
            .font_data
            .get("Ubuntu-Light")
            .expect("egui's embedded Ubuntu font");
        let mut resources = RenderResources::new();
        resources.set_font(data.font.to_vec(), data.index).unwrap();
        resources
    }

    fn mask_at(points: &[(usize, usize)]) -> Vec<bool> {
        let mut mask = vec![false; PIXELS];
        for &(x, y) in points {
            mask[y * SIDE + x] = true;
        }
        mask
    }

    fn extents(strokes: &[HandwritingStroke]) -> (f32, f32, f32, f32) {
        strokes.iter().flat_map(|s| &s.points).fold(
            (
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ),
            |(x0, y0, x1, y1), p| (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
        )
    }

    #[test]
    fn snapshot_is_lightweight_send_sync_and_survives_resource_changes() {
        fn send_sync<T: Send + Sync + Clone>() {}
        send_sync::<HandwritingFont>();
        assert_eq!(
            std::mem::size_of::<HandwritingFont>(),
            std::mem::size_of::<Arc<SharedFont>>()
        );
        assert!(RenderResources::new().handwriting_font().is_none());
        let mut resources = resources();
        let snapshot = resources.handwriting_font().unwrap();
        let expected = snapshot.sample('A').unwrap();
        assert!(resources.set_font(vec![0], 0).is_err());
        assert_eq!(
            resources.handwriting_font().unwrap().sample('A').unwrap(),
            expected
        );
        resources.clear_font();
        assert!(resources.handwriting_font().is_none());
        drop(resources);
        assert_eq!(
            std::thread::spawn(move || snapshot.clone().sample('A').unwrap())
                .join()
                .unwrap(),
            expected
        );
    }

    #[test]
    fn cache_is_shared_single_flight_unstyled_and_generation_scoped() {
        let mut resources = resources();
        let font = resources.handwriting_font().unwrap();
        let other = resources.handwriting_font().unwrap();
        assert!(Arc::ptr_eq(&font.shared, &other.shared));
        let barrier = Arc::new(std::sync::Barrier::new(8));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let font = font.clone();
                let barrier = barrier.clone();
                scope.spawn(move || {
                    barrier.wait();
                    font.sample('A').unwrap();
                    assert!(font.sample(' ').is_err());
                });
            }
        });
        assert_eq!(font.shared.samples.lock().unwrap().builds, 2);
        let expected = font.sample('A').unwrap();
        let mut styled = other.sample('A').unwrap();
        styled[0].points[0].x = -999.0;
        styled[0].style.width = 19.0;
        assert_eq!(font.sample('A').unwrap(), expected);
        let scale = font.shared.scale.get().unwrap() as *const _;
        font.sample('B').unwrap();
        assert_eq!(font.shared.scale.get().unwrap() as *const _, scale);
        assert!(resources.set_font(vec![0], 0).is_err());
        assert!(Arc::ptr_eq(
            &font.shared,
            &resources.handwriting_font().unwrap().shared
        ));
        let definitions = egui::FontDefinitions::default();
        let data = &definitions.font_data["Ubuntu-Light"];
        resources.set_font(data.font.to_vec(), data.index).unwrap();
        let replacement = resources.handwriting_font().unwrap();
        assert!(!Arc::ptr_eq(&font.shared, &replacement.shared));
        assert_eq!(replacement.shared.samples.lock().unwrap().builds, 0);
        resources.clear_font();
        assert_eq!(font.sample('A').unwrap(), expected);
        assert_eq!(replacement.sample('A').unwrap(), expected);
    }

    #[test]
    fn cache_lru_negative_results_and_payload_counters_are_bounded() {
        let font = resources().handwriting_font().unwrap();
        for value in 0..MAX_CACHED_CHARS + 20 {
            assert!(
                font.sample(char::from_u32(0x100000 + value as u32).unwrap())
                    .is_err()
            );
        }
        let mut cache = font.shared.samples.lock().unwrap();
        assert_eq!(cache.entries.len(), MAX_CACHED_CHARS);
        assert_eq!(cache.entries.front().unwrap().ch as u32, 0x100014);
        assert_eq!(cache.points, 0);
        assert_eq!(
            cache.bytes,
            cache.entries.iter().map(|e| e.bytes).sum::<usize>()
        );
        drop(cache);
        font.sample(char::from_u32(0x100014).unwrap()).unwrap_err();
        font.sample('A').unwrap();
        cache = font.shared.samples.lock().unwrap();
        assert!(cache.entries.iter().any(|e| e.ch as u32 == 0x100014));
        assert!(!cache.entries.iter().any(|e| e.ch as u32 == 0x100015));
        drop(cache);

        let mut cache = SampleCache::default();
        for ch in 0..100 {
            let strokes = vec![HandwritingStroke {
                points: vec![
                    StrokePoint {
                        x: 0.0,
                        y: 0.0,
                        time: 0.0,
                        pressure: 1.0
                    };
                    MAX_POINTS
                ],
                style: Style::default(),
            }];
            cache.insert(SampleEntry::new(char::from_u32(ch).unwrap(), Ok(strokes)));
            assert!(cache.points <= MAX_CACHED_POINTS);
            assert!(cache.bytes <= MAX_CACHED_BYTES);
            assert_eq!(
                cache.points,
                cache.entries.iter().map(|e| e.points).sum::<usize>()
            );
            assert_eq!(
                cache.bytes,
                cache.entries.iter().map(|e| e.bytes).sum::<usize>()
            );
        }
        assert!(cache.entries.len() < 100);
        let mut oversized =
            Vec::with_capacity(MAX_CACHED_BYTES / std::mem::size_of::<HandwritingStroke>() + 1);
        oversized.push(HandwritingStroke {
            points: Vec::new(),
            style: Style::default(),
        });
        let old = cache.bytes;
        cache.insert(SampleEntry::new('X', Ok(oversized)));
        assert_eq!(cache.bytes, old);

        // Capacity, not length, must also drive eviction independently of points.
        let mut cache = SampleCache::default();
        for ch in ['a', 'b', 'c'] {
            let mut points = Vec::with_capacity(40_000);
            points.push(StrokePoint {
                x: 0.0,
                y: 0.0,
                time: 0.0,
                pressure: 1.0,
            });
            cache.insert(SampleEntry::new(
                ch,
                Ok(vec![HandwritingStroke {
                    points,
                    style: Style::default(),
                }]),
            ));
            assert!(cache.bytes <= MAX_CACHED_BYTES);
            assert_eq!(cache.points, cache.entries.len());
        }
        assert!(!cache.entries.iter().any(|e| e.ch == 'a'));
        assert!(cache.entries.iter().any(|e| e.ch == 'c'));
    }

    #[test]
    fn embedded_font_samples_are_deterministic_and_bounded() {
        let font = resources().handwriting_font().unwrap();
        for ch in
            "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz.,:;+-=*/()[]!?".chars()
        {
            let strokes = font
                .sample(ch)
                .unwrap_or_else(|error| panic!("{ch:?}: {error}"));
            assert_eq!(strokes, font.sample(ch).unwrap(), "{ch:?}");
            assert!((1..=MAX_STROKES).contains(&strokes.len()));
            assert!(strokes.iter().map(|s| s.points.len()).sum::<usize>() <= MAX_POINTS);
            for stroke in strokes {
                assert_eq!(stroke.style, Style::default());
                assert!(!stroke.points.is_empty());
                assert_eq!(stroke.points[0].time, 0.0);
                for p in &stroke.points {
                    assert!((0.0..=128.0).contains(&p.x));
                    assert!((0.0..=128.0).contains(&p.y));
                    assert_eq!(p.pressure, 1.0);
                }
                for pair in stroke.points.windows(2) {
                    let distance = (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y);
                    assert!(
                        (pair[1].time - pair[0].time - f64::from(distance) / 600.0).abs() < 1e-10
                    );
                }
            }
        }
    }

    #[test]
    fn missing_glyph_is_not_replaced_and_blank_is_not_ink() {
        let font = resources().handwriting_font().unwrap();
        let missing = '\u{10ffff}';
        assert_eq!(font.shared.font.glyph_id(missing).0, 0);
        assert!(matches!(font.sample(missing), Err(RenderError::MissingGlyph(c)) if c == missing));
        assert!(font.sample(' ').is_err());
    }

    #[test]
    fn punctuation_keeps_baseline_size_and_disconnected_dots() {
        let font = resources().handwriting_font().unwrap();
        let (_, zero_top, _, zero_bottom) = extents(&font.sample('0').unwrap());
        assert!((20.0..35.0).contains(&zero_top));
        assert!((88.0..102.0).contains(&zero_bottom));
        let (left, top, right, bottom) = extents(&font.sample('.').unwrap());
        assert!(right - left < 12.0 && bottom - top < 12.0);
        assert!(top > 85.0 && bottom < 102.0);
        let minus = font.sample('-').unwrap();
        let (left, top, right, bottom) = extents(&minus);
        assert_eq!(
            minus.len(),
            1,
            "minus must be one centerline, not a hollow outline"
        );
        assert!(right - left > 10.0 && bottom - top < 5.0);
        assert!(top > 50.0 && bottom < 85.0);
        let colon = font.sample(':').unwrap();
        assert_eq!(colon.len(), 2);
        assert!(extents(&colon[..1]).3 + 10.0 < extents(&colon[1..]).1);
        assert!(font.sample('i').unwrap().len() >= 2);
        let (_, _, _, descender) = extents(&font.sample('g').unwrap());
        assert!(descender > 100.0);
    }

    #[test]
    fn thinning_preserves_tiny_disconnected_components_and_finds_centerline() {
        let mut mask = mask_at(&[(4, 4), (5, 4), (4, 5), (5, 5), (20, 20)]);
        for y in 40..47 {
            for x in 20..70 {
                mask[y * SIDE + x] = true;
            }
        }
        thin(&mut mask).unwrap();
        let paths = trace(&mask).unwrap();
        assert_eq!(paths.len(), 3);
        assert_eq!(paths[0].len(), 1);
        assert_eq!(paths[1].len(), 1);
        assert!(paths[2].len() > 35);
        assert!(paths[2].iter().all(|&i| i / SIDE == 43));
        let stable = mask.clone();
        thin(&mut mask).unwrap();
        assert_eq!(mask, stable);
    }

    #[test]
    fn trace_visits_each_edge_once_including_junctions_and_closed_loops() {
        let mut mask = vec![false; PIXELS];
        for x in 5..16 {
            mask[5 * SIDE + x] = true;
            mask[15 * SIDE + x] = true;
        }
        for y in 5..16 {
            mask[y * SIDE + 5] = true;
            mask[y * SIDE + 15] = true;
        }
        for x in 30..41 {
            mask[35 * SIDE + x] = true;
        }
        for y in 30..41 {
            mask[y * SIDE + 35] = true;
        }
        let paths = trace(&mask).unwrap();
        let loops: Vec<_> = paths.iter().filter(|p| p.first() == p.last()).collect();
        assert_eq!(loops.len(), 1);
        assert!(simplify(loops[0]).len() >= 5);
        let mut expected = BTreeSet::new();
        for i in 0..PIXELS {
            if mask[i] {
                for direction in 0..8 {
                    if edges(&mask, i) & (1 << direction) != 0 {
                        let j = neighbor(i, direction).unwrap();
                        expected.insert((i.min(j), i.max(j)));
                    }
                }
            }
        }
        let mut actual = BTreeSet::new();
        for path in paths {
            for pair in path.windows(2) {
                assert!(actual.insert((pair[0].min(pair[1]), pair[0].max(pair[1]))));
            }
        }
        assert_eq!(actual, expected);
    }

    #[test]
    fn stroke_and_point_budgets_reject_whole_samples() {
        let mut mask = vec![false; PIXELS];
        for x in (2..34).step_by(2) {
            mask[2 * SIDE + x] = true;
        }
        assert_eq!(trace(&mask).unwrap().len(), MAX_STROKES);
        mask[2 * SIDE + 34] = true;
        assert!(matches!(trace(&mask), Err(RenderError::ResourceLimit(_))));
        // Alternating far-apart endpoints cannot be simplified without losing
        // reversals. Exercise the exact total-point boundary and its rejection.
        let path: Vec<_> = (0..MAX_POINTS)
            .map(|i| 10 * SIDE + if i % 2 == 0 { 10 } else { 60 })
            .collect();
        let strokes = make_strokes(&[path.clone()], 80.0, -50.0).unwrap();
        assert_eq!(strokes[0].points.len(), MAX_POINTS);
        let mut over = path;
        over.push(10 * SIDE + 10);
        assert!(matches!(
            make_strokes(&[over], 80.0, -50.0),
            Err(RenderError::ResourceLimit(_))
        ));
    }

    fn assert_complete_paths(mask: &[bool]) {
        let Ok(paths) = trace(mask) else {
            // Complexity rejection is allowed; partial successful ink is not.
            return;
        };
        let mut covered = BTreeSet::new();
        let mut actual = BTreeSet::new();
        for path in &paths {
            covered.extend(path.iter().copied());
            for pair in path.windows(2) {
                assert!(actual.insert((pair[0].min(pair[1]), pair[0].max(pair[1]))));
            }
            let simplified = simplify(path);
            assert_eq!(simplified.first(), path.first());
            assert_eq!(simplified.last(), path.last());
            // Expand exact straight segments back to pixel edges: no skipped
            // branches, new crossings or collapsed tiny loops can hide here.
            let mut expanded = vec![simplified[0]];
            for pair in simplified.windows(2) {
                let (mut x, mut y) = (pair[0] % SIDE, pair[0] / SIDE);
                let (ex, ey) = (pair[1] % SIDE, pair[1] / SIDE);
                let dx = (ex as isize - x as isize).signum();
                let dy = (ey as isize - y as isize).signum();
                while (x, y) != (ex, ey) {
                    x = x.checked_add_signed(dx).unwrap();
                    y = y.checked_add_signed(dy).unwrap();
                    expanded.push(y * SIDE + x);
                    assert!(expanded.len() <= path.len());
                }
            }
            assert_eq!(expanded, *path);
        }
        let expected_pixels: BTreeSet<_> = (0..PIXELS).filter(|&i| mask[i]).collect();
        assert_eq!(covered, expected_pixels);
        let mut expected = BTreeSet::new();
        for &i in &expected_pixels {
            for direction in 0..8 {
                if edges(mask, i) & (1 << direction) != 0 {
                    let j = neighbor(i, direction).unwrap();
                    expected.insert((i.min(j), i.max(j)));
                }
            }
        }
        assert_eq!(actual, expected);
    }

    #[test]
    fn adversarial_masks_preserve_components_holes_endpoints_and_every_branch_or_fail() {
        // Exhaust all 3x3 neighborhoods: diagonal bridges, tiny holes, 2x2
        // components, closely spaced junctions and short spurs are included.
        for bits in 1..512 {
            let mut mask = vec![false; PIXELS];
            for bit in 0..9 {
                mask[(10 + bit / 3) * SIDE + 10 + bit % 3] = bits & (1 << bit) != 0;
            }
            assert_complete_paths(&mask);
            let before = topology(&mask);
            let ends: Vec<_> = (0..PIXELS)
                .filter(|&i| mask[i] && edges(&mask, i).count_ones() == 1)
                .collect();
            if thin(&mut mask).is_ok() {
                assert_eq!(topology(&mask), before, "mask {bits}");
                assert!(ends.iter().all(|&i| mask[i]), "mask {bits} lost a branch");
                assert_complete_paths(&mask);
            }
        }
        // Two holes joined by a narrow bridge, with external branches and an
        // isolated punctuation dot. This must succeed, not only reject safely.
        let mut mask = vec![false; PIXELS];
        for offset in [0, 20] {
            for y in 20..31 {
                for x in 10 + offset..21 + offset {
                    mask[y * SIDE + x] = y == 20 || y == 30 || x == 10 + offset || x == 20 + offset;
                }
            }
        }
        for x in 5..46 {
            mask[25 * SIDE + x] = true;
        }
        mask[50 * SIDE + 50] = true;
        let before = topology(&mask);
        assert_eq!(before, (2, 4));
        thin(&mut mask).unwrap();
        assert_eq!(topology(&mask), before);
        assert_complete_paths(&mask);
        assert!(trace(&mask).is_ok());
    }

    #[test]
    fn dense_masks_and_rings_terminate_without_losing_the_hole() {
        let mut solid = vec![false; PIXELS];
        let mut ring = solid.clone();
        for y in 1..SIDE - 1 {
            for x in 1..SIDE - 1 {
                solid[y * SIDE + x] = true;
                ring[y * SIDE + x] = !(20..76).contains(&x) || !(20..76).contains(&y);
            }
        }
        thin(&mut solid).unwrap();
        assert!(!trace(&solid).unwrap().is_empty());
        thin(&mut ring).unwrap();
        let paths = trace(&ring).unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].first(), paths[0].last());
        assert!(simplify(&paths[0]).len() >= 5);
    }
}
