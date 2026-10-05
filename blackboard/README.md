# Blackboard

`neo-blackboard` is a multipage writing board with an opaque textured background, optional handwriting recognition, and limited mathematics. **Version 0.0.1 is a source-only prerelease**; binary packaging and cross-platform acceptance testing are not complete.

## Build and run

Use a Rust 2024-compatible toolchain and native platform build tools. From the repository root:

```powershell
cargo build --release -p neo-blackboard --locked
.\target\release\neo-blackboard.exe --gui
```

- No arguments or `--help` shows help without opening a window; `--version` shows the version. Help and version output use stderr.
- `--headless` provides windowless JSON Lines communication. `--gui --hosted` waits for host configuration before creating a native window; host services are separate.
- Blackboard defaults to **wgpu**, including on Windows; Windows Drawing defaults to Glow/OpenGL. Override with `NEO_DRAW_RENDERER=wgpu` or `glow`, or remove the variable to restore defaults. There is no automatic fallback.

## Writing and navigation

- The toolbar provides pens, color/width/dash settings, erasing, selection, lines, shapes, undo/redo, and page management for up to 500 pages. More holds secondary controls on narrow windows.
- Click the toolbar pen or eraser once to select it, then again to open settings; no rapid double-click is needed. Other menus open with one click.
- Clear all removes every object and connection on the current page as one undo step, without changing other pages. Drag erasing previews first and commits on release as one undo step.
- Select the line tool and drag to draw; use selection to move objects and edit shapes.
- Select a screenshot/image or function plot and drag a blue corner handle to resize, keeping the opposite corner fixed. Images preserve aspect ratio; plot frames allow independent width/height changes while retaining expressions and coordinate ranges. Each drag is one undo step; scrolling over a selected plot still zooms its coordinate range. Selected images keep interior-click Agent authorization without an overlapping corner button.
- Collapsing Blackboard reduces the same native window to a 220 x 64 logical-pixel expand/exit control, hiding the board surface. Expanding restores its geometry and content; undo history is retained.
- Drawing instead collapses only its toolbar and keeps ink visible. Neither collapse action confirms all windows are hidden for capture.

## Save, open, and export

- Enter explicit paths to save/open documents, import PNG images, or export the current page as PNG/SVG. `Ctrl+S` saves; `Ctrl+Z` and `Ctrl+Y` undo and redo.
- Saves retain referenced images but not undo history. The application reads v1/v2/v3 and saves the smallest required version: any `Handwritten` object requires v3; otherwise any `Math` object requires v2; all other documents use v1. Images determine whether resources are packaged, not the version. Older v1/v2 readers cannot read v3; v1-only readers cannot read v2.
- Page export is not a desktop screenshot or a document save. Missing images, PNG text fonts, or glyphs in supplied fonts cause errors; SVG without supplied font outlines relies on viewer fonts.
- New, open, and exit operations protect unsaved changes. A failed save leaves the document dirty. Disconnection attempts a recovery save and reports its path to stderr; recovery is not automatically reopened or guaranteed against disk failure.

## Handwriting recognition and models

- **Handwriting recognition is off by default.** Enable it explicitly in the mathematics panel for the current run.
- A 2.5-second writing pause triggers background recognition only, not calculation, plotting, panel opening, or write-back; it is not a completion deadline.
- Review the candidate, then click its calculator or plot icon. The correction icon opens input for editing without calculating. Busy tasks cannot be submitted again.
- Successful calculation keeps the original ink and adds a result as one undo step. Candidates are cached only in memory; edits to source ink or nearby objects can invalidate them, and reopening a document clears them.
- The default offline template backend is limited, not general handwriting recognition. Optional TexTeller requires selecting the backend, entering an absolute model directory, and explicitly loading/reloading it before enabling recognition.
- Models are not guaranteed to be present with the source or application. The GUI neither downloads nor automatically discovers and loads models. To download and verify them explicitly from the repository root:

```powershell
python crates/board-hwr/download_texteller.py --dir models/texteller
```

The download is approximately 1.25 GB and requires network access. Python is used for installation; inference runs locally. Additional setup notes are maintained in the local-only `api/MODELS.md`, excluded from current public checkouts. A directory's existence does not establish model completeness or loading. Review all recognition results; unsupported or ambiguous LaTeX is rejected rather than silently removed.

For low-memory CPUs, use a separately converted INT8 directory with the [offline converter](../crates/board-hwr/prepare_texteller_int8.py). Run it with `--help` for options; conversion and benchmark notes remain in the local-only `api/MODELS.md`. Synthetic benchmark results do not establish recognition accuracy or target-machine acceptance. The GUI prefers a complete `models/texteller-int8` directory suggestion but still requires explicit loading. Use **Unload model**, or switch to templates, to release it after ongoing work drains. Recognition Off alone retains the model. Reload is unavailable until old inference/loading work has finished. No whole-machine 3 GB guarantee is made.

## Personal handwritten answers (experimental)

- In the mathematics panel, **个人笔迹答案（实验性）** enables local numeric style learning and personalized answers by default, independently of HWR, which remains off by default. Calculation and insertion still require a click. Only newly committed local pen strokes feed automatic style statistics; optional exact-glyph sampling needs a manual character label. See the [full workflow and privacy limits](../README.md#personal-handwritten-answers-experimental).
- Preview generation and profile JSON save/load share one background slot with no queue. Stale preview/load results are discarded after relevant source, profile, document/page, panel, or font changes; a valid preview reuses cached render meshes on idle frames and does not modify the document or learning statistics.
- Saving/loading requires an explicit local path and overwrite/replacement confirmation. An authorized save writes the profile snapshot captured at the click: closing the panel or changing the profile does **not** interrupt that in-progress write, and later edits are not included. There is no automatic profile persistence or loading.
- Same-frame keyboard edits, cancellation, and setting changes are processed before accepting ready background answers. Sampling-area `Ctrl+Z` takes priority over document undo while a sample draft is active and text input is not focused.
- Synthesized operators and fraction bars share a math axis; spacing accounts for the clamped pen width and bounded miter joins. Lowercase `o` retains a smaller body than `0`, and `x` uses a hooked form distinct from `×`; these are readability improvements, not an accuracy guarantee. Frozen answers remain whole objects and do not change with later profile edits.
- Font-derived skeletons share a bounded cache per loaded font generation (128 characters, 65,536 points, 2 MiB of accounted entry payload); replacing the font starts a new generation. This is separate from the page renderer's 64 MiB retained mesh-buffer budget, neither of which caps total process or machine RAM. Committed pages use document-revision caching, while temporary previews stay separate; font-file reads are bounded to 64 MiB and validated before installation.
- Native GUI readability, complex Chinese glyphs, keyboard/cancellation behavior, and target i5/3 GB performance/memory still need manual validation.

## Mathematics scope

- Manual controls calculate, insert results, plot, and set coordinate bounds. Constants support bounded exact fractions and square roots; other supported operations may return approximate results. Overflow, domain errors, and budget limits produce errors.
- Supported algebra includes limited x/y polynomials, linear/quadratic equations in one variable, and two-variable linear systems. `simplify`, `diff`, and `integrate` are limited to polynomials of total degree at most 12, not a full CAS or general symbolic integration.
- Equation/polynomial operations use floating-point algorithms. GUI quadratic-system, root, and intersection searches are bounded; no candidate does not mean no solution. Numerical derivatives and definite integrals are not general symbolic solvers.
- Plots support explicit functions and single implicit x/y polynomial equations with numeric coefficients and total degree at most two, such as `y^2=x`. Arbitrary implicit, higher-degree, and parametric plots are unsupported.
- Select a plot and use **求点** for supported curve/curve and coordinate-axis intersections within the plot's x/y bounds. Straight lines accept `y=2x+1`, `f(x)=2x+1`, `y-x=1`, and manually plotted vertical lines such as `x=2`; lines can be mixed with supported explicit functions. Parallel lines yield no pairwise point; coincident lines sharing a segment within the rectangle report non-discrete intersections; if they only touch one rectangle corner, that single point is returned.
- Line/line intersections use analytic formulas in **f64 floating-point**, not exact rational arithmetic; near-singular systems, domain errors, overflow, or failed residual checks can produce errors. Vertical-line/explicit-function pairs evaluate at the line's x; other explicit pairs use bounded, incomplete numerical search, with candidates checked against both original expressions. Boundary clipping accepts at most four ULPs of rounding, not the root-finding tolerance, and rechecks clamped points at machine-precision scale. This preserves decimal boundary cases such as `0.1*x+0.2` at `x=1`, `y_max=0.3`, without treating genuinely out-of-range points as in bounds. General implicit quadratics such as `y^2=x` or `x^2+y^2=9` remain unsupported for 求点: an unsupported curve in the inspected set prevents the whole-plot search, rather than returning only the explicit subset. At most the first 16 curves are inspected, with an explicit limit diagnostic for larger plots.
- These are GUI/library features, not new RPCs or an expansion of `math.calculate`. 3D shapes are two-dimensional projected wireframes, not rotatable solids.

## Windows screenshots and Agent

Local capture uses a built-in Win32 selection overlay and GDI pixel acquisition. It does not depend on Snipping Tool or the clipboard. Native Windows manual testing is still pending. Cancellation and errors restore the previous visibility intent after the capture thread exits, without an external-tool confirmation dialog.

- The standalone screenshot action authorizes one local capture. Drag a region in the built-in overlay; `Esc` or right-click cancels. This workflow uses neither Snipping Tool nor the clipboard.
- **Hide window during capture** defaults to `true` for the current run. Turning it off keeps Blackboard and its ink visible, so they may be captured.
- Success inserts and selects the image as one undo step only if the original document/page/revision is still valid. Click the image to open its Agent panel; drag to move it. Opening the panel does not send anything.
- Agent requests require a Neo host; standalone mode does not send them. Sending images and allowing answer write-back require separate authorization, with write-back off by default. Images are not uploaded automatically; ordinary answers do not become board objects automatically.
- Hosted capture still requires host permission, classroom-safe mode to be off, and confirmed hiding of all owned windows. The local option does not change Session permissions, bypass hosted requirements, or add an RPC.
- Hosted tasks can be cancelled manually; the GUI also requests cancellation after 60 seconds. The request is not proof that capture has stopped; host confirmation remains necessary.
- Compatibility with every stripped-down Windows installation is not guaranteed. GDI may not capture protected content or reproduce HDR accurately. Native cancellation, DPI, multiple displays, and real host integration still require manual validation.

## License and documentation

Project-owned code is [Apache-2.0](../LICENSE); third-party dependencies, fonts, runtimes, and models retain their own licenses. See the [distribution requirements](../RELEASING.md).

[Root README](../README.md) · [Release notes](../CHANGELOG.md) · [Drawing](../drawing/README.md)

The API reference and protocol are maintained in the local-only `api/` directory. Coordinate with the maintainer for host integration documentation.
