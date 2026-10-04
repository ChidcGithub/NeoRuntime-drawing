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
- Implicit plots support single x/y polynomial equations with numeric coefficients and total degree at most two. Arbitrary implicit, higher-degree, and parametric plots are unsupported; implicit/mixed-plot intersection search is not supported.
- **Handwriting recognition is off by default.** Enabling it allows a 2.5-second writing pause to trigger recognition only. Review or manually correct the candidate, then click its calculate or plot icon. Recognition does not automatically calculate, plot, or write back.
- The default offline template recognizer is limited, with explicit personal-template learning/save/load. Optional TexTeller recognition uses local Rust/ONNX Runtime inference and separately prepared models. Select its backend, provide an absolute model directory, and explicitly load it; the GUI does not download or automatically load models.
- The optional [model preparation script](crates/board-hwr/download_texteller.py) requires Python and network access and downloads approximately 1.25 GB. This is a separate opt-in action, not part of the source release or ordinary drawing setup. Model and runtime licensing must be reviewed separately before redistribution.

Recognition candidates are held in memory and can become invalid after source or nearby-content edits. Successful calculation preserves the original ink and adds a result as one undo step. Recognition scores are not correctness guarantees.

## Documents, recovery, and limitations

- Save/open and PNG import/export use explicit paths in the file panel. Exporting a page is neither a desktop screenshot nor a document save.
- Unsaved changes require confirmation before exit or document replacement. Failed saves leave the document dirty.
- Saved documents retain referenced images and connections, but not undo history or recognition candidates. The application reads document formats v1/v2; two-dimensional math objects require v2, which older v1-only readers cannot read. JSON Lines protocol version remains 1.
- On disconnection, the application attempts a recovery save under `NeoRuntime-drawing/recovery` in `LOCALAPPDATA`, falling back to the system temporary directory. Paths or errors are reported to stderr. Recovery files are not reopened automatically, and disk failures can prevent recovery.
- History, object counts, image resources, computation, and recognition are bounded. 3D shapes are projected wireframes, not solid models.
- PNG text export requires suitable fonts; missing images, fonts, or required glyphs cause errors. SVG without embedded font outlines depends on the viewer's fonts.
- Native rendering, transparency, touch, pointer passthrough, multiple displays, clean-machine runtime requirements, and real Neo host integration are not fully validated. Equivalent Windows behavior is not promised on other platforms.

## Documentation and validation

- [Changelog: English and Chinese release notes](CHANGELOG.md)
- [Source release procedure and distribution boundaries](RELEASING.md)
- [Communication protocol](api/PROTOCOL.md) and [API reference](api/DRAWING_API.md)
- [Drawing guide](drawing/README.md) and [Blackboard guide](blackboard/README.md)

To run the default automated suite from the repository root:

```powershell
cargo test --workspace --locked
```

Ignored model, GPU, and performance tests require explicit execution. The last recorded result and its limits are documented in the changelog; neither automated tests nor a successful build establish complete native or distribution acceptance.

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
