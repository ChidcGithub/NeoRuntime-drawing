# Renderer cache contracts

## Font-derived handwriting

`RenderResources::handwriting_font()` returns a cheap `Send + Sync + Clone`
snapshot with no image or GUI texture ownership. Repeated snapshots and clones
share one font-generation cache. No file discovery, style learning, RPC or model
is involved.

- Successful `set_font`, including identical font bytes/face index, starts a new
  generation. Failed `set_font` preserves the current font and cache.
- `clear_font` detaches the resource's generation. Existing worker snapshots
  remain valid with their original font/cache until the last clone is dropped.
  Retaining many old generations therefore retains multiple bounded caches.
- Character is the only cache key. Results are **unstyled** owned stroke copies;
  applying style to a returned sample cannot change the cache. Failures are
  cached as well. There is no cross-font or process-global cache.
- LRU retention is limited to 128 characters, 65,536 points and 2 MiB of accounted
  entries (including stroke/point vector capacities and failure strings).
  Allocator bookkeeping and the bounded deque's spare entry slots are excluded.
- The reference `0` outline/scale, or ascent fallback, is initialized once per
  generation, independently of LRU eviction.
- Call `sample` on a background worker. A per-generation mutex serializes both
  hits and misses, eliminating duplicate concurrent builds while an entry stays
  resident and limiting raster scratch work to one glyph per generation. Hits
  may wait for a miss; this is not a nonblocking or hard-deadline API. Cloning a
  snapshot, replacing a font and clearing a font never acquire that mutex.
  Evicted characters may be sampled again. Poisoning fails explicitly.
- Raster/thinning work is bounded by a 96x96 mask and 96 thinning iterations.
  Samples exceeding 16 strokes or 2,048 points fail as a whole. Foreground
  eight-connectivity and background four-connectivity define components/holes.
  Thinning checks component/hole counts and protects original graph endpoints;
  tracing consumes every graph edge, and simplification only removes forward
  collinear points, never moving a branch or closing a tiny hole. This describes
  raster topology, not fidelity to the original outline or handwritten pen order.
- Inject an upright face. Source italic/slant is not estimated or normalized;
  adding a user slant to an italic source can compound the slant.

## Page meshes and revision fast path

`PageRenderer::paint_page_at_document_revision(painter, page,
(document_id, revision), blackboard, resources)` is the existing trusted path.
The caller must advance revision for every content/order change. After replacing
or reopening a document with potentially reused ID/revision, call `clear()`.
Page/document switches reset the cache; preview rendering through
`paint_page_with_resources` forces the next trusted draw to synchronize.
Clip, DPI and tessellation changes invalidate meshes even at the same revision.
Use `paint_page_at_revision` instead when same-revision direct mutation must be
caught by full source comparison.

Per-object and aggregate page mesh vertex/index **capacities together** are
limited to 64 MiB retained by each `PageRenderer`; aggregates additionally retain
at most 32 MiB. Rejected mesh retention uses the original scene's uncached draw,
not partial geometry. Changed chunks release old source Arcs; viewport-key changes
release obsolete offscreen meshes. Unchanged chunks and trusted idle draws still
reuse immutable Arcs without vertex copies.

This is not a whole-process or all-cache byte limit: source snapshots, scene
vectors, plot samples, thumbnail meshes, image/font resources, allocator/container
metadata, a mesh currently being built, and frame Arcs retained by egui have
separate ownership/budgets. A global cap across those requires coordinating caller
lifetimes and transient tessellation; this change intentionally does not claim it.

## Validation

Run `cargo test -p board-render --lib` (headless, no GUI launch). Tests cover
concurrent cache reuse, mutation isolation, font invalidation, LRU/point/capacity
accounting, negative results, exhaustive 3x3 raster neighborhoods, rings/branches,
exact tracing/simplification edge coverage, mesh-budget fallback triangle equality,
and stale mesh release. Ignored performance tests remain opt-in. These tests do
not constitute native GUI/CJK visual acceptance or a machine-level memory/latency
measurement.
