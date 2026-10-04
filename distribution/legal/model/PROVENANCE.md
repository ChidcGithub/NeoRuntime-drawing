# TexTeller model redistribution evidence — BLOCKED

Review date: 2026-10-04. This is an engineering evidence record, not legal advice, release approval, or an accuracy evaluation.

**Status: BLOCKED.** Official Hugging Face fixed-revision terms and applicable weight/tokenizer notices could not be verified. The local INT8 manifest also explicitly records `quality_validated: false`; no representative quality dataset was supplied or evaluated. No `REVIEWED.md` is provided. Do not bypass the packager's review gate or treat these materials as permission to publish.

## Exact identities and scope

- Weight/model repository: `OleehyO/TexTeller` on Hugging Face.
- Required **model revision**: `7b96df06b9d81cdb129c3bef68b7250bc3e2b0ea`.
- Intended original inputs: `encoder_model.onnx`, `decoder_model.onnx`, `config.json`, `generation_config.json`, `tokenizer.json` from that revision.
- Separate upstream **code commit**: `12e6bb4312cc934bb4b69fcda2a1ac38e94930e6` in GitHub `OleehyO/TexTeller`. This is NOT the weight revision, proof of the weight export's source commit, or independent authorization for the weights/tokenizer.
- Candidate derivative: the two locally prepared dynamic INT8 ONNX files described in `INT8-PROVENANCE.txt` and `MODIFICATIONS.md`; configuration/tokenizer files are copied without intended content changes by the converter.
- No other HF weights, merged/cached decoder variants, dataset, runtime binary, upstream source tree, or tokenizer library is included here. The Rust `tokenizers` library's own license does not establish permission for `tokenizer.json` data.

## Evidence and limits

| Subject | Observation | Conclusion |
|---|---|---|
| Fixed model card | `MODEL-CARD-MIRROR.md` preserves the raw README response from `hf-mirror.com` at the exact model revision; YAML says `license: apache-2.0`. | Mirror-backed declaration only; official verification remains blocked. Upstream performance statements are preserved as quotations, not adopted as local INT8 accuracy claims. |
| Revision metadata | `HF-REVISION-MIRROR.txt` preserves the mirror API JSON. `sha` matches the model revision; `cardData.license` is `apache-2.0`; siblings include both ONNX weights and `tokenizer.json`. | Supports repository-wide declared terms, but does not independently resolve tokenizer origin, component exceptions, or official rights/notice scope. |
| Model LICENSE / NOTICE | Mirror siblings list no LICENSE, NOTICE or COPYING file. Official API, card, LICENSE and NOTICE requests failed (details below). | Do NOT interpret a network failure as HTTP 404 or absence of notices. Official weight/tokenizer notice inventory remains unresolved. |
| Apache standard text | `LICENSE-APACHE-2.0.txt` is the complete, unmodified official Apache response, including appendix. | Verified standard terms, not proof that the model owner granted those terms. Appendix placeholders remain standard boilerplate, not invented attribution. |
| GitHub code license | `LICENSE-TEXTELLER-SOURCE.txt` is the exact LICENSE blob from the separate fixed code commit, retrieved through the official GitHub API. It contains `Copyright OleehyO`. | Verified for that code source; attribution retained verbatim without inventing a year. NOT a substitute for official weight/tokenizer verification. |
| GitHub code notices | `SOURCE-TREE-GITHUB.txt` is the official recursive tree response for the code commit, with `truncated: false`. Case-insensitive LICENSE/NOTICE/COPYING path inspection found only root LICENSE. | No separately named NOTICE/COPYING file identified in that tree. This is not an audit of every source header, third-party dependency or training-data right. |

## Official Hugging Face attempts

The following exact official endpoints were attempted on the review date:

1. <https://huggingface.co/api/models/OleehyO/TexTeller/revision/7b96df06b9d81cdb129c3bef68b7250bc3e2b0ea>
2. <https://huggingface.co/OleehyO/TexTeller/raw/7b96df06b9d81cdb129c3bef68b7250bc3e2b0ea/README.md>
3. <https://huggingface.co/OleehyO/TexTeller/raw/7b96df06b9d81cdb129c3bef68b7250bc3e2b0ea/LICENSE>
4. <https://huggingface.co/OleehyO/TexTeller/raw/7b96df06b9d81cdb129c3bef68b7250bc3e2b0ea/NOTICE>

All four fetch-tool requests returned `error sending request`; no official body or HTTP status was obtained. API and README were additionally retried with Python urllib, an empty proxy handler, a 20-second request timeout and a 2 MiB response bound; both returned `URLError: <urlopen error timed out>`. Nothing in this directory is presented as an official HF model-card snapshot. Successful mirror responses do not supersede these failures.

## Preserved source mapping and fingerprints

`FETCH-LOG.txt` records successful direct retrieval URLs, UTC attempt times, sizes, SHA-256 values and the failed raw GitHub LICENSE request. Only small public text/metadata was requested, using no credentials or ambient proxy configuration, a 25-second request timeout and a 2 MiB body bound. No model binary URL was fetched. JSON response bodies have `.txt` suffixes solely for the packager's legal-file allowlist; their bytes are unchanged.

| Preserved file | Bytes | SHA-256 |
|---|---:|---|
| `LICENSE-APACHE-2.0.txt` | 11358 | `cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30` |
| `MODEL-CARD-MIRROR.md` | 1457 | `ac635a57419f02053817d7aa6922db9afffa563af8f7bcf1bd92775d99207ba4` |
| `HF-REVISION-MIRROR.txt` | 2030 | `2da53a12c7b0f016b47fce12a1082243cc13b16acd43cfd5c0b58fe0f7774156` |
| `SOURCE-TREE-GITHUB.txt` | 31682 | `247dabe5848f3c90542f62ae26153c39138cb5e612eac5f5d446e51a580ff74c` |
| `LICENSE-TEXTELLER-SOURCE.txt` | 11333 | `229eec032b2c84c01aa27d9aaf43bf9c93fa47144b97386ca832d9ac21b78317` |

After the raw GitHub LICENSE connection reset, the official API fallback succeeded with HTTP 200:
<https://api.github.com/repos/OleehyO/TexTeller/git/blobs/0b8fa30fa5e220d61c8c129cb86bb91e5842071b>.
The base64 content was decoded without text/newline normalization and its Git blob SHA-1 independently recomputed as `0b8fa30fa5e220d61c8c129cb86bb91e5842071b`, matching the LICENSE entry in the fixed commit's tree. The preserved LICENSE is the decoded upstream file, not the API JSON envelope. The logical source URL is <https://github.com/OleehyO/TexTeller/blob/12e6bb4312cc934bb4b69fcda2a1ac38e94930e6/LICENSE>.

## Conditions before redistribution

1. Obtain the official fixed HF revision metadata/card and inventory of applicable LICENSE, NOTICE, attribution and component exceptions. Confirm scope for both ONNX weights, configuration and tokenizer data; retain any additional applicable material. Resolve ambiguous tokenizer provenance with upstream rather than inferring it from a library license or the GitHub code license.
2. If Apache-2.0 is confirmed, supply the full license; retain applicable copyright, patent, trademark and attribution notices; comply with existing applicable NOTICE requirements. Apache-2.0 does not require fabricating a NOTICE file when upstream has none. These review records are not an upstream NOTICE.
3. Carry a prominent modification notice for each changed ONNX file with the actual derivative distribution. `MODIFICATIONS.md` identifies the two changes, but the eventual package's notice placement and file identification still require review under section 4(b); this evidence-only task did not edit binary metadata.
4. Independently verify actual final artifact hashes, source binding, exact package contents and all runtime/library licenses. Stored manifest hashes are not a fresh weight-file verification or license clearance.
5. Evaluate a representative handwriting/formula quality dataset and record limitations before any quality approval. Integrity checks, node counts, upstream card claims and FP32/INT8 agreement do not establish accuracy. Do not invent quality sign-off or a `REVIEWED.md` marker.

## Local validation boundary

Only `distribution/legal/model/` was written by this task. Converter, installer, requirements and local optimization manifest were read without executing conversion; only non-personal conversion parameters, relative source paths and hashes were extracted. No model download, GUI, desktop/microphone access, inference, commit, build or package publication was performed. Building would write outside this task's exclusive directory, and model distribution remains blocked. Other working-tree activity is outside this task.
