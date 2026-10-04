# Releasing NeoRuntime-drawing

## v0.0.1 release policy

The intended public repository is **ChidcGithub/NeoRuntime-drawing**, with tag **v0.0.1** and GitHub's **pre-release** flag enabled. The repository owner has explicitly authorized this public source prerelease. This document describes the procedure; it is not evidence that a tag, upload, visibility change, or release publication has occurred.

This is the initial prerelease, not an upgrade from a previous published version. Application version 0.0.1 does not change JSON Lines protocol version 1 or document formats v1/v2. Use the bilingual [changelog](CHANGELOG.md) as the release notes and the [README](README.md) as the user entry point.

### Distribution boundary

- Publish source only through GitHub's automatically generated **Source code (zip)** and **Source code (tar.gz)** archives for the release tag.
- Do not attach executables, installers, portable packages, model weights, fonts, ONNX Runtime binaries, runtime DLLs, or dependency bundles. A successful local build is not permission to distribute its output.
- Keep `Cargo.toml`, `Cargo.lock`, project source, public documentation, API documents/examples, and `LICENSE` in the source tree. A lockfile describes dependencies; it does not bundle them.
- Third-party license/NOTICE obligations and clean-machine runtime requirements have not been fully reviewed. They block binary/model packaging, not the explicitly authorized project-source prerelease, provided the public source contents themselves are reviewed.
- Builds can still download dependencies. In particular, the current `ort` configuration enables native runtime download/copy support. “Source only” does not mean dependency-free, fully offline, or reproducibly built from source alone.

## 1. Review the public source boundary

Before making the repository public or publishing its tag:

- [ ] Confirm the repository owner/name, intended commit, version 0.0.1 in workspace metadata, tag `v0.0.1`, and prerelease designation.
- [ ] Review both the intended tree and reachable repository history for secrets, personal data, local documents, and unintended binaries. Source archives follow the tagged tree; a public repository also exposes its reachable history.
- [ ] Keep local-only development histories, agent instructions, license-review working notes, debug logs, `.dbg/`, `.workbuddy/`, `target/`, downloaded models, personal templates, board documents, and recovery files out of the public tree. Preserve the original local materials rather than deleting them as a documentation cleanup.
- [ ] Verify the actual tracked/tagged contents. Local ignore rules do not remove already tracked files or erase prior history; an ignore entry alone is not proof of exclusion. If excluded content is already tracked or present in history, resolve that separately with the owner before publication.
- [ ] Keep the API protocol/reference and JSON examples available. Check links against the intended public tree, not just files that happen to exist locally. The three root release documents must not depend on local-only notes.
- [ ] Review older application/API documents for stale release wording or links to excluded local notes. Do not treat the root-document update as a complete audit of every historical document.
- [ ] Confirm `LICENSE` is present and applicable to the project-owned source. Retain applicable third-party notices for any third-party material actually present in the source tree.

No neighboring Neo repository needs to be modified to publish this source release. Host services are external integration requirements, not included implementations.

## 2. Record validation without overstating it

Use a Windows MSVC environment with the C++ build tools and Windows SDK, a Rust edition 2024-compatible compiler, and the locked dependency set. There is no pinned Rust toolchain; record the actual Rust/Cargo versions, target, operating system, and commands used for each release validation run.

Suggested validation commands, to be run explicitly by the release maintainer:

```powershell
rustc --version --verbose
cargo --version
cargo test --workspace --locked
cargo build --release -p neo-drawing -p neo-blackboard --locked
.\target\release\neo-drawing.exe --version
.\target\release\neo-blackboard.exe --version
```

The version commands do not open a GUI and write to stderr. Add `--offline` only when all necessary inputs are available; Cargo's offline flag does not police network access by arbitrary native build scripts. Building locally does not add binaries to the release scope.

### Prepublication validation

Validation was rerun after removing the legacy local HTTP debug collector:

- Environment: Windows x64, `x86_64-pc-windows-msvc`, rustc **1.97.1**, Cargo **1.97.1**.
- `cargo test --workspace --locked --offline`: **540 passed, 12 ignored, 0 failed**.
- `cargo build --release -p neo-drawing -p neo-blackboard --locked --offline`: successful. These local binaries are not release assets.
- The staged source allowlist and local documentation links were checked before the initial commit. Build caches, model weights, local notes, diagnostics, and user data are excluded, not deleted locally.
- No GUI or desktop capture was started during validation. Clean-machine, native capture, and real host acceptance remain pending.

The same results and limits are recorded in the [changelog](CHANGELOG.md). Revalidate after subsequent code changes; ignored tests are not passing tests.

Ignored real-model, GPU, and performance tests are separate opt-in checks. Synthetic input and headless tests do not prove native rendering, actual desktop capture, model quality, or real-host integration.

### Manual checks that remain distinct from source publication

The following checks are not claimed complete. Outstanding checks must remain disclosed in the prerelease notes; do not label the release fully accepted merely because the default suite passes.

- [ ] On target Windows hardware, check both renderers as applicable, transparency, Chinese fonts, pen/eraser menus, shapes, page navigation, collapse/expand, touch, and pointer passthrough.
- [ ] Check image aspect-ratio resizing, independent plot-frame resizing, unchanged plot coordinate ranges, wheel zoom, and one-step undo/redo without accidental Agent actions or plot merging.
- [ ] Check save/reopen with images and two-dimensional math, PNG/SVG export, unsaved-change prompts, and failure/recovery behavior.
- [ ] With explicit consent to capture, check the built-in Win32/GDI region selector: hide/keep-visible options, insertion and selection, cancellation, soft timeout, negative monitor coordinates, and mixed DPI. Do not use real desktop acquisition as an unattended test.
- [ ] Check optional TexTeller preparation/loading, candidate review/manual correction, and calculate/plot-on-click. Verify ordinary drawing without downloaded model weights.
- [ ] If claiming real Neo integration, validate the actual host, per-request image authorization, separate answer write-back consent, stale-result rejection, and cancellation/hiding confirmation. Otherwise retain the external-host and unvalidated-integration caveats.

## 3. Publish the source prerelease

These are maintainer actions, not actions performed by updating this file. They can be completed through GitHub's repository and release interfaces.

1. Finish the public-content review and fresh validation record. Resolve accidental tracked local files before selecting the release commit; do not silently rewrite history or remove local evidence as part of a release-note edit.
2. Make the reviewed `ChidcGithub/NeoRuntime-drawing` repository public under the owner's authorization, after checking the exposure of its tree and reachable history.
3. Create or select tag `v0.0.1` at the reviewed commit. Do not move or overwrite an existing published tag without a separate decision.
4. Draft a release titled **v0.0.1 — Initial source prerelease**. Use the English and Chinese v0.0.1 notes from `CHANGELOG.md`; retain source-only scope, external host/model requirements, validation provenance, and known limits.
5. Enable **Set as a pre-release**. Do not describe it as stable or production-ready, and do not mark it as the latest stable release.
6. Leave custom release assets empty. GitHub provides the source ZIP and tar.gz archives; do not upload a locally built executable or a model/runtime package as a convenience asset.
7. Preview the release notes and check links and archive contents against the tag. Confirm that public-facing documentation does not imply completed native or distribution acceptance.
8. Publish, then verify the public repository, tag, prerelease badge, bilingual notes, automatic source downloads, and absence of attached binary/model/dependency assets. Inspect the downloaded source archives to confirm the expected public tree.

Do not claim a release URL is live until publication has actually been verified. If an archive exposes unintended content, stop distribution and coordinate remediation with the owner rather than merely adding an ignore rule afterward.

## 4. Requirements for any future binary or model distribution

These are follow-up gates, not assets included in v0.0.1:

- Inventory the actual package contents and statically/dynamically linked components, including ONNX Runtime, compiler runtimes, fonts, and any model weights. Review their individual licenses, notices, attribution, redistribution conditions, and provenance.
- Confirm the project-owned code's licensing scope and any applicable copyright/NOTICE statements with the rights holders. Do not invent a copyright holder, year, or attribution. The standard Apache license appendix is a template, not an assertion of project ownership.
- Validate runtime/DLL requirements on a clean Windows machine. Do not infer “single executable” portability from a developer-machine launch or static linking of one dependency.
- Assemble any future package from an explicit allowlist in a clean staging directory, include the required legal materials and setup instructions, and exclude private/local data and build caches.
- Complete package-specific acceptance, checksums, and download/run verification; obtain approval for the expanded distribution scope before uploading it.

Project-owned workspace packages use [Apache-2.0](LICENSE). That license does not replace the licenses of third-party dependencies, fonts, models, or runtimes.
