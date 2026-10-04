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

## Licensing and limitations

The pinned model card identifies Apache-2.0. Review the upstream license, attribution, and any applicable notices before redistribution. The installer does not collect license/NOTICE documents; integrity verification is not distribution clearance. ONNX Runtime and other dependencies have separate requirements, described in the [release procedure](../RELEASING.md).

Missing or incompatible model files cause loading errors. Recognition is bounded, can be incorrect, and does not imply support for arbitrary LaTeX or a complete computer algebra system.
