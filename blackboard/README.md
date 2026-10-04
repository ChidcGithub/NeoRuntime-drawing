# Blackboard

`neo-blackboard` is a multipage writing board with an opaque textured background, optional handwriting recognition, and limited mathematics. **Version 0.0.1 is unreleased**; packaging and cross-platform acceptance testing are not complete.

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
- Saves retain referenced images but not undo history. Two-dimensional math objects require v2 files; the current application reads v1/v2, while older readers cannot read v2.
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

The download is approximately 1.25 GB and requires network access. Python is used for installation; inference runs locally. See [model setup, provenance, and licensing](../api/MODELS.md). A directory's existence does not establish model completeness or loading. Review all recognition results; unsupported or ambiguous LaTeX is rejected rather than silently removed.

## Mathematics scope

- Manual controls calculate, insert results, plot, and set coordinate bounds. Constants support bounded exact fractions and square roots; other supported operations may return approximate results. Overflow, domain errors, and budget limits produce errors.
- Supported algebra includes limited x/y polynomials, linear/quadratic equations in one variable, and two-variable linear systems. `simplify`, `diff`, and `integrate` are limited to polynomials of total degree at most 12, not a full CAS or general symbolic integration.
- Equation/polynomial operations use floating-point algorithms. GUI quadratic-system, root, and intersection searches are bounded; no candidate does not mean no solution. Numerical derivatives and definite integrals are not general symbolic solvers.
- Plots support explicit functions and single implicit x/y polynomial equations with numeric coefficients and total degree at most two, such as `y^2=x`. Arbitrary implicit, higher-degree, and parametric plots are unsupported.
- Intersection search is not supported for implicit or mixed explicit/implicit plots. 3D shapes are two-dimensional projected wireframes, not rotatable solids.

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

[Root README](../README.md) · [API reference](../api/DRAWING_API.md) · [Protocol](../api/PROTOCOL.md) · [Release notes](../CHANGELOG.md) · [Drawing](../drawing/README.md)
