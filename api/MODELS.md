# Optional TexTeller models

Ordinary drawing and template recognition do not require model weights. Neural recognition uses local ONNX Runtime inference; the GUI neither downloads weights nor loads them without an explicit action. This source prerelease does not include weights or native runtime redistribution packages.

## Installation

From the repository root, with Python and network access:

```powershell
python crates/board-hwr/download_texteller.py --dir models/texteller
```

The script downloads five pinned files totaling approximately 1.25 GB and verifies lengths and hashes. Verified files are reused. It connects to Hugging Face, with retry fallback to `hf-mirror.com`; `--mirror` selects only the mirror. It does not use ambient proxy configuration or credentials. Downloads are an explicit action, separate from the GUI.

In Blackboard, choose TexTeller, enter the absolute model directory, select load/reload, and enable handwriting recognition. Review each candidate before clicking calculate or plot. Scores are not accuracy probabilities.

## Provenance and integrity

Source: [OleehyO/TexTeller](https://huggingface.co/OleehyO/TexTeller/tree/7b96df06b9d81cdb129c3bef68b7250bc3e2b0ea).
Pinned revision: `7b96df06b9d81cdb129c3bef68b7250bc3e2b0ea`.

| File | Bytes | SHA-256 |
|---|---:|---|
| encoder_model.onnx | 343553824 | `071afc0a0c92b2ee3847612127eb9e82b1c39413725e692f4405e5ea9e265bf0` |
| decoder_model.onnx | 908716081 | `288cb8e37cdda66f725f2739efd2e7846636666a815e27533891c68389ec3659` |

Configuration and tokenizer lengths and Git-blob hashes are specified in the [installer](../crates/board-hwr/download_texteller.py).

## GitHub build integration

Application builds and model preparation are separate manual workflows. Both upload reports only by default; uploading candidate weights/binaries requires explicit maintainer approval and supplied license materials. They do not create releases or modify the source-only v0.0.1 tag. See [pipeline instructions and package layout](BUILD_PIPELINE.md).

## Low-memory CPU preparation

For machines with limited RAM, use a separately prepared dynamic INT8 directory rather than the original 1.25 GB FP32 weights. Conversion is an **offline preparation step on a machine with ample RAM**, not something to run automatically on a 3 GB PC. The application itself still uses Rust/ONNX Runtime; Python is only needed for conversion and optional benchmarking.

On a preparation machine with Python 3.11 or 3.12:

```powershell
py -3.11 -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r crates/board-hwr/requirements-quantize.txt
.\.venv\Scripts\python.exe crates/board-hwr/prepare_texteller_int8.py --source models/texteller --output models/texteller-int8
```

Package installation uses the network; conversion does not. The converter accepts only the original installer-pinned files, checks hashes before loading ONNX, rejects external tensor references, and refuses an existing or nested destination. It uses separate sequential processes for encoder/decoder conversion, dynamically quantizes constant MatMul/Gemm weights to signed INT8 with per-channel reduced range, verifies model signatures, and writes `optimization.json` with source/output hashes and tool versions. Other operators remain floating point. It preserves the original model directory and publishes the new directory only after success. The pinned ONNX parser is for these hash-verified inputs, **not an untrusted-model conversion service**.

Use the complete new directory on the target machine. Blackboard suggests `models/texteller-int8` in the working/executable directory when the five model/config files and manifest are present, before falling back to `models/texteller`. This is a filename-presence suggestion, not cryptographic authentication: load only trusted files. Select TexTeller and explicitly load the suggested directory. Nothing is automatically converted, downloaded, loaded, or uploaded. License/attribution requirements still apply if you distribute converted weights.

### Runtime and lifecycle policy

- `NeuralRecognizer::load` uses `NeuralOptions::low_memory()`: at most two intra-op threads by default, one inter-op thread, sequential execution, no thread spinning, CPU arena and memory-pattern retention disabled, Level3 optimization, and weight prepacking enabled. The original CPU provider already had its arena disabled; this is not counted as a new memory saving.
- Prepacking stays enabled because the measured INT8 short case was faster for approximately 2.4 MiB extra peak memory. It can be disabled with `load_with_options` for comparisons, but lower memory settings are not universally faster.
- Input remains 448 x 448; default limits remain 256 generated tokens and a 60-second cooperative budget. No lower-resolution approximation, injected answers, or automatic calculation was introduced.
- Loading and inference share a process-wide nonqueuing mutex. Both sessions remain resident after loading; independent library recognizers do not share weights. There is no hard process-memory limit.
- GUI reload is blocked until old inference/loading work drains, avoiding overlapping old/new model residency. Switching to templates or using **Unload model** releases the GUI model; in-flight work must finish before its last reference is released. Turning recognition Off alone retains the model. Stale load results are discarded.
- Decoding still evaluates the complete prefix without KV caching. A KV-cache decoder requires a different model export and validated signatures; it is not implemented by merely adding a flag.

### Measured results and limitations

Measured locally on Windows 11, a 14-core/20-logical-CPU Intel host with approximately 96 GiB RAM, **not the target i5/3 GB machine**. Each row used a fresh benchmark process and three synthetic recognition runs. Peak working set includes model loading; it excludes the GUI, Python sampler, OS, and other applications.

| Input / configuration | Peak working set | Mean recognition | Output observation |
|---|---:|---:|---|
| Short `1+1`, original FP32 baseline, four threads | 1324.6 MiB | 2.44 s | Complete `1+1` output |
| Short `1+1`, FP32, two threads, prepacking off | 1319.6 MiB | 5.25 s | Same output; tuning alone did not solve memory usage |
| Short `1+1`, INT8, two threads, prepacking on | 486.2 MiB | 1.72 s | Same output on this sample |
| Four repeated groups, FP32, low-memory profile, two-CPU affinity | 1321.7 MiB | 11.07 s | Complete output, but missing one `1` |
| Same groups/settings/affinity, INT8 | 487.2 MiB | 6.48 s | Identical to FP32, including its error |
| Eight repeated groups, INT8, two-CPU affinity | 492.1 MiB | 12.18 s | 30 tokens, EOS; incorrect recognition |

The matched four-group comparison reduced peak working set by approximately **63%** and mean latency by approximately **41%**. Affinity to two logical CPUs is not emulation of an older i5. These are small synthetic samples, not an accuracy corpus or a worst-case 256-token bound. Long thin inputs are downscaled into the same fixed image; output agreement is not proof of correctness. Validate fractions, radicals, equations and real handwriting before relying on INT8 results.

The two ONNX files shrink from 1,252,269,905 to 368,065,562 bytes (approximately 70.6%):

| Converted file | Bytes | SHA-256 from the measured conversion |
|---|---:|---|
| encoder_model.onnx | 89332035 | `6d1daf6052c879e0fba9b0a282c2720ddd03b7642063f38392db03d78123d0cb` |
| decoder_model.onnx | 278733527 | `28a6b9d60644d6b2e3280593dc985456c2db71bbd4e86f69585d385801a651a8` |

A second conversion with the same environment produced identical hashes. Quantization still changes model behavior; the generated manifest intentionally keeps `quality_validated: false`.

**3 GB whole-machine acceptance remains pending.** Windows, the GUI, document/history/cache sizes, drivers and other applications consume additional RAM. An i5 generation and exact available-memory budget have not been established. No VM/job-object 3 GB cap, long-running full-GUI workload, or target-machine latency/accuracy test was performed. Use the template backend when neural memory/latency is unacceptable.

### Reproduce measurements

```powershell
cargo build -p board-hwr --example neural_benchmark --release --locked
python -m pip install psutil
python crates/board-hwr/examples/neural_benchmark_runner.py --exe target/release/examples/neural_benchmark.exe --model-dir models/texteller-int8 --profile low-memory --ink-repeat 4 --rounds 3 --cpu 2 --timeout 240
```

Repeat in a fresh process with `--model-dir models/texteller` for the matched FP32 comparison. Use `--profile baseline` for the old four-thread policy; this is not the same configuration as the matched comparison. The benchmark reports actual LaTeX, EOS, token counts, options, load/inference times and Windows memory high-water marks. The runner requires psutil, records local paths and hardware, and must not be published unreviewed. Its timeout terminates only its own benchmark process tree; no GUI or desktop acquisition is involved.

## Licensing and limitations

The pinned model card identifies Apache-2.0. Review the upstream license, attribution, and any applicable notices before redistribution. The installer does not collect license/NOTICE documents; integrity verification is not distribution clearance. ONNX Runtime and other dependencies have separate requirements, described in the [release procedure](../RELEASING.md).

Missing or incompatible model files cause loading errors. Recognition is bounded, can be incorrect, and does not imply support for arbitrary LaTeX or a complete computer algebra system.
