"""Temporary dummy files only: no builds, real models, downloads, or GUI."""

import contextlib
import hashlib
import io
import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path
from typing import Any
from unittest import mock

import package_candidate as package

COMMIT = "0123456789abcdef0123456789abcdef01234567"


class CandidateTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.root / "input"
        self.legal = self.root / "legal"
        self.project = self.root / "project"
        for path in (self.source, self.legal, self.project):
            path.mkdir()
        (self.legal / "REVIEWED.md").write_text("Dummy review note; NOT legal clearance.\n", encoding="utf-8")
        (self.legal / "LICENSE.txt").write_text("Dummy license text for tests only.\n", encoding="utf-8")
        for name in ("LICENSE", "README.md"):
            (self.project / name).write_text(f"Dummy project {name}\n", encoding="utf-8")
        for name in package.APP_FILES:
            (self.source / name).write_bytes(b"dummy executable: " + name.encode())
        self.output = self.root / "result"

    def run_package(self, kind="app", /, **overrides: Any):
        arguments: dict[str, Any] = {
            "kind": kind, "input_dir": self.source, "output_dir": self.output,
            "legal_dir": self.legal, "project_dir": self.project, "commit": COMMIT,
        }
        arguments.update(overrides)
        return package.package_candidate(**arguments)

    def model(self):
        files = {}
        for name in package.MODEL_FILES:
            data = b'{"dummy": true}\n' if name.endswith(".json") else b"dummy ONNX " + name.encode()
            (self.source / name).write_bytes(data)
            files[name] = {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
        manifest = {"format": package.FORMAT, "source_revision": package.REVISION, "files": files}
        self.save_manifest(manifest)
        return manifest

    def save_manifest(self, manifest):
        (self.source / "optimization.json").write_text(json.dumps(manifest), encoding="utf-8")

    def archive(self, kind="app", output=None):
        return (output or self.output) / f"neo-drawing-{package.VERSION}-{kind}-candidate.zip"

    def assert_no_output(self):
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.root.glob(".package-candidate-*")), [])

    def symlink(self, link, target, directory=False):
        try:
            link.symlink_to(target, target_is_directory=directory)
        except (OSError, NotImplementedError) as error:
            self.skipTest(f"symlink creation unavailable: {error}")

    def test_app_exact_allowlist_and_metadata(self):
        (self.source / "private.txt").write_text("secret", encoding="utf-8")
        (self.source / "onnxruntime.dll").write_bytes(b"not allowlisted")
        (self.source / "models").mkdir()
        (self.source / "models" / "weights").write_bytes(b"not requested")
        (self.project / "secret.md").write_text("secret", encoding="utf-8")
        (self.legal / "secret.exe").write_bytes(b"not text")
        nested = self.legal / "component"
        nested.mkdir()
        (nested / "LICENSE-MIT").write_text("Dummy license", encoding="utf-8")
        (nested / "NOTICE.md").write_text("Dummy notice", encoding="utf-8")
        self.run_package()
        with zipfile.ZipFile(self.archive()) as archive:
            self.assertEqual(set(archive.namelist()), {
                *package.APP_FILES, "LICENSE", "README.md", "CANDIDATE.md", "SOURCE.json",
                "FILES.sha256", "legal/REVIEWED.md", "legal/LICENSE.txt",
                "legal/component/LICENSE-MIT", "legal/component/NOTICE.md",
            })
            metadata = json.loads(archive.read("SOURCE.json"))
            self.assertEqual(metadata["commit"], COMMIT)
            self.assertEqual(metadata["kind"], "app")
            self.assertEqual(metadata["version"], "0.0.1")
            self.assertEqual(metadata["status"], "candidate")
            for key in ("final_release", "license_clearance", "runtime_completeness_verified"):
                self.assertIs(metadata[key], False)
            self.assertIn(b"NOT a final release", archive.read("CANDIDATE.md"))
            self.assertIn(b"not license clearance", archive.read("CANDIDATE.md"))

    def test_extensionless_notices_preserved_at_root_and_nested(self):
        nested = self.legal / "component"
        nested.mkdir()
        for folder in (self.legal, nested):
            for name in ("NOTICE", "COPYING"):
                (folder / name).write_text("Required upstream attribution\n", encoding="utf-8")
        self.run_package()
        with zipfile.ZipFile(self.archive()) as archive:
            for name in ("NOTICE", "COPYING", "component/NOTICE", "component/COPYING"):
                self.assertEqual(archive.read("legal/" + name), (self.legal / name).read_bytes())

    def test_optional_directml_only_when_present(self):
        self.run_package()
        with zipfile.ZipFile(self.archive()) as archive:
            self.assertNotIn("DirectML.dll", archive.namelist())
        (self.source / "DirectML.dll").write_bytes(b"dummy DLL")
        other = self.root / "with-dll"
        self.run_package(output_dir=other)
        with zipfile.ZipFile(self.archive(output=other)) as archive:
            self.assertEqual(archive.read("DirectML.dll"), b"dummy DLL")

    def test_model_layout_and_hashes(self):
        self.model()
        (self.source / "encoder_model.onnx.data").write_bytes(b"not allowlisted")
        self.run_package("model")
        with zipfile.ZipFile(self.archive("model")) as archive:
            models = {name for name in archive.namelist() if name.startswith("models/")}
            self.assertEqual(models, {f"{package.MODEL_PREFIX}/{name}" for name in
                                      (*package.MODEL_FILES, "optimization.json")})
            self.assertNotIn("neo-drawing.exe", archive.namelist())
            records = self.parse_checksums(archive.read("FILES.sha256"))
            self.assertEqual(set(records), set(archive.namelist()) - {"FILES.sha256"})
            for name, digest in records.items():
                self.assertEqual(hashlib.sha256(archive.read(name)).hexdigest(), digest)
            self.assertEqual(archive.read("FILES.sha256"), (self.output / "FILES.sha256").read_bytes())
            self.assertEqual(archive.read("SOURCE.json"), (self.output / "SOURCE.json").read_bytes())
        outer = self.parse_checksums((self.output / "SHA256SUMS").read_bytes())
        self.assertEqual(set(outer), {self.archive("model").name, "FILES.sha256", "SOURCE.json"})
        self.assertEqual({p.name for p in self.output.iterdir()}, {*outer, "SHA256SUMS"})
        for name, digest in outer.items():
            self.assertEqual(hashlib.sha256((self.output / name).read_bytes()).hexdigest(), digest)

    @staticmethod
    def parse_checksums(data):
        return {name: digest for digest, name in (line.split("  ", 1) for line in data.decode().splitlines())}

    def test_deterministic_bytes_and_zip_metadata(self):
        self.model()
        self.run_package("model")
        for path in self.source.iterdir():
            os.utime(path, (1700000000, 1700000000))
        other = self.root / "again"
        self.run_package("model", output_dir=other)
        for path in self.output.iterdir():
            self.assertEqual(path.read_bytes(), (other / path.name).read_bytes())
        with zipfile.ZipFile(self.archive("model")) as archive:
            self.assertEqual(archive.namelist(), sorted(archive.namelist()))
            for entry in archive.infolist():
                self.assertEqual(entry.date_time, (1980, 1, 1, 0, 0, 0))
                self.assertEqual(entry.compress_type, zipfile.ZIP_STORED)
                self.assertEqual(entry.external_attr >> 16, stat.S_IFREG | 0o644)
                self.assertEqual(entry.create_system, 3)
                self.assertEqual(entry.comment, b"")

    def test_model_tampering_each_file_rejected(self):
        for name in package.MODEL_FILES:
            with self.subTest(name=name):
                self.model()
                path = self.source / name
                original = path.read_bytes()
                path.write_bytes(b"!" + original[1:])
                with self.assertRaisesRegex(package.PackageError, "hash/size mismatch"):
                    self.run_package("model")
                self.assert_no_output()

    def test_manifest_validation(self):
        for problem in ("format", "revision", "missing", "extra", "traversal", "size", "bool-size",
                        "over-limit", "digest", "wrong-digest", "not-object", "not-files-object"):
            with self.subTest(problem=problem):
                manifest = self.model()
                entry = manifest["files"][package.MODEL_FILES[0]]
                if problem == "format":
                    manifest["format"] = "other"
                elif problem == "revision":
                    manifest["source_revision"] = "0" * 40
                elif problem == "missing":
                    del manifest["files"]["tokenizer.json"]
                elif problem in ("extra", "traversal"):
                    manifest["files"]["../private" if problem == "traversal" else "extra"] = entry
                elif problem == "size":
                    entry["bytes"] += 1
                elif problem == "bool-size":
                    entry["bytes"] = True
                elif problem == "over-limit":
                    entry["bytes"] = package.BINARY_LIMIT + 1
                elif problem == "digest":
                    entry["sha256"] = "not SHA256"
                elif problem == "wrong-digest":
                    entry["sha256"] = "0" * 64
                elif problem == "not-object":
                    manifest["files"][package.MODEL_FILES[0]] = []
                else:
                    manifest["files"] = []
                self.save_manifest(manifest)
                with self.assertRaises(package.PackageError):
                    self.run_package("model")
                self.assert_no_output()

    def test_invalid_duplicate_and_nonfinite_json_rejected(self):
        for data in (b"{", b"[]", b'{"format":1,"format":2}', b'{"value":NaN}'):
            with self.subTest(data=data):
                self.model()
                (self.source / "optimization.json").write_bytes(data)
                with self.assertRaises(ValueError):
                    self.run_package("model")
                self.assert_no_output()

    def test_hashed_but_invalid_model_json_rejected(self):
        manifest = self.model()
        data = b"not JSON"
        (self.source / "config.json").write_bytes(data)
        manifest["files"]["config.json"] = {"sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}
        self.save_manifest(manifest)
        with self.assertRaises(ValueError):
            self.run_package("model")
        self.assert_no_output()

    def test_missing_required_inputs(self):
        for path in (self.source / package.APP_FILES[0], self.project / "LICENSE",
                     self.project / "README.md", self.legal / "REVIEWED.md", self.legal / "LICENSE.txt"):
            with self.subTest(path=path):
                saved = path.read_bytes()
                path.unlink()
                with self.assertRaises((OSError, ValueError)):
                    self.run_package()
                self.assert_no_output()
                path.write_bytes(saved)
        self.model()
        (self.source / "decoder_model.onnx").unlink()
        with self.assertRaises(OSError):
            self.run_package("model")
        self.assert_no_output()

    def test_commit_and_kind_required_and_strict(self):
        for commit in ("", "abc", "g" * 40, "0" * 39, "0" * 41, COMMIT + "\n"):
            with self.subTest(commit=commit), self.assertRaises(package.PackageError):
                self.run_package(commit=commit)
        with self.assertRaises(package.PackageError):
            self.run_package("auto")
        self.assert_no_output()
        self.run_package(commit=COMMIT.upper())
        self.assertEqual(json.loads((self.output / "SOURCE.json").read_bytes())["commit"], COMMIT)

    def test_paths_reject_existing_nested_traversal_missing_and_source_overwrite(self):
        existing = self.root / "existing"
        existing.mkdir()
        marker = existing / "keep"
        marker.write_bytes(b"unchanged")
        existing_file = self.root / "file"
        existing_file.write_bytes(b"unchanged")
        for output in (self.source, self.source / "child", self.root, existing, existing_file,
                       self.legal / "child", self.project, self.root / "missing" / "child",
                       self.root / "input" / ".." / "new"):
            with self.subTest(output=output), self.assertRaises((OSError, ValueError)):
                self.run_package(output_dir=output)
        for key in ("input_dir", "legal_dir", "project_dir"):
            with self.subTest(key=key), self.assertRaises(package.PackageError):
                self.run_package(**{key: self.root / "absent"})
        self.assertEqual(marker.read_bytes(), b"unchanged")
        self.assertEqual(existing_file.read_bytes(), b"unchanged")
        self.assert_no_output()

    def test_output_may_be_inside_project_without_scanning_it(self):
        output = self.project / "candidate"
        with mock.patch.object(Path, "rglob", side_effect=AssertionError("no repository scan")):
            self.run_package(output_dir=output)
        self.assertTrue(self.archive(output=output).is_file())

    def test_symlink_selected_file_and_legal_entry_rejected(self):
        target = self.root / "target"
        target.write_bytes(b"dummy")
        paths = [self.source / "DirectML.dll", self.legal / "ignored.bin",
                 self.legal / "linked.txt", self.project / "LICENSE"]
        self.model()
        paths.append(self.source / "encoder_model.onnx")
        for path in paths:
            with self.subTest(path=path):
                if path.exists():
                    path.unlink()
                self.symlink(path, target)
                try:
                    with self.assertRaisesRegex(package.PackageError, "symlink/reparse"):
                        self.run_package("model" if path.name.endswith(".onnx") else "app")
                    self.assert_no_output()
                finally:
                    path.unlink()
                    if path.name == "LICENSE":
                        path.write_bytes(b"Dummy license")

    def test_symlink_directories_and_dangling_output_rejected(self):
        link = self.root / "alias"
        self.symlink(link, self.source, directory=True)
        for overrides in ({"input_dir": link}, {"output_dir": link / "new"}):
            with self.assertRaisesRegex(package.PackageError, "symlink/reparse"):
                self.run_package(**overrides)
        self.symlink(self.output, self.root / "absent", directory=True)
        with self.assertRaisesRegex(package.PackageError, "symlink/reparse"):
            self.run_package()
        self.assertTrue(self.output.is_symlink())

    def test_windows_junction_rejected(self):
        if os.name != "nt":
            self.skipTest("Windows junction test")
        link = self.root / "junction"
        result = subprocess.run(["cmd", "/c", "mklink", "/J", str(link), str(self.source)],
                                capture_output=True, timeout=10)
        if result.returncode:
            self.skipTest(f"junction creation unavailable: {result.stderr!r}")
        try:
            with self.assertRaisesRegex(package.PackageError, "symlink/reparse"):
                self.run_package(input_dir=link)
        finally:
            link.rmdir()

    def test_legal_text_and_size_restrictions(self):
        path = self.legal / "NOTICE.txt"
        for data in (b"\x00binary", b"\xff", b" \n"):
            with self.subTest(data=data):
                path.write_bytes(data)
                with self.assertRaises(ValueError):
                    self.run_package()
                self.assert_no_output()
        path.write_bytes(b"x" * 65)
        with mock.patch.object(package, "TEXT_LIMIT", 64):
            with self.assertRaisesRegex(package.PackageError, "oversized"):
                self.run_package()
        self.assert_no_output()

    def test_limits_without_large_files(self):
        with mock.patch.object(package, "BINARY_LIMIT", 1):
            with self.assertRaisesRegex(package.PackageError, "oversized"):
                self.run_package()
        self.assert_no_output()
        self.model()
        with mock.patch.object(package, "JSON_LIMIT", 8):
            with self.assertRaisesRegex(package.PackageError, "oversized"):
                self.run_package("model")
        self.assert_no_output()
        with mock.patch.object(package, "TOTAL_LIMIT", 40):
            with self.assertRaisesRegex(package.PackageError, "total limit"):
                self.run_package()
        self.assert_no_output()
        with mock.patch.object(package, "LEGAL_ENTRY_LIMIT", 1):
            with self.assertRaisesRegex(package.PackageError, "too many"):
                self.run_package()
        self.assert_no_output()

    def test_unsafe_legal_names_and_depth(self):
        for name in ("../x", "a\\b", "a:b", "line\nfeed", "NUL", "CON.txt", "COM1.md", "end."):
            with self.subTest(name=name), self.assertRaises(package.PackageError):
                package.safe_component(name)
        nested = self.legal / "nested"
        nested.mkdir()
        with mock.patch.object(package, "LEGAL_DEPTH_LIMIT", 0):
            with self.assertRaisesRegex(package.PackageError, "nesting limit"):
                self.run_package()
        self.assert_no_output()

    def test_case_colliding_legal_paths(self):
        one, two = self.legal / "notice.md", self.legal / "NOTICE.md"
        one.write_bytes(b"first")
        if two.exists():
            self.skipTest("case-insensitive filesystem")
        two.write_bytes(b"second")
        with self.assertRaisesRegex(package.PackageError, "case-colliding"):
            self.run_package()
        self.assert_no_output()

    def test_link_metadata_rejected_without_symlink_privileges(self):
        # Windows runners can expose TEMP through its 8.3 alias; the packager
        # resolves the parent before reaching the selected file. Compare the
        # same canonical path without resolving inside the patched lstat.
        path = (self.source / package.APP_FILES[0]).resolve()
        original = Path.lstat
        for mode, attributes in ((stat.S_IFLNK | 0o777, 0), (stat.S_IFREG | 0o644, 0x400)):
            def injected(candidate, *args, mode=mode, attributes=attributes, **kwargs):
                if candidate == path:
                    return mock.Mock(st_mode=mode, st_file_attributes=attributes)
                return original(candidate, *args, **kwargs)

            with self.subTest(mode=mode, attributes=attributes):
                with mock.patch.object(Path, "lstat", injected):
                    with self.assertRaisesRegex(package.PackageError, "symlink/reparse"):
                        self.run_package()
                self.assert_no_output()

    def test_empty_and_nonregular_selected_files_rejected(self):
        path = self.source / package.APP_FILES[0]
        path.write_bytes(b"")
        with self.assertRaisesRegex(package.PackageError, "empty or oversized"):
            self.run_package()
        self.assert_no_output()
        path.unlink()
        path.mkdir()
        with self.assertRaisesRegex(package.PackageError, "not a regular file"):
            self.run_package()
        self.assert_no_output()

    def test_final_rename_failure_cleans_staging(self):
        with mock.patch.object(Path, "rename", side_effect=OSError("rename failure")):
            with self.assertRaisesRegex(OSError, "rename failure"):
                self.run_package()
        self.assert_no_output()

    def test_streaming_hash_and_mutation_check(self):
        path = self.root / "stream"
        data = b"0123456789" * 1000
        path.write_bytes(data)
        destination = io.BytesIO()
        with mock.patch.object(package, "CHUNK_SIZE", 17), mock.patch.object(
            Path, "read_bytes", side_effect=AssertionError("hash must stream")
        ):
            digest, size = package.stream_file(path, len(data), destination)
        self.assertEqual(digest, hashlib.sha256(data).hexdigest())
        self.assertEqual(size, len(data))
        self.assertEqual(destination.getvalue(), data)
        before = path.stat()
        changed = mock.Mock(st_dev=before.st_dev, st_ino=before.st_ino,
                            st_size=before.st_size, st_mtime_ns=before.st_mtime_ns + 1,
                            st_ctime_ns=before.st_ctime_ns)
        with mock.patch.object(package, "regular_file", side_effect=[before, changed]):
            with self.assertRaisesRegex(package.PackageError, "changed during"):
                package.stream_file(path, len(data))

    def test_copy_and_zip_failures_leave_no_output_or_source_changes(self):
        original = (self.source / package.APP_FILES[0]).read_bytes()
        for target in ("stream_file", "write_archive"):
            with self.subTest(target=target):
                with mock.patch.object(package, target, side_effect=OSError("injected failure")):
                    with self.assertRaisesRegex(OSError, "injected failure"):
                        self.run_package()
                self.assert_no_output()
                self.assertEqual((self.source / package.APP_FILES[0]).read_bytes(), original)

    def test_output_appearing_during_packaging_is_preserved(self):
        real_archive = package.write_archive

        def intervening_writer(*args):
            real_archive(*args)
            self.output.mkdir()
            (self.output / "keep").write_bytes(b"other writer")

        with mock.patch.object(package, "write_archive", side_effect=intervening_writer):
            with self.assertRaisesRegex(FileExistsError, "appeared"):
                self.run_package()
        self.assertEqual((self.output / "keep").read_bytes(), b"other writer")
        self.assertEqual(list(self.root.glob(".package-candidate-*")), [])

    def test_cli_help_requires_no_site_packages(self):
        result = subprocess.run([sys.executable, "-B", "-S", package.__file__, "--help"],
                                capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(b"NOT license clearance", result.stdout)
        self.assertIn(b"--project-dir", result.stdout)

    def test_cli_required_flags_and_success(self):
        arguments = ["--kind", "app", "--input-dir", str(self.source),
                     "--output-dir", str(self.output), "--legal-dir", str(self.legal),
                     "--commit", COMMIT, "--project-dir", str(self.project)]
        for flag in ("--kind", "--input-dir", "--output-dir", "--legal-dir", "--commit"):
            missing = arguments.copy()
            index = missing.index(flag)
            del missing[index:index + 2]
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
                package.main(missing)
            self.assertEqual(error.exception.code, 2)
        result = subprocess.run([sys.executable, "-B", "-S", package.__file__, *arguments],
                                capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.archive().is_file())

    def test_cli_failure_is_actionable_without_traceback(self):
        result = subprocess.run([
            sys.executable, "-B", "-S", package.__file__, "--kind", "app",
            "--input-dir", str(self.source), "--output-dir", str(self.output),
            "--legal-dir", str(self.legal), "--project-dir", str(self.project), "--commit", "invalid",
        ], capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"40 hexadecimal", result.stderr)
        self.assertNotIn(b"Traceback", result.stderr)
        self.assert_no_output()


if __name__ == "__main__":
    unittest.main()
