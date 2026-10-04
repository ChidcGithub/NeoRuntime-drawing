# Application and INT8 model build pipeline

Two independent, manual GitHub Actions workflows prepare evaluation candidates. They never create releases, push commits, update tags, or attach assets to `v0.0.1`. The existing prerelease remains source-only.

## Workflows

| Workflow | Runner | Default output |
|---|---|---|
| Windows application candidate | Windows Server 2022 x64 | Tests, build log, version output and binary hashes; no binaries |
| TexTeller INT8 preparation | Windows Server 2022 x64 | Conversion manifest, tool versions and matched FP32/INT8 synthetic benchmark reports; no weights |

Both workflows are `workflow_dispatch` only, have `contents: read`, use commit-pinned checkout/setup/upload actions, and disable persisted checkout credentials. Rust is selected explicitly as 1.97.1 for CI; Python is 3.11, with ONNX/ORT conversion versions fixed in `requirements-quantize.txt`. The runner image and transitive Python/native dependencies are not a hermetic build environment; their versions and generated hashes must be reviewed.

From the repository's **Actions** tab, select a workflow and **Run workflow**. Leave **upload_candidate** unchecked for the initial run. The model workflow downloads roughly 1.25 GB of original weights and performs memory-intensive conversion; it can take considerably longer than an application build and consumes runner/network/storage resources. Conversion belongs on a preparation machine, not the 3 GB target PC. No model inference GUI, desktop capture, microphone access, or real handwriting collection runs in CI.

Reports expire after 14 days. They identify the commit and may contain runner-local paths, hardware information and synthetic recognition output. Public-repository artifacts must be treated as distributable downloads, not confidential staging storage.

## Candidate uploads are explicitly gated

Only check **upload_candidate** after reviewing the intended materials. Add these directories in a reviewed commit beforehand:

- Application: `distribution/legal/app/`
- Model: `distribution/legal/model/`

Each directory needs a nonempty `REVIEWED.md` and the complete applicable license/attribution/NOTICE texts. Record the reviewer, artifact/component versions, provenance, selected license branches and unresolved caveats. For models, also record recognition-quality evaluation. No placeholder clearance or fabricated attribution is supplied by this repository.

The packaging script requires UTF-8 text legal files and at least one nonempty `LICENSE`, `LICENSE-*`, `LICENSE.txt` or `LICENSE.md`. These checks establish presence only: **a checkbox or a file named LICENSE is not legal clearance**. See [distribution requirements](../RELEASING.md#4-requirements-for-any-future-binary-or-model-distribution). ONNX Runtime, DirectML and embedded fonts need their own review; optional model weights require separate provenance and permission checks.

Approved artifacts expire after seven days and contain an inner candidate ZIP, `SOURCE.json`, `FILES.sha256`, and `SHA256SUMS`. The commit SHA/run ID identifies the artifact; application version 0.0.1 does not imply a stable model ABI or a final release.

### Application allowlist

- `neo-drawing.exe`, `neo-blackboard.exe`
- `DirectML.dll` only if present in the input build directory
- Project `LICENSE`, `README.md`, supplied legal materials and generated provenance/checksums

No arbitrary DLL or entire `target/` directory is copied. This allowlist does not prove runtime completeness or portability; clean-machine testing is still required. Application packages do not contain weights.

### Model allowlist

The ZIP preserves the `models/texteller-int8/` layout with exactly the five model/config/tokenizer files plus `optimization.json`. The packager verifies the known manifest format, pinned source revision, output file sizes and SHA-256 hashes before packaging. The models keep standard encoder/decoder filenames and existing I/O signatures.

Extract the model ZIP beside the application executables. The GUI will suggest the complete directory but still requires explicit loading. Neither model discovery nor a hash manifest makes untrusted ONNX files safe. Do not run the original FP32 installer against the INT8 destination: it expects original hashes and would replace converted weights.

The main program is not modified or recompiled to embed a model. No `include_bytes!` model payload, startup download, or unconditional `latest` model lookup is introduced.

## Evaluation and promotion

The model workflow measures short and longer synthetic inputs, with equal low-memory runtime settings and two-logical-CPU affinity for both precisions. It requires successful inference and EOS, **not agreement with a known correct answer**. A passing run cannot establish recognition accuracy, i5 equivalence, peak memory for all formulas, or whole-machine 3 GB usability. Use a representative labeled handwriting corpus and the target hardware before release.

The ONNX/ORT quantizer remains restricted to installer-pinned official inputs, with external tensor references refused. It is not a general untrusted-model service. Framework warnings about preprocessing and any numerical differences should be reviewed, not hidden.

Promote an approved candidate manually only after its legal, quality and runtime gates pass. Use a separate model version/identifier and pinned SHA-256 rather than converting again during every application release. A future release may provide an application ZIP and a separately downloadable model ZIP; neither workflow currently performs that promotion or consumes arbitrary external model assets.

## Local validation

```powershell
cargo test --workspace --locked
cargo test -p board-hwr --example neural_benchmark --locked
python -m unittest discover -s crates/board-hwr -p test_prepare_texteller_int8.py
python -m unittest discover -s crates/board-hwr/examples -p test_neural_benchmark_runner.py
python -m unittest discover -s scripts -p test_package_candidate.py
```

Local tests use synthetic package data, not model downloads or desktop capture. Successful local YAML checks do not mean a GitHub-hosted job has run successfully.
