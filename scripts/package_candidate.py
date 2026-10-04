"""Build an allowlisted candidate ZIP using only Python 3.11+ stdlib.

Run from the repository root (replace paths and the commit with reviewed inputs):
  python scripts/package_candidate.py --kind app --input-dir APP_DIR \
      --output-dir NEW_OUTPUT --legal-dir LEGAL_DIR --commit FULL_40_HEX_COMMIT
  python scripts/package_candidate.py --kind model --input-dir INT8_DIR \
      --output-dir NEW_OUTPUT --legal-dir LEGAL_DIR --commit FULL_40_HEX_COMMIT
  python -B -m unittest discover -s scripts -p test_package_candidate.py -v

The output parent must exist. Output must not exist or overlap input/legal;
output may be below project-dir, but may not replace it or its ancestors.
Only LICENSE and README.md are read from --project-dir (default: script's repo).
Legal input requires a nonempty root REVIEWED.md and at least one nonempty
LICENSE / LICENSE-* / LICENSE.txt / LICENSE.md (also in subdirectories).
Only UTF-8 .txt/.md, LICENSE/ LICENSE-*, NOTICE and COPYING files are included.

Output: candidate ZIP, SOURCE.json, FILES.sha256 (ZIP payload file hashes),
and SHA256SUMS (hashes of those three artifacts). FILES.sha256 is also inside
the ZIP and excludes itself to avoid self-reference. ZIP entries are sorted,
with fixed timestamps/permissions and no compression for reproducible bytes.
An upload-artifact workflow should upload this output, which already has a ZIP.

This is a technical gate, NOT license clearance, a build/provenance attestation,
ONNX validation, a runtime completeness check, or permission to publish. The
workflow maintainer must separately gate publication with manual compliance
review/checkboxes. Keep all inputs and the output parent private and unchanged
during packaging; this is not a sandbox against concurrent hostile writers.
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import sys
import tempfile
import zipfile
from pathlib import Path

VERSION = "0.0.1"
FORMAT = "texteller-dynamic-int8-v1"
REVISION = "7b96df06b9d81cdb129c3bef68b7250bc3e2b0ea"
MODEL_FILES = (
    "encoder_model.onnx", "decoder_model.onnx", "config.json",
    "generation_config.json", "tokenizer.json",
)
APP_FILES = ("neo-drawing.exe", "neo-blackboard.exe")
MODEL_PREFIX = "models/texteller-int8"
CHUNK_SIZE = 1024 * 1024
BINARY_LIMIT = 2 * 1024**3
JSON_LIMIT = 8 * 1024**2
TEXT_LIMIT = 10 * 1024**2
TOTAL_LIMIT = 8 * 1024**3
LEGAL_ENTRY_LIMIT = 4096
LEGAL_DEPTH_LIMIT = 16
DEFAULT_PROJECT = Path(__file__).absolute().parent.parent


class PackageError(ValueError):
    """An input failed the candidate package's technical checks."""


def checked_path(value):
    path = Path(value)
    if ".." in path.parts:
        raise PackageError(f"parent traversal is not allowed: {path}")
    path = path.absolute()
    for part in reversed((path, *path.parents)):
        try:
            info = part.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & getattr(
            stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400
        ):
            raise PackageError(f"symlink/reparse point is not allowed: {part}")
    return path.resolve()


def directory(value):
    path = checked_path(value)
    if not path.is_dir():
        raise PackageError(f"directory does not exist: {path}")
    return path


def overlaps(left, right):
    return left == right or left in right.parents or right in left.parents


def validate_paths(input_dir, output_dir, legal_dir, project_dir):
    source, legal, project = map(directory, (input_dir, legal_dir, project_dir))
    output = checked_path(output_dir)
    if overlaps(source, output) or overlaps(legal, output):
        raise PackageError("output and input/legal must be separate, non-nested directories")
    if output == project or output in project.parents:
        raise PackageError("output must not replace project-dir or its ancestors")
    if output.exists():
        raise FileExistsError(f"output already exists: {output}")
    directory(output.parent)
    return source, output, legal, project


def regular_file(path, limit):
    path = checked_path(path)
    info = path.stat()
    if not stat.S_ISREG(info.st_mode):
        raise PackageError(f"not a regular file: {path}")
    if not 0 < info.st_size <= limit:
        raise PackageError(f"empty or oversized file (limit {limit} bytes): {path}")
    return info


def fingerprint(info):
    identity = (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns)
    # Windows stat/fstat can disagree on ctime (creation vs. change time).
    return identity if os.name == "nt" else (*identity, info.st_ctime_ns)


def stream_file(path, limit, destination=None):
    """Bounded streaming hash/copy, checking the opened file and its final state."""
    before = regular_file(path, limit)
    digest, size = hashlib.sha256(), 0
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
    with os.fdopen(os.open(path, flags), "rb") as source:
        if fingerprint(os.fstat(source.fileno())) != fingerprint(before):
            raise PackageError(f"input changed before opening: {path}")
        while chunk := source.read(CHUNK_SIZE):
            size += len(chunk)
            if size > limit:
                raise PackageError(f"file exceeds size limit: {path}")
            digest.update(chunk)
            if destination is not None:
                destination.write(chunk)
        if size != before.st_size or fingerprint(os.fstat(source.fileno())) != fingerprint(before):
            raise PackageError(f"input changed during reading: {path}")
    if fingerprint(regular_file(path, limit)) != fingerprint(before):
        raise PackageError(f"input changed during reading: {path}")
    return digest.hexdigest(), size


def bounded_bytes(path, limit):
    regular_file(path, limit)
    with path.open("rb") as source:
        data = source.read(limit + 1)
    if len(data) > limit:
        raise PackageError(f"file exceeds size limit: {path}")
    return data


def text_file(path):
    text = bounded_bytes(path, TEXT_LIMIT).decode("utf-8-sig")
    if not text.strip() or any(ord(c) < 32 and c not in "\t\r\n" for c in text):
        raise PackageError(f"expected nonempty UTF-8 text: {path}")
    return text


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise PackageError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def reject_constant(value):
    raise PackageError(f"non-finite JSON constant: {value}")


def read_json(path):
    value = json.loads(bounded_bytes(path, JSON_LIMIT), object_pairs_hook=unique_object,
                       parse_constant=reject_constant)
    if not isinstance(value, dict):
        raise PackageError(f"expected JSON object: {path}")
    return value


def model_manifest(path):
    manifest = read_json(path)
    if manifest.get("format") != FORMAT or manifest.get("source_revision") != REVISION:
        raise PackageError("incorrect optimization format/source_revision")
    files = manifest.get("files")
    if not isinstance(files, dict) or set(files) != set(MODEL_FILES):
        raise PackageError("manifest files must contain exactly the five model filenames")
    for name, entry in files.items():
        limit = JSON_LIMIT if name.endswith(".json") else BINARY_LIMIT
        if not isinstance(entry, dict):
            raise PackageError(f"invalid manifest entry: {name}")
        size, digest = entry.get("bytes"), entry.get("sha256")
        if type(size) is not int or not 0 < size <= limit:
            raise PackageError(f"invalid manifest size: {name}")
        if not isinstance(digest, str) or re.fullmatch(r"[0-9a-fA-F]{64}", digest) is None:
            raise PackageError(f"invalid manifest SHA256: {name}")
    return files


def safe_component(name):
    # Archives are destined for Windows, even when built on another host.
    stem = name.split(".", 1)[0].upper()
    if (not name or name in (".", "..") or name.endswith((".", " "))
            or any(ord(c) < 32 or c in '<>:"/\\|?*' for c in name)
            or stem in {"CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"}
            or re.fullmatch(r"(?:COM|LPT)[0-9¹²³]", stem)):
        raise PackageError(f"unsafe archive path component: {name!r}")


def is_license(name):
    lower = name.lower()
    if lower.endswith((".txt", ".md")):
        lower = lower.rsplit(".", 1)[0]
    return re.fullmatch(r"license(?:[-_][a-z0-9_-]+)?", lower) is not None


def legal_files(root):
    selected, seen = [], set()
    pending, count = [(root, 0)], 0
    while pending:
        folder, depth = pending.pop()
        for path in sorted(folder.iterdir()):
            count += 1
            if count > LEGAL_ENTRY_LIMIT:
                raise PackageError("too many legal directory entries")
            checked_path(path)
            safe_component(path.name)
            relative = path.relative_to(root).as_posix()
            folded = relative.casefold()
            if folded in seen:
                raise PackageError(f"case-colliding legal path: {relative}")
            seen.add(folded)
            if path.is_dir():
                if depth >= LEGAL_DEPTH_LIMIT:
                    raise PackageError("legal directory nesting limit exceeded")
                pending.append((path, depth + 1))
            elif not path.is_file():
                raise PackageError(f"not a regular legal file: {path}")
            elif (path.suffix.lower() in (".txt", ".md") or is_license(path.name)
                              or path.name.upper() in ("NOTICE", "COPYING")):
                regular_file(path, TEXT_LIMIT)
                selected.append((path, f"legal/{relative}"))
    if not any(name == "legal/REVIEWED.md" for _, name in selected):
        raise PackageError("legal-dir requires a root REVIEWED.md")
    if not any(is_license(path.name) for path, _ in selected):
        raise PackageError("legal-dir requires at least one license text")
    return sorted(selected, key=lambda item: item[1])


def json_bytes(value):
    return (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True) + "\n").encode("utf-8")


def checksum_bytes(records):
    return "".join(f"{records[name][0]}  {name}\n" for name in sorted(records)).encode("utf-8")


def write_archive(payload, archive, names):
    with zipfile.ZipFile(archive, "x", compression=zipfile.ZIP_STORED, allowZip64=True) as output:
        for name in sorted(names):
            path = payload / name
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (stat.S_IFREG | 0o644) << 16
            info.compress_type = zipfile.ZIP_STORED
            info.file_size = path.stat().st_size
            with output.open(info, "w") as destination:
                stream_file(path, TOTAL_LIMIT, destination)


def package_candidate(*, kind, input_dir, output_dir, legal_dir, commit,
                      project_dir=DEFAULT_PROJECT):
    if kind not in ("model", "app"):
        raise PackageError("kind must be explicitly model or app")
    if not isinstance(commit, str) or re.fullmatch(r"[0-9a-fA-F]{40}", commit) is None:
        raise PackageError("commit must be exactly 40 hexadecimal characters")
    source, output, legal, project = validate_paths(input_dir, output_dir, legal_dir, project_dir)
    legal_entries = legal_files(legal)
    records, total = {}, 0
    archive_name = f"neo-drawing-{VERSION}-{kind}-candidate.zip"
    with tempfile.TemporaryDirectory(prefix=".package-candidate-", dir=output.parent) as temporary:
        staging = Path(temporary)
        payload = staging / "payload"
        payload.mkdir()

        def copy_file(path, name, limit, *, text=False):
            nonlocal total
            info = regular_file(path, limit)
            if total + info.st_size > TOTAL_LIMIT:
                raise PackageError("selected files exceed 8 GiB total limit")
            destination = payload / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            with destination.open("xb") as stream:
                record = stream_file(path, limit, stream)
            if text:
                text_file(destination)
            records[name] = record
            total += record[1]
            return record

        def generate(name, data):
            nonlocal total
            total += len(data)
            if total > TOTAL_LIMIT:
                raise PackageError("package payload exceeds 8 GiB total limit")
            (payload / name).write_bytes(data)
            records[name] = hashlib.sha256(data).hexdigest(), len(data)

        if kind == "model":
            manifest_name = f"{MODEL_PREFIX}/optimization.json"
            copy_file(source / "optimization.json", manifest_name, JSON_LIMIT)
            expected = model_manifest(payload / manifest_name)
            for name in MODEL_FILES:
                is_json = name.endswith(".json")
                target = f"{MODEL_PREFIX}/{name}"
                digest, size = copy_file(source / name, target, JSON_LIMIT if is_json else BINARY_LIMIT)
                if digest != expected[name]["sha256"].lower() or size != expected[name]["bytes"]:
                    raise PackageError(f"model hash/size mismatch: {name}")
                if is_json:
                    read_json(payload / target)
        else:
            for name in APP_FILES:
                copy_file(source / name, name, BINARY_LIMIT)
            dll = source / "DirectML.dll"
            checked_path(dll)
            if dll.exists():
                copy_file(dll, dll.name, BINARY_LIMIT)

        for name in ("LICENSE", "README.md"):
            copy_file(project / name, name, TEXT_LIMIT, text=True)
        for path, name in legal_entries:
            copy_file(path, name, TEXT_LIMIT, text=True)
        metadata = {
            "commit": commit.lower(), "kind": kind, "version": VERSION,
            "status": "candidate", "final_release": False,
            "technical_gate_only": True, "license_clearance": False,
            "runtime_completeness_verified": False,
            "commit_is_caller_supplied_not_build_attestation": True,
        }
        generate("SOURCE.json", json_bytes(metadata))
        guide = (
            "# Candidate package - NOT a final release\n\n"
            "Technical checks only: this is not license clearance or approval to publish.\n"
            "REVIEWED.md records the maintainer's supplied note, not an automated legal opinion.\n"
            "Maintainers must manually gate publication (compliance review/checkboxes),\n"
            "review all licenses/notices and static dependencies, and test clean-machine use.\n"
            "Neither runtime completeness nor model accuracy/ONNX safety is verified.\n"
            "The commit is caller-supplied; no build provenance is attested.\n\n"
            "App: executables are at ZIP root; DirectML.dll is included only if supplied.\n"
            "No other runtime DLLs, fonts, or models are implicitly bundled.\n"
            "Optional model: extract beside the executables, preserving models/texteller-int8/.\n"
            "Select that directory explicitly in the application's model loader.\n"
            "Review destination conflicts before extracting app/model archives together.\n\n"
            "LICENSE and README.md are verbatim project documents, not candidate clearance.\n"
            "Their source-release policy is not altered by this technical packaging tool.\n"
            "FILES.sha256 lists ZIP payload files except itself; SHA256SUMS outside the ZIP\n"
            "covers the ZIP, the standalone file list, and SOURCE.json. These are integrity\n"
            "checks, not signatures or proof of trusted origin.\n"
        )
        generate("CANDIDATE.md", guide.encode("utf-8"))
        generate("FILES.sha256", checksum_bytes(records))
        write_archive(payload, staging / archive_name, records)
        for name in ("SOURCE.json", "FILES.sha256"):
            shutil.copyfile(payload / name, staging / name)
        # ZIP_STORED has small header overhead beyond the bounded payload.
        artifacts = {name: stream_file(staging / name, TOTAL_LIMIT + 16 * 1024**2)
                     for name in (archive_name, "SOURCE.json", "FILES.sha256")}
        (staging / "SHA256SUMS").write_bytes(checksum_bytes(artifacts))
        shutil.rmtree(payload)
        checked_path(output)
        if output.exists():
            raise FileExistsError("output appeared during packaging; refusing replacement")
        staging.rename(output)
    return output


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--kind", required=True, choices=("model", "app"))
    parser.add_argument("--input-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--legal-dir", type=Path, required=True)
    parser.add_argument("--commit", required=True, help="caller-supplied full 40-hex source commit")
    parser.add_argument("--project-dir", type=Path, default=DEFAULT_PROJECT,
                        help="read only LICENSE and README.md; default: script's repository root")
    args = parser.parse_args(argv)
    try:
        output = package_candidate(**vars(args))
    except (OSError, ValueError, RecursionError) as error:
        print(f"Candidate packaging failed: {error}", file=sys.stderr)
        return 1
    print(f"Created candidate: {output}; technical gate only, NOT license clearance or final release")
    return 0


if __name__ == "__main__":
    sys.exit(main())
