# Native redistribution evidence — BLOCKED

Review date: 2026-10-04. Scope: existing Windows x64 release artifacts, their
native dependency provenance, and original license/notice collection. This is
an engineering record, not legal advice or an approval to publish binaries.
No `REVIEWED.md` or global clearance is supplied. No rebuild was performed by
this review; the hashes below describe the inspected existing artifacts only.
Recheck them after any formal build and against the final package allowlist.

## Result

| Item | Result | Remaining condition |
| --- | --- | --- |
| DirectML exact identity | VERIFIED | Release DLL, derived ORT cache DLL and official NuGet x64 DLL are byte-identical by SHA-256. |
| DirectML original terms/notices | COLLECTED; identity/terms uncertainty cleared | Use only under the actual Microsoft runtime terms, including the Windows/Xbox application/service scope. This is not MIT licensing of the DLL or blanket application clearance. |
| ORT upstream MIT and third-party notices | COLLECTED | Upstream v1.22.0 texts are not an artifact-specific Pyke inventory. |
| Pyke native artifact provenance/completeness | BLOCKED | Exact source revision, patches, build configuration, embedded component versions/notices, and correspondence to the cached static library remain unverified. |
| Eigen/MPL covered-source obligations | BLOCKED | Establish exact covered source and modifications and provide a working, recipient-facing source-availability route. Copying MPL text alone does not discharge section 3.2. |
| MSVC runtime deployment | OPEN / package gate | Both executables import MSVC runtime DLLs. Select a supported prerequisite installer or separately authorized redistribution strategy; do not copy developer-machine DLLs based on this review. |

**Do not treat this directory as a complete native redistribution bundle.**
Do not clear a binary release while the Pyke/Eigen blockers remain unresolved.
Other Rust dependencies, fonts, models, ownership review, packaging and
clean-machine acceptance are outside this focused native review.

## Evidence chain and privacy boundary

1. `cargo metadata --format-version 1 --locked --offline --filter-platform
   x86_64-pc-windows-msvc` located `ort-sys` 2.0.0-rc.10. Its actual crate
   directory was inspected read-only, not found by searching unrelated user
   files. Unfiltered offline metadata initially failed because
   `foreign-types v0.3.2` was not cached; the filtered command succeeded.
2. Existing `target/release/build/ort-sys-d8983585266b2d39/output` supplied the
   native cache location through `cargo:rustc-link-search=native`. Only that
   artifact cache subtree was listed/read, without modifying it.
3. `cache-evidence.json` records portable cache locators, original build-output
   hash, crate revision, and the complete two-file inventory of that artifact
   cache. `ort-sys/build-output.sanitized.txt` preserves the output with local
   absolute paths replaced by `<PROJECT>` and `<ORT_CACHE>`.
4. Original cached `ort-sys` texts are retained byte-for-byte: `LICENSE-MIT`,
   `LICENSE-APACHE`, `dist.txt`, `build.rs`, and `.cargo_vcs_info.json`.
   The source revision is `daf91046dc814d5aea61d24e0daf571c7df91930`.
   These wrapper/build-script licenses do not replace native library licenses.
5. `pe-evidence.json` records SHA-256, sizes, PE machine type, version strings,
   normal import DLLs/symbols and delay imports using Python `pefile`.
   No executable or DLL was launched. The initial extraction command timed
   out after writing its JSON; a subsequent successful JSON read confirmed
   all three complete entries. Final validation checked the recorded hashes.
6. `sources.json` maps collected source materials to URLs and byte hashes;
   `SHA256SUMS` covers the final evidence/text bundle, excluding itself.
   Cache/build-output provenance is distinguished from official upstream
   material. No local absolute personal paths are included in these records.

## DirectML 1.15.4: resolved artifact identification

Inspected file: `target/release/DirectML.dll`, 18,527,776 bytes, PE machine
`0x8664` (AMD64).

- File/Product version: `1.15.4+241025-1615.1.dml-1.15.fac7597`.
- Product: `DirectML Redistributable Library`; Microsoft Corporation.
- SHA-256: `9c9e6d822561c6c41b90e6994b3e8857cf1d66dbfb1e0c4c799c7c89b4e92da1`.
- Official package: <https://api.nuget.org/v3-flatcontainer/microsoft.ai.directml/1.15.4/microsoft.ai.directml.1.15.4.nupkg>.
- NuGet SHA-256: `4e7cb7ddce8cf837a7a75dc029209b520ca0101470fcdf275c1f49736a3615b9`.
- Matching member: `bin/x64-win/DirectML.dll`. Its hash also matches the DLL
  in the build-output-derived ORT cache. This is a byte comparison, not merely
  reliance on version-resource text. NuGet's signature was not independently
  verified; acquisition was through the official HTTPS endpoint.

The exact-version package was downloaded for inspection only (approximately
193 MiB, larger than a small package). No DLL was uploaded, replaced, or copied
into this legal directory. Debug DLLs, other architectures, PDBs and unrelated
NuGet binaries are not part of the proposed application payload.

Original package members retained under `directml/`:

- `LICENSE.txt`: **runtime** Microsoft DirectML agreement.
- `ThirdPartyNotices.txt`: entire original third-party notices, including GSL,
  half and Zstandard materials, not just extracted license names.
- `LICENSE-CODE.txt`: MIT terms for headers/build integration, **not the DLL**.
- `README.md`: upstream statement of the license scopes (`bin/` versus
  `include/` and `build/`).
- `Microsoft.AI.DirectML.nuspec`: exact version, Microsoft author, file-license
  declaration and redistributable description.
- `nuget-evidence.json`: package/member hashes and full member-name inventory.

Runtime license section 1(a) permits copying/distributing the software in the
specified developed applications/services/games for Windows and Xbox, subject
to the agreement. Sections 1(c), 3(c), 3(e) and 4 require attention to third-party
terms, preservation of notices, the restriction on stand-alone distribution,
and export restrictions. Other sections also remain applicable. Preserve the
original runtime terms and notices with a Windows application package; do not
relabel the binary Apache-2.0 or MIT and do not present it as a stand-alone SDK
release. Section 2 contains Microsoft's data terms; this review did not test
telemetry and makes no claim that the runtime never communicates externally.

## Actual executable imports

Both inspected executables are AMD64 and directly import
`directml.dll!DMLCreateDevice1`, not merely a string mentioning DirectML:

| Existing file | SHA-256 |
| --- | --- |
| `target/release/neo-drawing.exe` | `c28fadc676e9e6dcd84b8a7c5db5fd052e9821cdb277a33c32447342149a1b04` |
| `target/release/neo-blackboard.exe` | `92a095b3ea3abfbcf7300aac8fd24423ecea3510c2580463c8e779d6703ec3a2` |

Neither has an `onnxruntime.dll` import; existing build output explicitly says
`cargo:rustc-link-lib=static=onnxruntime`. Static linking does not remove license
or covered-source obligations. Import evidence does not prove GPU execution.
Both also import `MSVCP140.dll`, `MSVCP140_1.dll`, `VCRUNTIME140.dll`,
`VCRUNTIME140_1.dll` and Windows Universal CRT API-set DLLs. Full import lists
are in `pe-evidence.json`. OS/API-set imports are not an instruction to bundle
Windows DLLs. No compiler redistributable license is fabricated here.

## ORT 1.22.0 Pyke artifact: remaining provenance blocker

The exact `ort-sys/dist.txt` row is feature set `none`, target
`x86_64-pc-windows-msvc`:

- URL: <https://cdn.pyke.io/0/pyke:ort-rs/ms@1.22.0/x86_64-pc-windows-msvc.tgz>.
- Expected archive SHA-256:
  `540D19B3379FDA6FB8F7280D8C15EFDE20ED225A67A357A6DAE38C4300FE190D`.
- Cached `onnxruntime/lib/onnxruntime.lib`: 303,265,000 bytes; measured SHA-256
  `ea936a55f367d8eeaafd50e595a18b254281ed16fe566c614e6e4b10055ce024`.

**The expected archive hash is not a newly measured archive hash.** The original
TGZ was not present in the inspected artifact cache and was not downloaded.
The build script verifies SHA-256 on download, but skips downloading/checking
when the extraction directory already exists. Its directory name and dist
row therefore do not independently prove the integrity of today's static lib
against the original archive. Existing build output identifies its selection
and linkage, not a signed source/build attestation.

That cache contains only `onnxruntime/lib/DirectML.dll` and
`onnxruntime/lib/onnxruntime.lib`: no LICENSE, ThirdPartyNotices, source or
patch manifest was found there. `static_link_prerequisites(true)` in the
retained `build.rs` adds DirectML linking for Windows Pyke libraries even when
selected feature set is `none`.

Original upstream materials retained under `onnxruntime/`:

- `LICENSE`: <https://raw.githubusercontent.com/microsoft/onnxruntime/v1.22.0/LICENSE>.
- `ThirdPartyNotices.txt`: complete upstream v1.22.0 file (326,866 bytes).
  Raw-host requests timed out/reset; the successful official GitHub Contents
  API response was decoded without text normalization and checked against
  its Git blob SHA-1. See `thirdparty-retrieval.json` and `sources.json`.
- `deps.txt` and `eigen.cmake`: upstream dependency pins/build-source context,
  not proof that the Pyke producer used them unchanged.
- `symbol-evidence.json`: native static-library symbol fragments provide
  positive Eigen/Abseil/protobuf/FlatBuffers clues, not an exhaustive SBOM or
  proof of exact versions/final-linker retention. The broad `re2` substring
  probe has unrelated Windows-symbol matches; it is **not** RE2 evidence.

The fixed `pykeio/ort` repository tree was queried for build/patch/legal files
at the crate's revision. It does not by itself supply provenance binding this
CDN archive to the exact native source and modifications. No claim is made
that the Pyke artifact equals a Microsoft-built ONNX Runtime release.

## Eigen/MPL: source obligation is not discharged

The full Eigen MPL-2.0 text is already preserved in upstream
`onnxruntime/ThirdPartyNotices.txt` (Eigen section begins at line 417).
Sections 3.1, 3.2 and 3.4 cover source licensing, executable distribution/source
availability and preservation of notices; section 3.3 permits a larger work
under other terms while retaining covered-software obligations. This does
**not** require automatically relicensing all application code as MPL.

Upstream ORT v1.22.0 `deps.txt` pins Eigen to
`1d8b82b0740839c0de7f1242a3585e3390ff5f33`, not simply stock Eigen 3.4.0:

- Candidate source: <https://gitlab.com/libeigen/eigen/-/archive/1d8b82b0740839c0de7f1242a3585e3390ff5f33/eigen-1d8b82b0740839c0de7f1242a3585e3390ff5f33.zip>.
- Upstream recorded archive SHA-1: `5ea4d05e62d7f954a46b3213f9b2535bdd866803`.
- The pinned GitLab `COPYING.MPL2` request returned HTTP 403 here; no separate
  Eigen source archive was fetched or verified. The complete MPL text in ORT's
  original notices was retained, not replaced with this link.

Native Eigen symbol fragments make dismissing Eigen as merely an unused
upstream notice unsafe. They do not identify exact covered files, patches,
compile flags or exceptions. Do not infer an MPL exception, a no-modifications
assertion or `EIGEN_MPL2_ONLY` from the artifact name.

Before distribution, obtain producer evidence of the exact covered source,
modifications and relevant build configuration; preserve component notices;
and make that source (including modifications to covered files) available to
recipients under MPL by reasonable, timely means with a clear accompanying
instruction. An upstream URL can be part of a verified source-delivery plan,
but this unverified candidate URL is **not** a fulfilled source offer. No
invented source-hosting promise is made. If provenance cannot be obtained,
a separately reviewed source-built/replacement runtime may be needed; this
review does not change Cargo, runtime selection or workflows to do that.

## Verification and release handoff

Original legal bytes were downloaded or extracted without rewriting content;
hashes are recorded in `sources.json` and `SHA256SUMS`. The official NuGet DLL
comparison, local cache DLL comparison, JSON integrity and final file hashes
were checked. No GUI, desktop capture, microphone, inference, DLL upload,
commit, formal build or runtime/workflow change was performed.

This review supplies evidence and closes the DirectML version/license-text
unknowns only. Resolve the artifact-specific ORT/Eigen blockers and compiler
runtime packaging strategy before declaring native distribution cleared.
Then rebuild under the separately approved release procedure, re-hash/recheck
imports, include all required texts and source instructions in the actual
package, and validate that package on a clean Windows machine.
