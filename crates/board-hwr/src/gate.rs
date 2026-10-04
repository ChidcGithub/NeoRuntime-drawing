use crate::{Result, validate_strokes};
use board_core::{Document, StrokePoint};
use std::time::{Duration, Instant};

/// Capture from the same document/page snapshot as the input strokes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextToken {
    pub document_id: String,
    pub page_id: String,
    pub revision: u64,
}
impl ContextToken {
    pub fn capture(document: &Document) -> Self {
        Self {
            document_id: document.id.clone(),
            page_id: document.current_page().id.clone(),
            revision: document.revision,
        }
    }
}

/// An immutable request identity with an owned stroke snapshot for a background worker.
#[derive(Debug, Clone, PartialEq)]
pub struct CalculationRequest {
    id: String,
    pub context: ContextToken,
    pub strokes: Vec<Vec<StrokePoint>>,
}
impl CalculationRequest {
    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug)]
struct Pending {
    request: CalculationRequest,
    last_pen_up: Instant,
    fired: bool,
}

/// UI-independent one-shot gate, initially disabled. Does not evaluate or write anything.
/// Feed pen-down immediately to `input_started`, pen-up plus committed revision to
/// `strokes_finished`, and all document/page/revision changes to `context_changed`.
/// Poll may start recognition/calculation after 2.5 s; even a high score requires a user
/// action calling `confirm`. Before actually writing, also use the core expected_revision
/// transaction: checking a token here does not lock the document against concurrent edits.
#[derive(Debug, Default)]
pub struct AutoCalculate {
    enabled: bool,
    pending: Option<Pending>,
    last_capture: Option<(ContextToken, Vec<Vec<StrokePoint>>)>,
}
impl AutoCalculate {
    pub const IDLE_DELAY: Duration = Duration::from_millis(2500);

    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.cancel();
        }
    }

    /// Invalidate an in-flight worker result as soon as the user resumes writing.
    pub fn input_started(&mut self) {
        self.cancel();
    }

    /// Call only after pen-up (not per pointer move), using the final committed revision.
    /// Duplicate notifications for exactly the same token and strokes do not rearm.
    pub fn strokes_finished(
        &mut self,
        now: Instant,
        context: ContextToken,
        strokes: &[Vec<StrokePoint>],
    ) -> Result<()> {
        if let Err(error) = validate_strokes(strokes) {
            self.cancel();
            return Err(error);
        }
        if !self.enabled {
            return Ok(());
        }
        if self
            .last_capture
            .as_ref()
            .is_some_and(|(token, old)| token == &context && old == strokes)
        {
            return Ok(());
        }
        self.cancel();
        self.last_capture = Some((context.clone(), strokes.to_vec()));
        self.pending = Some(Pending {
            request: CalculationRequest {
                id: board_core::new_id(),
                context,
                strokes: strokes.to_vec(),
            },
            last_pen_up: now,
            fired: false,
        });
        Ok(())
    }

    /// Switching away and back still cancels the old request: call on every change.
    pub fn context_changed(&mut self, current: &ContextToken) {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| &p.request.context != current)
        {
            self.cancel();
        }
    }

    /// Returns at most once per input snapshot; elapsed time uses a monotonic clock.
    pub fn poll(&mut self, now: Instant, current: &ContextToken) -> Option<CalculationRequest> {
        self.context_changed(current);
        if !self.enabled {
            return None;
        }
        let pending = self.pending.as_mut()?;
        if pending.fired || now.checked_duration_since(pending.last_pen_up)? < Self::IDLE_DELAY {
            return None;
        }
        pending.fired = true;
        Some(pending.request.clone())
    }

    /// Call only after explicit user approval of the chosen candidate/calculation.
    /// Consumes the request once. False means stale, cancelled, modified or duplicate.
    /// This is deliberately not an automatic confidence-threshold approval API.
    pub fn confirm(&mut self, request: &CalculationRequest, current: &ContextToken) -> bool {
        self.context_changed(current);
        let valid = self.enabled
            && self.pending.as_ref().is_some_and(|pending| {
                pending.fired && pending.request == *request && &request.context == current
            });
        if valid {
            self.cancel();
        }
        valid
    }

    /// Cancel timers and outstanding confirmations, for rejection, hide, undo or clear.
    pub fn cancel(&mut self) {
        self.pending = None;
    }
}
