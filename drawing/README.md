# Drawing

`neo-drawing` is a transparent, always-on-top desktop annotation tool. **Version 0.0.1 is unreleased**; packaging and cross-platform acceptance testing are not complete.

## Build and run

Use a Rust 2024-compatible toolchain and native platform build tools. From the repository root:

```powershell
cargo build --release -p neo-drawing --locked
.\target\release\neo-drawing.exe --gui
```

- No arguments or `--help` shows help without opening a window; `--version` shows the version. Help and version output use stderr.
- `--headless` provides windowless JSON Lines communication. `--gui --hosted` waits for host configuration before creating a native window; it does not provide host services itself.
- Windows Drawing defaults to **Glow/OpenGL**; other platforms default to **wgpu**. Override with `NEO_DRAW_RENDERER=glow` or `wgpu`, or remove the variable to restore defaults. There is no automatic fallback.

## Writing and tools

- The toolbar provides pens, color/width/dash settings, erasing, selection, lines, shapes, undo/redo, and page controls. Secondary controls move into More on narrow windows.
- Click the toolbar pen or eraser once to select it, then again to open settings; no rapid double-click is needed. Other menus open with one click.
- Clear all in the eraser menu removes every object and connection on the current page as one undo step. Other pages are unchanged.
- Select the line tool and drag to draw. Erasing previews during a drag and commits on release as one undo step.
- Selection supports moving objects, editing shape vertices, and resizing circles/ellipses. Line endpoints can attach to shape vertices or edges and follow their targets.
- Select a screenshot/image or function plot, then drag its blue corner handles to resize. Images preserve their aspect ratio; plot frames resize independently on each axis without changing expressions or coordinate ranges. The opposite corner stays fixed; each drag is one undo step. While an image is selected, click its interior to open Agent authorization; its corner handles take priority over the image button.
- Documents support up to 500 pages. Use `Ctrl+S` to save, `Ctrl+Z` to undo, and `Ctrl+Y` to redo.

## Collapse and pointer passthrough

- Collapsing Drawing hides only its toolbar and panels. Ink remains visible; the native window is neither hidden nor resized. Expand restores the toolbar; Exit is separate.
- Blackboard instead collapses to a 220 x 64 logical-pixel expand/exit window. Both retain the document and undo history; neither collapse action confirms all windows are hidden for capture.
- Windows pointer passthrough preserves visible ink and restores pointer interaction near the toolbar. Equivalent behavior is not guaranteed on other platforms.

## Save, open, and export

- Enter explicit paths in the file panel to save/open documents, import PNG images, or export the current page as PNG/SVG.
- Saves include referenced images, but not undo history. Files with two-dimensional math objects use v2; the current application reads v1/v2, while older readers cannot read v2.
- Page export is not a desktop screenshot and does not mark a document as saved. Drawing exports use a white background.
- Missing images, missing PNG text fonts, or missing glyphs in supplied fonts cause export errors. SVG without supplied font outlines relies on viewer fonts.
- New, open, and exit operations protect unsaved changes. Exit offers save, discard, or cancel; a failed save leaves the document dirty.
- Disconnection triggers an attempted recovery save, with the path sent to stderr. Files are not reopened automatically, and disk failures can prevent recovery. Save normally rather than terminating the process.

## Windows screenshots and Agent

Local capture uses a built-in Win32 selection overlay and GDI pixel acquisition. It does not depend on Snipping Tool or the clipboard. Native Windows manual testing is still pending. Cancellation and errors restore the previous visibility intent after the capture thread exits, without an external-tool confirmation dialog.

- In standalone `--gui` mode, the screenshot action authorizes one capture. Drag a region in the built-in overlay; `Esc` or right-click cancels. No Snipping Tool or clipboard is used by this workflow.
- **Hide window during capture** defaults to `true` and lasts for the current run. Turn it off to keep Drawing and its ink visible, with the risk of including them in the image.
- A successful capture is inserted and selected automatically as one undo step only while the original document/page/revision remains valid. Click the image to open its Agent panel; drag to move it.
- Opening the panel is not authorization to send. Agent requests require a Neo host; standalone mode does not send them. Sending images and allowing answer write-back require separate authorization; write-back defaults to off. Images are not uploaded automatically, and ordinary answers are not converted into board objects.
- Hosted capture still requires host capture permission, classroom-safe mode to be off, and confirmed hiding of all owned windows. Local authorization does not change Session permissions or add an RPC; the hide option cannot bypass hosted requirements.
- Hosted tasks support manual cancellation; the GUI also requests cancellation after 60 seconds. A cancellation request does not prove capture has stopped: host confirmation is still required.
- GDI is not guaranteed to capture protected content or preserve HDR accurately. Not every stripped-down Windows installation is supported; native cancellation, DPI, and multiple displays require manual testing.

## Scope and further reading

Drawing has no mathematics panel. See [Blackboard](../blackboard/README.md) for optional handwriting recognition and limited mathematics. Recognition is off by default, and recognized input requires a click to calculate or plot. TexTeller requires explicit model preparation and loading. Numerical searches are not complete solvers; 3D shapes are two-dimensional projected wireframes.

Compilation and headless tests do not establish native transparency, passthrough, touch, or real host-service acceptance.

Project-owned code is [Apache-2.0](../LICENSE); third-party dependencies, fonts, runtimes, and models retain their own licenses. See the [distribution requirements](../RELEASING.md).

[Root README](../README.md) · [API reference](../api/DRAWING_API.md) · [Protocol](../api/PROTOCOL.md) · [Release notes](../CHANGELOG.md)
