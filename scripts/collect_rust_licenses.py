"""Offline legal-text collector; never invokes Cargo, a build, or the network.

Run from the project root with Python 3.11+. See the Rust legal README for
scope and limitations. The supplied metadata is input, not a release SBOM.
"""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "distribution/legal/app/rust"
LEGAL = re.compile(r"license|licence|notice|copyright|copying|unlicense", re.I)
FONT_TEXTS = {"Hack-Regular.txt", "OFL.txt", "UFL.txt", "emoji-icon-font-mit-license.txt"}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def relative(value):
    p = PurePosixPath(value.replace("\\", "/"))
    if p.is_absolute() or ".." in p.parts or ":" in str(p):
        raise ValueError("Unsafe relative evidence path")
    return p.as_posix()


def is_legal(path):
    p = PurePosixPath(path)
    return bool(LEGAL.search(p.name) or
                any(x.lower() in ("licenses", "licences", "notices") for x in p.parts[:-1]) or
                (p.name.lower().startswith("font") and p.suffix.lower() == ".txt"))


def put(path, data, check):
    if check:
        if not path.is_file() or path.read_bytes() != data:
            raise ValueError("Output differs: " + path.relative_to(ROOT).as_posix())
    elif path.exists():
        if path.read_bytes() != data:
            raise ValueError("Refusing to replace existing text: " + path.relative_to(ROOT).as_posix())
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)


def store(data, check):
    # Deduplicate only identical bytes, never by SPDX or inferred ownership.
    name = "texts/" + digest(data) + ".txt"
    put(OUTPUT / name, data, check)
    return name


def json_bytes(value):
    return (json.dumps(value, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def collect(metadata_path, check):
    raw = metadata_path.read_bytes()
    metadata = json.loads(raw.decode("utf-8-sig"))
    lock_raw = (ROOT / "Cargo.lock").read_bytes()
    locked = {(p["name"], p["version"], p.get("source")): p
              for p in tomllib.loads(lock_raw.decode("utf-8"))["package"]}
    members = set(metadata["workspace_members"])
    packages = sorted((p for p in metadata["packages"] if p["id"] not in members),
                      key=lambda p: (p["name"], p["version"], p.get("source") or ""))
    upstream_raw = (OUTPUT / "upstream/index.json").read_bytes()
    upstream = json.loads(upstream_raw)["files"]
    for item in upstream:
        data = (OUTPUT / relative(item["file"])).read_bytes()
        if digest(data) != item["sha256"] or len(data) != item["bytes"]:
            raise ValueError("Upstream evidence hash mismatch")
        if hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest() != item["git_blob_sha1"]:
            raise ValueError("Upstream Git blob mismatch")
    inventory = []
    for p in packages:
        name, version, source = p["name"], p["version"], p.get("source")
        if source != "registry+https://github.com/rust-lang/crates.io-index":
            raise ValueError("Unsupported external source; manual review required: " + name)
        root = Path(p["manifest_path"]).parent
        if not root.is_dir():
            raise ValueError("Local crate unavailable: " + name + " " + version)
        manifest_raw = (root / "Cargo.toml").read_bytes()
        manifest = tomllib.loads(manifest_raw.decode("utf-8"))["package"]
        if (manifest["name"], manifest["version"], manifest.get("license")) != (name, version, p["license"]):
            raise ValueError("Metadata/manifest mismatch: " + name)
        lock = locked.get((name, version, source))
        if not lock:
            raise ValueError("Package absent from Cargo.lock: " + name)
        vcs_path = root / ".cargo_vcs_info.json"
        vcs_raw = vcs_path.read_bytes() if vcs_path.exists() else None
        vcs = json.loads(vcs_raw) if vcs_raw else {}
        commit = vcs.get("git", {}).get("sha1")
        vcs_subdir = vcs.get("path_in_vcs")
        if vcs_subdir is not None:
            vcs_subdir = relative(vcs_subdir)
        files = []
        pending = ["Final-build inclusion, file-level obligations and attribution review pending"]
        selected_paths = []
        for f in sorted(root.rglob("*")):
            if not f.is_file():
                continue
            rel = f.relative_to(root).as_posix()
            if is_legal(rel) or (name == "epaint_default_fonts" and f.name in FONT_TEXTS):
                selected_paths.append(rel)
        declared = p.get("license_file")
        if declared:
            declared_path = Path(declared)
            if declared_path.is_absolute():
                declared = declared_path.relative_to(root).as_posix()
            declared = relative(declared)
            if (root / declared).is_file():
                selected_paths.append(declared)
            else:
                pending.append("Declared license_file missing: " + declared)
        archive = root.parent.parent.parent / "cache" / root.parent.name / (root.name + ".crate")
        archive_verified = False
        tar = None
        if archive.is_file():
            if digest(archive.read_bytes()) != lock.get("checksum"):
                raise ValueError("Cached archive/lock checksum mismatch: " + name)
            tar = tarfile.open(archive, "r:gz")
            archive_verified = True
        else:
            pending.append("Cached archive unavailable; package authenticity not independently checked")
        try:
            def local_bytes(rel):
                f = root / relative(rel)
                if f.is_symlink() or not f.resolve().is_relative_to(root.resolve()):
                    raise ValueError("Crate evidence escapes package: " + name)
                data = f.read_bytes()
                if tar:
                    member = tar.extractfile(root.name + "/" + rel)
                    if member is None or member.read() != data:
                        raise ValueError("Local file/archive mismatch: " + name + "/" + rel)
                return data

            local_bytes("Cargo.toml")
            if vcs_raw:
                local_bytes(".cargo_vcs_info.json")
            for rel in sorted(set(selected_paths)):
                data = local_bytes(rel)
                if name == "epaint_default_fonts" and rel.startswith("fonts/"):
                    file = "../fonts/" + PurePosixPath(rel).name
                    if not (OUTPUT / file).is_file() or (OUTPUT / file).read_bytes() != data:
                        raise ValueError("Font handoff missing or differs: " + rel)
                    origin = "font-review-reference"
                else:
                    file = store(data, check)
                    origin = "local-crate"
                files.append(dict(file=file, source_path=rel, sha256=digest(data),
                                  bytes=len(data), origin=origin, archive_bytes_verified=archive_verified))
            # Retain complete legal comment blocks, not entire large API registries.
            # These are additional evidence, not substitutes for full license files.
            if name in ("khronos_api", "spirv", "gl_generator") or name.startswith("accesskit"):
                candidates = ([f.relative_to(root).as_posix() for f in root.rglob("*.xml")]
                              if name == "khronos_api" else
                              ["autogen_spirv.rs", "lib.rs"] if name == "spirv" else
                              ["src/lib.rs"] if name.startswith("accesskit") else ["lib.rs"])
                for rel in sorted(candidates):
                    data = local_bytes(rel)
                    if rel.endswith(".xml"):
                        blocks = list(re.finditer(rb"<!--.*?-->|<comment>.*?</comment>", data, re.S))
                    else:
                        blocks = list(re.finditer(rb"\A(?://[^\n]*\n|\s*\n)+", data))
                    for block in blocks:
                        text = block.group()
                        if not re.search(rb"copyright|license|licence|permission", text, re.I):
                            continue
                        file = store(text, check)
                        files.append(dict(file=file, source_path=rel, origin="local-legal-comment",
                                          sha256=digest(text), bytes=len(text), source_sha256=digest(data),
                                          byte_start=block.start(), byte_end=block.end(),
                                          archive_bytes_verified=archive_verified))
            for readme in sorted(root.glob("*")):
                if not readme.is_file() or not readme.name.lower().startswith("readme"):
                    continue
                data = local_bytes(readme.name)
                headings = list(re.finditer(rb"(?m)^(#{1,6})[ \t]+([^\r\n]+)", data))
                for i, heading in enumerate(headings):
                    if not re.search(rb"licen[cs]e|copyright|attribution|third.party", heading[2], re.I):
                        continue
                    end = next((h.start() for h in headings[i + 1:] if len(h[1]) <= len(heading[1])), len(data))
                    text = data[heading.start():end]
                    file = store(text, check)
                    files.append(dict(file=file, source_path=readme.name, origin="local-readme-section",
                                      sha256=digest(text), bytes=len(text), source_sha256=digest(data),
                                      byte_start=heading.start(), byte_end=end,
                                      archive_bytes_verified=archive_verified))
        finally:
            if tar:
                tar.close()
        local_count = sum(f["origin"] == "local-crate" for f in files)
        for item in upstream:
            if name in item["crate_names"] and commit == item["crate_commit"]:
                files.append({k: v for k, v in item.items() if k not in ("crate_names", "crate_commit")} |
                             {"origin": "fixed-github", "archive_bytes_verified": False})
        if not any(f["origin"] in ("local-crate", "fixed-github") for f in files):
            pending.append("No standalone crate license text collected")
        if name == "epaint_default_fonts":
            pending.append("Font obligations delegated to ../fonts/ATTRIBUTION.md and ../fonts/README.md; retain all four texts")
        if name == "khronos_api":
            pending.append("WebGL specification/extension terms and generated-binding attribution applicability pending; root Apache alone is not complete clearance")
        if name == "spirv":
            pending.append("SPIRV-Headers submodule texts retained conservatively; generated-code and specification applicability pending")
        license_raw = p["license"]
        spdx = re.sub(r"\s*/\s*", " OR ", license_raw) if license_raw else None
        inventory.append(dict(name=name, version=version, source=source,
                              spdx_declared=spdx, cargo_license=license_raw,
                              spdx_normalization="Cargo legacy slash means OR" if spdx != license_raw else None,
                              repository=p.get("repository"), selected_license=None,
                              license_selection_status="pending", review_status="pending",
                              evidence=dict(manifest_sha256=digest(manifest_raw),
                                            cargo_vcs_info_sha256=digest(vcs_raw) if vcs_raw else None,
                                            vcs_commit=commit, path_in_vcs=vcs_subdir,
                                            cargo_lock_checksum=lock.get("checksum"),
                                            cached_archive_sha256_verified=archive_verified),
                              local_standalone_text_count=local_count, files=files, pending=pending))
    missing = [{"name": p["name"], "version": p["version"], "reason": "No standalone crate license text"}
               for p in inventory if not any(f["origin"] in ("local-crate", "fixed-github") for f in p["files"])]
    unique = {f["file"]: f for p in inventory for f in p["files"] if not f["file"].startswith("../")}
    result = dict(schema_version=1, status="pending-review", not_sbom=True,
                  scope="All external packages in supplied Windows-filtered Cargo metadata; conservatively overinclusive, not final linked-binary contents",
                  metadata=dict(sha256=digest(raw), format_version=metadata["version"],
                                supplied_target="x86_64-pc-windows-msvc (caller-provided; metadata does not encode filter command)"),
                  cargo_lock_sha256=digest(lock_raw), upstream_index_sha256=digest(upstream_raw),
                  upstream_tree_evidence_sha256=digest((OUTPUT / "upstream/tree-evidence.json").read_bytes()),
                  font_review="../fonts/ATTRIBUTION.md",
                  coverage=dict(external_crates=len(inventory),
                                crates_with_local_standalone_texts=sum(p["local_standalone_text_count"] > 0 for p in inventory),
                                crates_with_fixed_github_texts=sum(any(f["origin"] == "fixed-github" for f in p["files"]) for p in inventory),
                                crates_with_standalone_texts=len(inventory) - len(missing),
                                missing_standalone_texts=len(missing),
                                unique_rust_text_files=len(unique), unique_rust_text_bytes=sum(f["bytes"] for f in unique.values()),
                                pending_license_selections=len(inventory)),
                  missing=missing, packages=inventory)
    data = json_bytes(result)
    if check:
        put(OUTPUT / "inventory.json", data, True)
    else:
        (OUTPUT / "inventory.json").write_bytes(data)
    print(json.dumps(result["coverage"], indent=2))
    return 1 if missing else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--metadata", type=Path, default=ROOT / ".dbg/license-build-metadata.json")
    parser.add_argument("--check", action="store_true", help="Verify source bytes, hashes and deterministic inventory without writing")
    args = parser.parse_args()
    try:
        return collect(args.metadata, args.check)
    except (OSError, ValueError, KeyError, tarfile.TarError) as error:
        # OSError paths may include private cache locations; do not print them.
        print("Collection failed: " + (type(error).__name__ if isinstance(error, OSError) else str(error)), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
