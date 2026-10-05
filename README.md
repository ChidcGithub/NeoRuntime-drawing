# NeoRuntime-drawing

Rust desktop tools for classroom writing and screen annotation, with standalone interfaces and a JSON Lines integration layer for a Neo host.

**v0.0.1 is an initial, source-only prerelease**, not a production-ready binary distribution. The release uses GitHub's automatically generated source archives; no executables, model weights, dependency bundles, or runtime packages are attached. Native Windows and real-host acceptance testing are not complete. See the [release notes](CHANGELOG.md) for scope and known limitations.

## Overview

- **Drawing (`neo-drawing`)**: a transparent, always-on-top annotation surface with pens, erasing, shapes, and Windows pointer passthrough.
- **Blackboard (`neo-blackboard`)**: a multipage board with an opaque textured background, function plots, bounded mathematics, and optional local handwriting recognition.
- **Shared tools**: selection and geometry editing, line-to-shape connections, undo/redo, up to 500 pages, document save/open, PNG import, and current-page PNG/SVG export.
- **Integration**: headless and hosted GUI modes implement the documented session protocol. Neo host capture and Agent services are separate and are not included here.

The current GUI, command-line help, and many status/error messages are in **Chinese**. This English README does not imply an English-localized interface.

## Build on Windows

Install Rust through rustup with an MSVC toolchain, plus Visual Studio Build Tools with the **Desktop development with C++** workload and a Windows SDK. Use a current Rust toolchain that supports **edition 2024** and the locked dependencies.

There is no pinned `rust-toolchain` or `rust-toolchain.toml` in this repository. `Cargo.lock` fixes dependency resolution, not the compiler, native SDK, or complete build environment; edition support alone is not a tested minimum Rust version.

For Windows x64, run from the repository root in PowerShell:

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc
cargo +stable-x86_64-pc-windows-msvc build --release -p neo-drawing -p neo-blackboard --locked
```

Builds may access the network for crates and native dependencies. The current `ort` dependency enables ONNX Runtime binary download/copy support even when you do not use neural recognition. Add `--offline` only when all required Cargo and native build inputs are already available; it is not a guarantee that dependency build scripts cannot access the network. No model weights are needed for ordinary drawing or template recognition.

After building, choose one application:

```powershell
.\target\release\neo-drawing.exe --gui
# Alternatively:
.\target\release\neo-blackboard.exe --gui
```

These commands build and run locally; they do not establish that an executable is self-contained or cleared for redistribution.

## Launch modes and help

| Arguments | Behavior |
|---|---|
| None, `--help`, or `-h` | Show help without opening a window |
| `--version` or `-V` | Show the application version without opening a window |
| `--gui` | Open the standalone interface |
| `--gui --hosted` | Run under a Neo host; await `configure` before creating a native window |
| `--headless` | Run the windowless JSON Lines interface |

Help, version output, and logs use stderr. Protocol stdout is reserved for JSON Lines. Starting the application does not automatically capture the desktop or authorize Agent requests.

### Renderer selection

Windows Drawing defaults to **Glow/OpenGL**. Blackboard and other platforms default to **wgpu**. To override the renderer for the current PowerShell session:

```powershell
$env:NEO_DRAW_RENDERER = "wgpu" # Or "glow"
# Remove the override to restore the platform/application default:
Remove-Item Env:NEO_DRAW_RENDERER -ErrorAction SilentlyContinue
```

Launch the chosen application after setting an override, before removing it. There is no automatic renderer fallback; graphics-driver and native transparency behavior still require validation on the target machine.

## Everyday use

- Click the toolbar pen or eraser once to select it, then again to open settings. This is not a timed double-click. Other menus open with one click; narrow windows move secondary controls into More.
- Drag erasing previews changes and commits on release as one undo step. Clear all in the eraser menu removes every object and connection on the current page as one undo step, leaving other pages intact.
- Use selection to move objects and edit supported shape geometry. Lines can attach to shape vertices or edges and follow their targets.
- Select an image, screenshot, or function plot and drag a corner handle to resize it, keeping the opposite corner fixed. Images preserve aspect ratio. Plot frames allow independent width/height changes without changing expressions or coordinate ranges. Each completed resize is one undo step; scrolling over a selected plot still zooms its coordinate range.
- `Ctrl+S` saves, `Ctrl+Z` undoes, `Ctrl+Y` redoes, and `Esc` cancels the current interaction.
- Collapsing Drawing hides the toolbar and panels, not its ink. Collapsing Blackboard reduces it to a small expand/exit window. Neither action confirms that all windows are hidden for capture.

## Windows screenshots and Agent

Standalone capture uses a **built-in Win32 selection overlay and GDI pixel acquisition**, not Snipping Tool or the clipboard. The application does not automatically upload captured images.

- Clicking the standalone screenshot action authorizes one capture. Drag a region; `Esc` or right-click cancels. Loss of focus or display-layout changes also cancel selection.
- **Hide window during capture** defaults to `true` for the current run. Turning it off keeps the board and ink visible, so they may be included in the image. Pixels are acquired after selection ends, not from a frozen desktop snapshot.
- Success, failure, cancellation, and the 60-second cooperative soft timeout wait for the local capture thread and overlay to finish cleanup before restoring the previous visibility intent. This is not a hard deadline, and no external-tool confirmation dialog is used.
- A successful capture is inserted and selected as one undo step only if the original document/page/revision remains valid. Click the image interior to open its Agent panel, drag to move it, or use its corner handles to resize.
- Opening the Agent panel sends nothing. Agent requests require a **separate Neo host** and explicit authorization to send images. Answer write-back needs separate authorization and defaults to off; ordinary answers do not automatically become board content. The host's service may have its own network, privacy, and cost implications.
- Hosted capture still requires host permission, classroom-safe mode to be off, and confirmed hiding of all owned windows. The standalone hide option does not relax these requirements or add an RPC. A host cancellation request is not proof that capture has stopped; host confirmation remains necessary.

Native screenshot acceptance is pending. Automated capture tests use synthetic inputs, not desktop acquisition. GDI may not capture protected content or reproduce HDR accurately; mixed DPI, multiple displays, cancellation, and stripped-down Windows installations need manual testing.

## Blackboard mathematics and handwriting

Drawing has no mathematics panel. Blackboard supports manual calculation, result insertion, explicit function plots, and limited implicit curves.

- Exact constant arithmetic supports bounded fractions and square roots; other supported constant operations may return approximate results. Polynomial algebra, equations, and numerical searches have separate limits and may use floating-point arithmetic.
- `simplify`, `diff`, and `integrate` cover limited polynomials, not a full computer algebra system. GUI root, intersection, and quadratic-system searches are bounded: no candidate does not mean no solution.
- Implicit plots support single x/y polynomial equations with numeric coefficients and total degree at most two. Arbitrary implicit, higher-degree, and parametric plots are unsupported.
- Plot **求点** supports straight lines (`y=2x+1`, `f(x)=2x+1`, `y-x=1`, and manually plotted `x=2`), their coordinate-axis intersections, and mixtures with supported explicit functions. Parallel lines have no pairwise point; coincident lines sharing a segment within the rectangle report non-discrete intersections, while contact at only one rectangle corner returns that single point. Line/line formulas are analytic but use **f64 floating-point**, not exact arithmetic; vertical-line/explicit pairs evaluate at the fixed x, while other explicit pairs retain bounded numerical search with residual checks against both original expressions. Boundary clipping absorbs at most four ULPs of rounding and rechecks any clamped point at machine-precision scale; it does not expand the rectangle by the root-finding tolerance. General implicit quadratics remain unsupported for intersections: any unsupported curve among the inspected first 16 prevents the whole-plot search, not just that curve's pairs. Larger plots receive a limit diagnostic. No RPC or `math.calculate` scope is added.
- **Handwriting recognition is off by default.** Enabling it allows a 2.5-second writing pause to trigger recognition only. Review or manually correct the candidate, then click its calculate or plot icon. Recognition does not automatically calculate, plot, or write back.
- The default offline template recognizer is limited, with explicit personal-template learning/save/load. Optional TexTeller recognition uses local Rust/ONNX Runtime inference and separately prepared models. Select its backend, provide an absolute model directory, and explicitly load it; the GUI does not download or automatically load models.
- Low-memory CPU use: prepare a separate dynamic INT8 model directory with the [offline converter](crates/board-hwr/prepare_texteller_int8.py). Local synthetic benchmarks reduced model-process peak working set from about 1.3 GiB to 0.48 GiB; this is **not** full-GUI or i5/3 GB whole-machine acceptance. Setup and measurement notes are kept in the local-only `api/MODELS.md`, which is not included in public checkouts. The GUI provides model unloading and prevents overlapping reload/inference residency.
- The optional [model preparation script](crates/board-hwr/download_texteller.py) requires Python and network access and downloads approximately 1.25 GB. This is a separate opt-in action, not part of the source release or ordinary drawing setup. Model and runtime licensing must be reviewed separately before redistribution.

Recognition candidates are held in memory and can become invalid after source or nearby-content edits. Successful calculation preserves the original ink and adds a result as one undo step. Recognition scores are not correctness guarantees.

### Personal handwritten answers (experimental)

In Blackboard's mathematics panel, expand **个人笔迹答案（实验性）**. Personal style learning and answers are **enabled by default**, independently of handwriting recognition: learning works with HWR **Off**, which remains its default. The enabled setting is session-only and returns to the default on restart. Calculating or inserting a result still requires an explicit click.

- While enabled, fresh, successfully committed local **`Tool::Pen` strokes** can update numeric style statistics without labels, confirmation, or prior manual samples. A bounded window of **31 numeric observations**, rolling medians, and capped exponentially weighted moving averages (EWMA) estimate pen width, slant, aspect, and speed. Only numeric statistics are retained by automatic learning, not unlabeled stroke examples. The writing-like-stroke filter is heuristic: it cannot reliably distinguish every diagram stroke from handwriting. This is not character recognition or automatic character segmentation, and does not learn an exact alphabet or clone unseen Chinese characters.
- Imports, old documents, host operations, undo/redo, and generated answers do not feed automatic learning. Generated glyphs/output are not stored as profile examples. This feature adds no neural models, model training/downloads, cloud service, or network access.
- Optional exact-glyph sampling is nested under **可选：精确字形采样**. Only this optional workflow requires a manually entered single non-whitespace, non-control character label; draw the sample and click **加入精确字形**, with no label-confirmation checkbox. The canvas uses **128×128 local coordinates**, displayed at **256×256**, with a top guide at **y=24** and baseline at **y=96**. GUI samples use constant pressure **1**, not native stylus pressure. Limits remain **96 characters**, **3 variants per character**, **65,536 total sample points**, and **16 strokes / 2,048 points per sample**.
- Style statistics and optional samples stay in memory unless explicitly saved/loaded through a local JSON path (maximum **8 MiB**). Profile format **2** also reads format **1**; this is separate from document versions. Saving requires overwrite permission and loading requires confirmation to replace the in-memory profile; destructive-action confirmations remain. There is no automatic persistence, load, or profile-directory scan.
- Preview generation and profile JSON I/O share one background slot, without a queue. Relevant context changes invalidate preview/load results; a valid preview reuses cached render meshes on idle frames without changing the document or learning statistics. **An authorized save writes the click-time profile snapshot in the background; closing the panel or editing the profile does not cancel an in-progress write.** Later edits are not included or automatically persisted.
- Keyboard edits, Esc, and relevant setting changes take effect before already-ready background answers are accepted in the same frame; held input does not indefinitely block result draining. Sampling-area `Ctrl+Z` precedes document undo while a draft is active and text input is not focused.
- Synthesized fraction bars and operators share a math axis; spacing includes clamped pen width and bounded miter joins. Lowercase `o` is smaller than `0`, and the hooked `x` differs from `×`; this improves readability without promising unambiguous recognition.
- Only new **Math/Text answers from the local mathematics worker** are eligible; host Agent write-back is not personalized. `render_adaptive` preserves optional exact samples and supplies missing characters from project-authored math templates and some extra procedural symbols, otherwise from an already explicitly loaded local font. Font glyphs are rasterized and skeletonized within **96×96 pixels**, then synthesized glyphs receive the style transform. The renderer performs no hidden filesystem font scans; the GUI's existing fixed Windows Chinese-font paths are unchanged. If a needed font/glyph is unavailable, a glyph is too complex, or a generation budget is exceeded, the **entire answer keeps standard rendering**, with a diagnostic—never partial handwriting mixed with standard output. Fraction bars, radical signs, and radical overbars remain procedural structural lines, not the user's traced glyphs.

Font-derived skeletons (including failure results) are cached per loaded font generation, shared by its snapshots, with limits of **128 characters, 65,536 points, and 2 MiB of accounted entry payload**. The cache stores unstyled glyphs, not personal style; replacing the font starts a new generation, while outstanding snapshots can retain the old one.

Each successful handwritten answer is **one object**: select, move, or delete it as a whole, with undo/redo. When selected, its standard text can be copied from the mathematics panel using **复制所选答案标准文字**. Saving and PNG/SVG export use frozen strokes; reopening or rendering the answer does not require the profile, and subsequent profile edits do not change existing answers. Each answer is limited to **4,096 UTF-8 text bytes, 1,024 strokes, and 32,768 points**. The frozen whole-answer object and v3 document format are unchanged. Font-derived centerlines approximate printed glyphs, not the original pen trajectory or stroke order. Complex Chinese output has not been visually tested; behavior on the real target machine and i5/3 GB performance or memory use are not guaranteed.

**Privacy:** saved documents (including recovery saves) contain the personal strokes used in their answers, although they do **not** embed the whole profile. Copied or shared saved documents and exports disclose the rendered personal style. Treat documents, exports, and separately saved profile JSON as personal data before copying or sharing them.

## Documents, recovery, and limitations

- Save/open and PNG import/export use explicit paths in the file panel. Exporting a page is neither a desktop screenshot nor a document save.
- Unsaved changes require confirmation before exit or document replacement. Failed saves leave the document dirty.
- Saved documents retain referenced images and connections, but not undo history or recognition candidates. The application reads document formats v1/v2/v3: any handwritten answer requires v3; otherwise two-dimensional math objects require v2, and documents with neither use v1. Images only determine whether resources are packaged, not the version. Older v1/v2 readers cannot read v3 files; v1-only readers cannot read v2 files. JSON Lines protocol version remains 1, with no new RPC.
- On disconnection, the application attempts a recovery save under `NeoRuntime-drawing/recovery` in `LOCALAPPDATA`, falling back to the system temporary directory. Paths or errors are reported to stderr. Recovery files are not reopened automatically, and disk failures can prevent recovery.
- History, object counts, image resources, computation, and recognition are bounded. Committed pages use document-revision render caching; temporary gesture previews do not reuse the committed-content key. The page renderer's **64 MiB retained mesh-buffer budget is not a global RAM cap**: source snapshots, scenes, resources, submitted frames, and other application state are separate. GUI font-file reads are bounded to 64 MiB and validated before installation. 3D shapes are projected wireframes, not solid models.
- PNG text export requires suitable fonts; missing images, fonts, or required glyphs cause errors. SVG without embedded font outlines depends on the viewer's fonts.
- Native rendering, transparency, touch, pointer passthrough, multiple displays, clean-machine runtime requirements, and real Neo host integration are not fully validated. Equivalent Windows behavior is not promised on other platforms.

## Documentation and validation

- [Changelog: English and Chinese release notes](CHANGELOG.md)
- [Source release procedure and distribution boundaries](RELEASING.md)
- [Windows application workflow](.github/workflows/windows-app.yml) and [INT8 model workflow](.github/workflows/texteller-int8.yml) (manual; reports only by default)
- [Drawing guide](drawing/README.md) and [Blackboard guide](blackboard/README.md)

Protocol, API, model setup, and Neo integration handoffs are maintained in the local-only `api/` directory. They are intentionally excluded from current public checkouts; coordinate with the maintainer for integration documentation.

### Repository layout

| Path | Purpose |
|---|---|
| `drawing/`, `blackboard/` | Application entry points, guides, and integration tests |
| `crates/` | Shared Rust libraries, unit tests, and handwriting-model tools |
| `scripts/` | Packaging tools, license collection tooling, and packaging tests |
| `.github/workflows/` | Manually triggered application and model build workflows |
| `distribution/legal/` | Third-party notices, provenance, and review evidence; not a disposable cache |
| `api/` | Local-only protocol and integration documentation (ignored) |
| `models/` | Locally prepared model weights (ignored) |
| `target/` | Generated Cargo build output (ignored) |

Keep source, tests, and supporting tools together in their existing packages. Build output and Python `__pycache__/` directories are regenerable; downloaded models, local documents, and review evidence are not treated as disposable files.

To run the default automated suite from the repository root:

```powershell
cargo test --workspace --locked
```

Ignored model, GPU, and performance tests require explicit execution. The v0.0.1 test/build record in the changelog is historical, not validation of Unreleased changes. Native GUI checks of keyboard/result ordering, previews, handwriting readability, and straight-line 求点, plus target i5/3 GB performance and whole-machine memory measurements, remain pending; neither automated tests nor a successful build establish complete native or distribution acceptance.

## License

Project-owned workspace packages are licensed under [Apache-2.0](LICENSE). Third-party dependencies, fonts, runtimes, and models retain their own licenses. This source-only prerelease does not bundle those dependencies and does not certify their redistribution requirements as complete. Binary/model distribution is deferred pending third-party license, attribution, and runtime review. See the [release procedure](RELEASING.md#4-requirements-for-any-future-binary-or-model-distribution) for distribution requirements.

## Acknowledgements

This project builds on the work of the following open-source projects and their contributors:

- [egui / eframe](https://github.com/emilk/egui) for the user interface and desktop framework, and [winit](https://github.com/rust-windowing/winit) for window integration.
- [wgpu](https://github.com/gfx-rs/wgpu) and [glow](https://github.com/grovesNL/glow) for graphics backends.
- [Serde](https://github.com/serde-rs/serde) and [serde_json](https://github.com/serde-rs/json) for serialization.
- [tiny-skia](https://github.com/linebender/tiny-skia), [image](https://github.com/image-rs/image), and [png](https://github.com/image-rs/image-png) for image processing and export.
- [ort](https://github.com/pykeio/ort), [ONNX Runtime](https://github.com/microsoft/onnxruntime), and [tokenizers](https://github.com/huggingface/tokenizers) for local neural inference and tokenization.
- [TexTeller](https://github.com/OleehyO/TexTeller) for the optional handwriting-recognition model integration.
- The Hack, Noto Emoji, Ubuntu, and emoji-icon-font contributors whose fonts are included by egui's default-font configuration when building the applications.

These acknowledgements do not imply sponsorship or endorsement. Each upstream component retains its own license; acknowledgements do not replace required copyright, license, or NOTICE materials. The complete locked dependency resolution is recorded in `Cargo.lock`.
