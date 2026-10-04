# Rust dependency legal-text collection — pending review

This is a **conservatively overinclusive collection, not an SBOM, legal approval,
or an all-cleared release declaration**. It prepares evidence for a formal
build; it does not certify that build or authorize publication. No
`REVIEWED.md` is created.

## Scope and coverage

Input: the existing `.dbg/license-build-metadata.json`, supplied as
Windows-filtered metadata for `x86_64-pc-windows-msvc`. Cargo metadata does not
record the filter command itself. We include **every external package in
`packages[]`**, excluding only workspace members, without pruning by reachability,
feature, dependency kind, or presumed final linking. Build dependencies,
proc-macros, unused/optional paths and platform-related overinclusion can remain.
No fresh Cargo metadata, crate download, model download, build, or GUI run was
performed. No sibling repository or existing project file was modified by this
collection work.

| Collection metric | Result |
|---|---:|
| External crate name/version/source entries | 287 |
| Crates with local standalone legal texts | 269 |
| Crates supplemented from pinned official GitHub repositories | 18 |
| Crates with standalone legal texts after supplementation | 287 / 287 |
| Entries with no standalone crate legal text | 0 |
| Cached crate archives matching Cargo.lock SHA-256 | 287 / 287 |
| Rust text files, including supplemental evidence sections | 317 |
| Rust text bytes, excluding font references and JSON | 825,864 |
| License selections / final compliance review still pending | 287 |

**Text presence is not completeness of obligations.** The 287/287 metric means
that at least one standalone legal text is associated with each crate, not that
every declared license alternative, source-file notice, generated-code obligation,
or bundled asset has been individually cleared. README sections and source
comments are supplemental evidence and do not increase standalone coverage.

## Files and evidence

- [inventory.json](inventory.json): one entry per crate, with name, version,
  registry source, original Cargo license expression, normalized declared SPDX
  expression, repository, selected license (`null`), pending status, evidence,
  and per-file paths, byte sizes and SHA-256 hashes.
- `texts/<sha256>.txt`: full local standalone legal files, plus explicitly
  identified complete legal comment blocks and README legal sections. Identical
  bytes are deduplicated; **different copyright notices are never collapsed
  merely because their SPDX identifier matches**. Original package-relative
  filenames remain in the inventory.
- [upstream/index.json](upstream/index.json): explicit fixed-commit GitHub
  Contents API and raw URLs, Git blob SHA-1, SHA-256, byte sizes and crate mappings
  for the 18 downloaded legal files (file count differs from supplemented crate
  count because several crates share a repository).
- [upstream/tree-evidence.json](upstream/tree-evidence.json): selected legal and
  submodule entries from eight complete, non-truncated fixed-commit repository
  trees; verbatim `.gitmodules` data connects ANGLE and SPIRV-Headers to their
  parent repositories. Response hashes identify the responses inspected; full
  tree responses are not archived, and these selected entries are not an SBOM.
- [.gitattributes](.gitattributes): preserves original legal-text bytes across Git
  checkout; otherwise automatic newline conversion could invalidate hashes.

All standalone legal files are retained **in full, unmodified bytes**, including
LICENSE/LICENCE, NOTICE, COPYRIGHT, COPYING, UNLICENSE, nested legal directories,
and `font*.txt` candidates. This is a recursive filename-based collection, not a
whole-source license scanner. Files referenced by `license_file` are also included.
`local-legal-comment` and `local-readme-section` entries are explicitly excerpts:
they record the full source file hash and exact half-open byte offsets. They are
not represented as complete source files. Source copyright statements are copied,
not inferred from Cargo authors, package names, or generic license templates.

Local manifests, available `.cargo_vcs_info.json`, collected files and excerpt
source files were compared byte-for-byte against their already-cached `.crate`
archive. Each archive was hashed against the exact name/version/source entry in
`Cargo.lock`. Archives are only read in memory, not extracted or copied here.
The input metadata and lockfile hashes are recorded in the inventory. Neither
metadata nor private absolute cache paths are copied into this public directory.
Registry/repository URLs in metadata are recorded as evidence only, never opened
by the collector.

`spdx_declared` preserves AND/OR structure. Legacy Cargo `/` is normalized to OR
and that normalization is recorded; it is not a new grant or a license choice.
The collector does not implement a legal/SPDX approval engine. All choices remain
`selected_license: null`, `license_selection_status: pending`; retaining both
alternatives does not mean selecting both or selecting a license for third-party
subcomponents.

## Originally missing standalone crate texts

All of the following now have fixed-commit upstream supplementation. Pins come
from the exact local package's `.cargo_vcs_info.json`, verified against its cached
archive. Repository paths in Cargo metadata that mention `main` are not used as
revision pins.

| Crates | Fixed repository commit | Texts retained |
|---|---|---|
| ecolor, eframe, egui, egui-wgpu, egui-winit, egui_glow, emath, epaint, epaint_default_fonts — all 0.36.2 | emilk/egui `49682f8baa058bf49e011035cfbd6e825f88a5ef` | Root LICENSE-MIT and LICENSE-APACHE |
| accesskit 0.24.1 | AccessKit/accesskit `a55d3e1a18bb9ef0e4bccc9083fb13c3e0ad8969` | LICENSE-MIT, LICENSE-APACHE, LICENSE.chromium |
| accesskit_consumer 0.35.0, accesskit_windows 0.32.1, accesskit_winit 0.32.2 | AccessKit/accesskit `1bbcf100942bac96c2c3a4a91cb67b0b20201a24` | LICENSE-MIT, LICENSE-APACHE, LICENSE.chromium |
| clipboard-win 5.4.1 | DoumanAsh/clipboard-win `3b27cf2bfd1adcfa6e0264eb51c1025ddaf0f342` | LICENSE (Boost Software License) |
| gl_generator 0.14.0 | brendanzab/gl-rs `ea503e8d5fb6d73c6030e6191ce738cd3bf3433e` | LICENSE (Apache) |
| khronos_api 3.1.0 | brendanzab/gl-rs `f150967b1c44ae888e6676f93f639ebc82771bdc` | LICENSE (Apache), plus ANGLE license and local XML legal blocks |
| profiling 1.0.18 | aclysma/profiling `8271551172eb6fa4cba47369aedd93790c623df9` | LICENSE-MIT and LICENSE-APACHE |
| spirv 0.4.0+sdk-1.4.341.0 | gfx-rs/rspirv `8afc3d0ac8e158128cd1410bb2e4b4c26ab11bb4` | LICENSE (Apache), plus SPIRV-Headers legal texts |

Supplemental submodule pins:

- ANGLE: `google/angle` at `7403dd2cd3764fe96660fe09892e764e9ae1dbca`;
  full root LICENSE retained. Local `gl_angle_ext.xml` and `egl_angle_ext.xml`
  explicitly refer to a BSD-style license. Their complete legal comment blocks
  are retained separately. `khronos_api`'s Apache metadata alone is insufficient.
- SPIRV-Headers: `KhronosGroup/SPIRV-Headers` at
  `04f10f650d514df88b76d25e83db360142c7b174`; full root LICENSE and
  `LICENSES/MIT.txt`, `LICENSES/CC-BY-4.0.txt` retained conservatively.
  Their inclusion does not assert that every specification term applies to the
  generated Rust enums. That applicability remains pending.

The fallback was performed explicitly through tooling against
`api.github.com` and `raw.githubusercontent.com`, not through a dynamic
metadata-driven downloader. Raw file bytes were checked against GitHub Contents
API Git blob SHA-1 and then recorded with SHA-256. Upstream root texts absent
from a published archive are **not** claimed to be archive members; the inventory
marks them `fixed-github`, with `archive_bytes_verified: false`. VCS provenance
supports the association but is not a cryptographic signature or legal opinion.

## Font handoff (reference only)

The Rust code part of `epaint_default_fonts 0.36.2` receives the egui root MIT and
Apache texts here. Its **font assets are separately scoped** to:

- [Font attribution](../fonts/ATTRIBUTION.md)
- [Font review and limitations](../fonts/README.md)
- [Hack-Regular.txt](../fonts/Hack-Regular.txt)
- [OFL.txt](../fonts/OFL.txt)
- [UFL.txt](../fonts/UFL.txt)
- [emoji-icon-font-mit-license.txt](../fonts/emoji-icon-font-mit-license.txt)

The inventory references and hashes all four existing font texts and verifies
that they equal the local crate/archive bytes. This task does not rewrite that
review, copy TTFs, or certify its conclusions. Include the font attribution and
all four texts in the final package alongside the Rust directory. Hack's
Bitstream Vera terms and the font copyright notices cannot be replaced by an
MIT/Apache crate-level choice. Other package test-font notices, such as
`ttf-parser/tests/fonts/colr_1_LICENSE`, remain retained as found; they are not
silently discarded as assumed irrelevant.

## Missing items and review limitations — release gate remains open

There are **zero entries without a standalone crate legal text**, but the
following evidence/decisions remain outstanding:

1. **Final artifact linkage and packaging:** no release executables, linker maps,
   build provenance or final archive contents were inspected. Reconcile the
   actual build's lockfile, target, features and native inputs; verify that these
   notices and the font handoff reach the shipped package. This collection does
   not cover Rust toolchain/standard-library notices, native runtimes, OS SDKs,
   models, system/user fonts or other non-crate inputs.
2. **License choice and obligations:** all 287 entries await final review.
   `self_cell` offers Apache-2.0 OR GPL-2.0-only; both complete texts are retained,
   and no GPL or Apache election is silently made. `dpi` declares Apache-2.0 AND
   MIT; its LICENSE-LIBM-MIT is retained. `unicode-ident`'s Unicode AND obligation
   and `webpki-root-certs`' CDLA-Permissive-2.0 data terms must not be treated as
   generic MIT/Apache alternatives.
3. **Embedded third-party material:** AccessKit's Chromium notice is retained
   even for conservatively overincluded members; `accesskit_winit` itself is
   declared Apache-only, so the repository's MIT text does not expand its grant.
   All discovered local nested notices are retained, including regex-syntax's
   Unicode tables, tracing-core's spin code and tiff's test COPYRIGHT. `libm`'s
   complete multi-origin LICENSE.txt is retained. Source headers and notices
   outside the collection patterns still require file-level review.
4. **Khronos/WebGL:** OpenGL/EGL XML copyright and permission blocks and ANGLE
   BSD terms are retained. The pinned WebGL submodule is
   `c987f075bfdca44119175c41f547d849c08983a3`; applicable specification/extension
   terms for its packaged material and generated bindings remain unresolved.
   Unrelated upstream demos/test-suite licenses were not substituted for those
   terms. Do not call this crate or its generated output all-cleared.
5. **SPIR-V and egui attribution:** SPIRV-Headers generated-code/specification
   applicability remains pending. egui's README license section retains its
   lyon_geom attribution; this is not proof of a full source-level attribution
   audit. Likewise profiling's README mentions an example font; retaining that
   section does not establish whether that asset enters a final distribution.
6. **Source validity and upstream association:** matching cached archive and
   lock hashes demonstrates consistency of the supplied local evidence, not
   independent registry authentication, correctness of upstream declarations,
   or exhaustive copyright ownership. No invented copyright lines or generic
   replacement license templates were introduced.

The machine-readable `missing` array reports only missing standalone-text
entries. Per-package `pending` and this section describe the broader open work;
`missing: []` must never be interpreted as release approval.

## Offline reproduction and validation

From the project root, with Python 3.11+ and the existing local crate cache:

```text
python scripts/collect_rust_licenses.py
python scripts/collect_rust_licenses.py --check
```

The first command writes new content-addressed texts and regenerates only this
Rust inventory. Existing differing text files are refused, not overwritten;
unreferenced old texts are not deleted automatically. `--check` is read-only and
requires byte-for-byte deterministic inventory output. It verifies all referenced
local/archive bytes, upstream SHA-256/Git blob hashes, and the four font references.
Exit 0 means collection/check success, 1 means missing standalone texts, and 2
means invalid/unavailable evidence. None of these exit codes grants legal
clearance. New versions need separately reviewed pinned fallback evidence; the
script never fetches a URL, executes package code, runs Cargo, or starts a build.

The metadata may contain private absolute source paths for local reads. Those
paths are deliberately omitted from the inventory; use metadata on the machine
where its paths exist, or supply an appropriate existing snapshot via
`--metadata`. Do not publish the raw metadata as part of this legal bundle.
