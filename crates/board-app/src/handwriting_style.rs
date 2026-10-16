//! Bounded numeric style estimates, not recognition or character segmentation.
use super::*;

// Persist only this rolling window of scalar heuristics, not an alphabet or
// unknown-character exemplars. Defaults remain explicitly unlearned on reload.
const WINDOW: usize = 31;
const RESAMPLE_POINTS: usize = 33;

fn default_speed_peak() -> f32 {
    1.35
}

fn default_corner() -> f32 {
    0.5
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    width: f32,
    slant: Option<f32>,
    aspect: Option<f32>,
    speed: Option<f32>,
    #[serde(default)]
    speed_peak: Option<f32>,
    #[serde(default)]
    corner: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LearnedStyle {
    learned_strokes: u32,
    pen_width: f32,
    slant: f32,
    aspect: f32,
    speed: f32,
    // Shape how synthesized ink moves: peak-to-mean speed ratio and corner
    // sharpness. Scalar statistics only, never unlabelled ink or positions.
    #[serde(default = "default_speed_peak")]
    speed_peak: f32,
    #[serde(default = "default_corner")]
    corner: f32,
    // Only scalar measurements survive a call; never unlabelled ink or positions.
    recent: Vec<Observation>,
}

impl Default for LearnedStyle {
    fn default() -> Self {
        Self {
            learned_strokes: 0,
            pen_width: 3.0,
            slant: -0.08,
            aspect: 0.7,
            speed: 240.0,
            speed_peak: default_speed_peak(),
            corner: default_corner(),
            recent: Vec::new(),
        }
    }
}

impl LearnedStyle {
    pub(super) fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub(super) fn count(&self) -> u32 {
        self.learned_strokes
    }

    /// Learned peak-to-mean speed ratio shaping synthetic stroke motion.
    pub(super) fn motion_peak(&self) -> f32 {
        self.speed_peak
    }

    /// Learned corner sharpness (0..1) shaping font-skeleton smoothing.
    pub(super) fn corner(&self) -> f32 {
        self.corner
    }

    pub(super) fn summary(&self) -> String {
        format!(
            "{}; strokes={}; window={}/{}; width@72={:.2}; slant(dx/dy)={:.3}; aspect={:.2}; speed@72={:.1}/s; peak×{:.2}; corner={:.2} (stroke-bbox heuristic, not character segmentation)",
            if self.learned_strokes == 0 {
                "Default style (not learned)"
            } else {
                "Locally learned style"
            },
            self.learned_strokes,
            self.recent.len(),
            WINDOW,
            self.pen_width,
            self.slant,
            self.aspect,
            self.speed,
            self.speed_peak,
            self.corner,
        )
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        let in_range = |value: f32, low, high| (low..=high).contains(&value);
        let valid_observation = |o: &Observation| {
            in_range(o.width, 0.5, 12.0)
                && o.slant.is_none_or(|v| in_range(v, -0.6, 0.6))
                && o.aspect.is_none_or(|v| in_range(v, 0.35, 1.4))
                && o.speed.is_none_or(|v| in_range(v, 60.0, 900.0))
                && o.speed_peak.is_none_or(|v| in_range(v, 1.0, 2.2))
                && o.corner.is_none_or(|v| in_range(v, 0.0, 1.0))
        };
        if !in_range(self.pen_width, 0.5, 12.0)
            || !in_range(self.slant, -0.6, 0.6)
            || !in_range(self.aspect, 0.35, 1.4)
            || !in_range(self.speed, 60.0, 900.0)
            || !in_range(self.speed_peak, 1.0, 2.2)
            || !in_range(self.corner, 0.0, 1.0)
            || self.recent.len() != self.learned_strokes.min(WINDOW as u32) as usize
            || !self.recent.iter().all(valid_observation)
            || (self.learned_strokes == 0 && !self.is_default())
        {
            return Err("Invalid learned handwriting style statistics".into());
        }
        Ok(())
    }

    pub(super) fn observe(&mut self, points: &[StrokePoint], style: Style) -> bool {
        if self.validate().is_err() {
            return false;
        }
        let Some(observation) = measure(points, style) else {
            return false;
        };
        if self.recent.len() == WINDOW {
            self.recent.remove(0);
        }
        self.recent.push(observation);
        self.learned_strokes = self.learned_strokes.saturating_add(1);
        // One vote per stroke, independent of pointer sampling density. A rolling
        // median rejects isolated outliers; capped EWMA also limits each update.
        update(
            &mut self.pen_width,
            self.recent.iter().map(|o| o.width),
            0.6,
        );
        update(
            &mut self.slant,
            self.recent.iter().filter_map(|o| o.slant),
            0.04,
        );
        update(
            &mut self.aspect,
            self.recent.iter().filter_map(|o| o.aspect),
            0.08,
        );
        update(
            &mut self.speed,
            self.recent.iter().filter_map(|o| o.speed),
            40.0,
        );
        update(
            &mut self.speed_peak,
            self.recent.iter().filter_map(|o| o.speed_peak),
            0.15,
        );
        update(
            &mut self.corner,
            self.recent.iter().filter_map(|o| o.corner),
            0.08,
        );
        true
    }

    pub(super) fn apply(&self, sample: &mut [HandwritingStroke]) {
        let aspect_scale = (self.aspect / 0.7).clamp(0.75, 1.3);
        let slant = self.slant.clamp(-0.35, 0.35);
        let mut left = f32::INFINITY;
        let mut right = f32::NEG_INFINITY;
        for stroke in sample.iter_mut() {
            for point in &mut stroke.points {
                point.x = 64.0 + (point.x - 64.0) * aspect_scale + slant * (point.y - 96.0);
                left = left.min(point.x);
                right = right.max(point.x);
            }
        }
        // Keep the sample canvas invariant without clipping individual points or
        // moving the baseline/dots vertically. Only very wide outlines compress.
        let fit = (124.0 / (right - left).max(1.0)).min(1.0);
        let shift = if fit < 1.0 {
            2.0 - left * fit
        } else if left < 2.0 {
            2.0 - left
        } else if right > 126.0 {
            126.0 - right
        } else {
            0.0
        };
        for stroke in sample {
            stroke.style.width = self.pen_width;
            stroke.style.dashed = false;
            for point in &mut stroke.points {
                point.x = point.x * fit + shift;
            }
            // Total duration stays path_length / speed; the learned speed peak
            // only redistributes motion inside the stroke (ease-in/ease-out).
            super::eased_retime(&mut stroke.points, self.speed, self.speed_peak);
        }
    }
}

fn update(value: &mut f32, values: impl Iterator<Item = f32>, max_step: f32) {
    if let Some(target) = median(values.collect()) {
        *value += ((target - *value) * 0.2).clamp(-max_step, max_step);
    }
}

fn median(mut values: Vec<f32>) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    })
}

fn measure(points: &[StrokePoint], style: Style) -> Option<Observation> {
    if !(2..=MAX_SAMPLE_POINTS).contains(&points.len())
        || !(MIN_BRUSH_WIDTH..=MAX_BRUSH_WIDTH).contains(&style.width)
        || style.dashed
        || style.color.a == 0
    {
        return None;
    }
    let (mut left, mut top) = (f32::INFINITY, f32::INFINITY);
    let (mut right, mut bottom) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut previous_time = 0.0;
    let mut length = 0.0;
    for (i, p) in points.iter().enumerate() {
        if !valid_coord(p.x)
            || !valid_coord(p.y)
            || !(0.0..=1.0).contains(&p.pressure)
            || !(previous_time..=MAX_HANDWRITING_TIME).contains(&p.time)
        {
            return None;
        }
        previous_time = p.time;
        left = left.min(p.x);
        right = right.max(p.x);
        top = top.min(p.y);
        bottom = bottom.max(p.y);
        if i > 0 {
            length += (p.x - points[i - 1].x).hypot(p.y - points[i - 1].y);
        }
    }
    let width = right - left;
    let height = bottom - top;
    let duration = points.last()?.time - points[0].time;
    // A single-stroke bbox is only a writing-like size proxy. These deliberately
    // conservative limits discard dots, underlines, huge diagrams and scribbles;
    // they cannot distinguish every small diagram from genuine handwriting.
    if !(12.0..=240.0).contains(&height)
        || width > 240.0
        || width / height > 3.0
        || style.width > height * 0.4
        || length > 1200.0
        || length > width.hypot(height) * 8.0
        || duration > 30.0
    {
        return None;
    }
    // Equal arc-length sampling keeps slant independent of event density. Tall
    // chords, rather than tiny adjacent pointer deltas, avoid jitter amplification.
    let mut resampled = Vec::with_capacity(RESAMPLE_POINTS);
    let mut segment = 1;
    let mut traversed = 0.0;
    for i in 0..RESAMPLE_POINTS {
        let target = length * i as f32 / (RESAMPLE_POINTS - 1) as f32;
        loop {
            let a = &points[segment - 1];
            let b = &points[segment];
            let distance = (b.x - a.x).hypot(b.y - a.y);
            if traversed + distance >= target || segment == points.len() - 1 {
                let t = if distance > 0.0 {
                    ((target - traversed) / distance).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                resampled.push(Point {
                    x: a.x + (b.x - a.x) * t,
                    y: a.y + (b.y - a.y) * t,
                });
                break;
            }
            traversed += distance;
            segment += 1;
        }
    }
    let slant = median(
        resampled
            .windows(9)
            .filter_map(|pair| {
                let dx = pair[8].x - pair[0].x;
                let dy = pair[8].y - pair[0].y;
                (dy.abs() >= height * 0.15 && dx.abs() <= dy.abs() * 0.75)
                    .then(|| (dx / dy).clamp(-0.6, 0.6))
            })
            .collect(),
    );
    let aspect = width / height;
    // Speed profile: middle arc third versus the whole-stroke mean speed. The
    // ratio is scale-free; 1.0 is uniform motion, higher values mean a faster
    // middle (an ease shape). Corner sharpness: median absolute turn between
    // consecutive resampled segments, normalized so 45 degrees saturates.
    let speed_peak = if duration >= 0.02 && length > 0.0 {
        let arc_time = |target: f32| -> Option<f64> {
            let mut traversed = 0.0;
            for pair in points.windows(2) {
                let step = (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y);
                if traversed + step >= target {
                    let t = if step > 0.0 {
                        (target - traversed) / step
                    } else {
                        0.0
                    };
                    return Some(pair[0].time + (pair[1].time - pair[0].time) * f64::from(t));
                }
                traversed += step;
            }
            Some(points[points.len() - 1].time)
        };
        let third = length / 3.0;
        let middle = arc_time(2.0 * third)? - arc_time(third)?;
        if middle > 1e-9 {
            let ratio = (f64::from(third) / middle) / (f64::from(length) / duration);
            Some((ratio as f32).clamp(1.0, 2.2))
        } else {
            Some(2.2)
        }
    } else {
        None
    };
    let corner = {
        let mut turns = Vec::new();
        for triple in resampled.windows(3) {
            let (ax, ay) = (triple[1].x - triple[0].x, triple[1].y - triple[0].y);
            let (bx, by) = (triple[2].x - triple[1].x, triple[2].y - triple[1].y);
            if ax.hypot(ay) > 1e-6 && bx.hypot(by) > 1e-6 {
                turns.push((ax * by - ay * bx).atan2(ax * bx + ay * by).abs());
            }
        }
        median(
            turns
                .iter()
                .map(|angle| (angle.to_degrees() / 45.0).clamp(0.0, 1.0))
                .collect(),
        )
    };
    Some(Observation {
        width: (style.width * CAP_HEIGHT / height).clamp(0.5, 12.0),
        slant,
        // Narrow individual stems are useful for slant, not character width.
        aspect: (0.2..=1.8)
            .contains(&aspect)
            .then(|| aspect.clamp(0.35, 1.4)),
        speed: (duration >= 0.02).then(|| {
            (f64::from(length * CAP_HEIGHT / height) / duration).clamp(60.0, 900.0) as f32
        }),
        speed_peak,
        corner,
    })
}
