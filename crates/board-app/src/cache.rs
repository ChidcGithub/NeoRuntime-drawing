use super::{HwrBackend, NeuralFormula, Recognition};
use board_core::{BoardObject, Document, ObjectKind};
use board_hwr::CalculationRequest;
use egui::Rect;
use std::collections::HashMap;

// Admission limits, not an LRU: existing valid entries are never evicted for new ink.
const DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
const DOCUMENT_ENTRIES: usize = 4096;
const PAGE_ENTRIES: usize = 1024;

pub(super) struct Candidate {
    pub id: String,
    pub request: CalculationRequest,
    pub source: Vec<BoardObject>,
    neighbors: Vec<BoardObject>,
    pub bounds: Rect,
    pub expression: String,
    pub recognition: Option<Recognition>,
    pub neural: Option<NeuralFormula>,
    pub backend: HwrBackend,
    // Reuse one identity on regeneration, including across undo/redo.
    pub result_id: String,
    pub result_present: bool,
    bytes: usize,
}

#[derive(Default)]
pub(super) struct Cache {
    pub entries: Vec<Candidate>,
    pub epoch: u64,
    checked: Option<(String, u64)>,
}

impl Cache {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.checked = None;
        self.epoch = self.epoch.wrapping_add(1);
    }

    pub fn sync(&mut self, document: &Document) {
        // #region debug-point B:cache-sync
        #[cfg(test)]
        let _debug_stage = super::debug_ink::Stage::begin(4, "candidate_cache_sync");
        // #endregion
        if self
            .checked
            .as_ref()
            .is_some_and(|(id, revision)| id == &document.id && *revision == document.revision)
        {
            return;
        }
        if self.entries.is_empty() {
            self.checked = Some((document.id.clone(), document.revision));
            return;
        }
        let pages: HashMap<_, _> = document
            .pages
            .iter()
            .map(|page| {
                (
                    &page.id,
                    page.objects
                        .iter()
                        .map(|object| (&object.id, (object, board_render::object_bounds(object))))
                        .collect::<HashMap<_, _>>(),
                )
            })
            .collect();
        self.entries.retain_mut(|entry| {
            if entry.request.context.document_id != document.id {
                return false;
            }
            let Some(objects) = pages.get(&entry.request.context.page_id) else {
                return false;
            };
            if entry
                .source
                .iter()
                .any(|source| objects.get(&source.id).map(|(object, _)| *object) != Some(source))
            {
                return false;
            }
            let neighborhood = entry.bounds.expand2(egui::vec2(100.0, 28.0));
            let mut count = 0;
            for (object, bounds) in objects.values() {
                if object.id == entry.result_id
                    || entry.source.iter().any(|source| source.id == object.id)
                {
                    continue;
                }
                if neighborhood.intersects(*bounds) {
                    count += 1;
                    if !entry.neighbors.iter().any(|old| old == *object) {
                        return false;
                    }
                }
            }
            entry.result_present = objects.contains_key(&entry.result_id);
            count == entry.neighbors.len()
        });
        self.checked = Some((document.id.clone(), document.revision));
    }

    pub fn insert(
        &mut self,
        document: &Document,
        request: CalculationRequest,
        source: Vec<BoardObject>,
        recognition: Option<Recognition>,
        neural: Option<NeuralFormula>,
        backend: HwrBackend,
    ) -> Option<String> {
        self.sync(document);
        let page = document.current_page();
        // Only committed gesture objects qualify; never accept arbitrary candidate strokes.
        if source.is_empty() || source.len() != request.strokes.len() || source.iter().zip(&request.strokes).any(|(object, stroke)| {
            !page.objects.contains(object) || !matches!(&object.kind, ObjectKind::Stroke { points, .. } if points == stroke)
        }) { return None; }
        let bounds = super::features::ink_bounds(&request.strokes);
        let neighborhood = bounds.expand2(egui::vec2(100.0, 28.0));
        let neighbors: Vec<_> = page
            .objects
            .iter()
            .filter(|object| {
                !source.iter().any(|s| s.id == object.id)
                    && neighborhood.intersects(board_render::object_bounds(object))
            })
            .cloned()
            .collect();
        // JSON length conservatively covers string/point payloads plus owned request and allocation overhead.
        let bytes = source
            .iter()
            .chain(&neighbors)
            .map(|object| {
                serde_json::to_vec(object)
                    .map_or(DOCUMENT_BYTES, |bytes| bytes.len().saturating_mul(2) + 256)
            })
            .sum::<usize>()
            + request
                .strokes
                .iter()
                .map(|stroke| stroke.len() * std::mem::size_of::<board_core::StrokePoint>())
                .sum::<usize>()
            + 32768;
        if self.entries.len() >= DOCUMENT_ENTRIES
            || self
                .entries
                .iter()
                .filter(|e| e.request.context.page_id == page.id)
                .count()
                >= PAGE_ENTRIES
            || self
                .entries
                .iter()
                .map(|e| e.bytes)
                .sum::<usize>()
                .saturating_add(bytes)
                > DOCUMENT_BYTES
        {
            return None;
        }
        let id = request.id().to_owned();
        self.entries.push(Candidate {
            id: id.clone(),
            request,
            source,
            neighbors,
            bounds,
            expression: recognition
                .as_ref()
                .map_or_else(String::new, |r| r.text.clone()),
            recognition,
            neural,
            backend,
            result_id: board_core::new_id(),
            result_present: false,
            bytes,
        });
        Some(id)
    }

    pub fn get(&self, id: &str) -> Option<&Candidate> {
        self.entries.iter().find(|entry| entry.id == id)
    }
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Candidate> {
        self.entries.iter_mut().find(|entry| entry.id == id)
    }
}
