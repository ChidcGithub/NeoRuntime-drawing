# Distribution license evidence

Review date: 2026-10-04. This directory preserves original legal texts and evidence for the current Windows x64 application and optional INT8 model. It is an engineering review, not legal advice or a declaration that every redistribution obligation has been fulfilled.

## Decision

**Local/CI build and test: proceed. Public binary or model upload: not approved yet.** No top-level `app/REVIEWED.md` or `model/REVIEWED.md` marker is provided; the existing candidate upload gate must remain closed. The published v0.0.1 tag remains source-only. Building a program is distinct from redistributing its linked libraries or weights.

| Area | Completed evidence | Remaining requirements |
|---|---|---|
| Rust packages | [287 package records and legal texts](app/rust/README.md), cached archives verified against Cargo.lock, missing texts supplemented at pinned upstream commits | Component-level selection/applicability and final-artifact inventory are pending; text coverage is not complete obligation clearance |
| Default fonts | [Four original license texts and font attribution](app/fonts/README.md), including Hack/Bitstream Vera, Noto/OFL, Ubuntu/UFL and emoji-icon/MIT | Include all applicable texts and specific attributions in a binary package; do not add installed Windows font files |
| DirectML | [Version 1.15.4 and official NuGet hash match](app/native/PROVENANCE.md), original Microsoft runtime terms and notices | Follow actual conditional Windows/Xbox application redistribution terms; not MIT licensing of the DLL |
| Native ONNX Runtime | v1.22.0 original MIT/third-party notices; existing static library and executable link evidence | Establish Pyke artifact's precise source, patches and component mapping; fulfill Eigen/MPL covered-source availability obligations |
| MSVC runtime | Existing executables' runtime imports identified | Choose supported prerequisite installation or a separately authorized redistributable strategy; validate on clean Windows |
| TexTeller weights/tokenizer | [Pinned mirror card, standard/code licenses and INT8 modification provenance](model/PROVENANCE.md) | Official fixed-revision scope/notice verification failed due to network access; representative quality validation remains absent |

## What is not an automatic blocker

- Apache-2.0 does not require inventing a NOTICE file or copyright identity. Retain actual applicable upstream notices and verify source ownership.
- An OR license expression offers alternatives; an AND expression requires its combined terms. A GPL alternative does not automatically make the whole application GPL.
- Linking MPL-covered components does not automatically relicense the entire application. The relevant covered source still needs an appropriate recipient-accessible delivery route.
- Optional models are separate from application packaging; lack of a model package does not prevent ordinary local drawing or template recognition.

## Next steps

1. Obtain a verifiable source/build mapping for the current native ORT artifact and make any required covered source available. If this cannot be obtained, evaluate a separately reviewed official/source-built runtime; do not silently relabel the current static binary.
2. Finish license selection/applicability review for the actual shipped Rust/native components and include the corresponding texts and notices. Preserve complete fonts attribution.
3. Verify official model revision terms and record appropriate weight/tokenizer permissions and INT8 modifications. Accuracy testing is a separate product gate, not proof of licensing.
4. Select and test the Windows runtime installation strategy.
5. Only after those decisions, supply truthful per-package `REVIEWED.md` records and enable candidate uploads. Do not create a clearance marker merely to make a workflow pass.

## Build troubleshooting performed during review

The first GitHub Actions attempts failed in Python packaging tests. Their reports were also hidden because upload-artifact excluded `.ci-reports/`. Workflows now explicitly upload only that controlled hidden report directory and tee failed test/build output to the console. A Windows temporary-path alias mismatch in the mock `lstat` test was corrected by canonicalizing the expected path; actual link/reparse rejection remains intact.

No GUI, desktop capture, microphone, or real handwriting input was used in this review. Legal material acquisition did not publish application/model binaries. See [build pipeline](../../api/BUILD_PIPELINE.md) and the per-component provenance records for scope, hashes, original sources and unresolved items.
