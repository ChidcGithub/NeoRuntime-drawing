import hashlib
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import prepare_texteller_int8 as prep


class PreparationTests(unittest.TestCase):
    def test_help_does_not_import_quantization_packages(self):
        result = subprocess.run([sys.executable, '-S', str(pathlib.Path(prep.__file__)), '--help'],
                                capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_paths_reject_source_nested_and_existing_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            source = root / 'source'
            source.mkdir()
            existing = root / 'existing'
            existing.mkdir()
            for destination in (source, source / 'child', root, existing):
                with self.assertRaises((ValueError, FileExistsError)):
                    prep.validate_paths(source, destination)
            self.assertEqual(prep.validate_paths(source, root / 'new')[1], (root / 'new').resolve())

    def test_corrupt_source_rejected_before_tools_or_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            source = root / 'source'
            source.mkdir()
            (source / 'encoder_model.onnx').write_bytes(b'not the official model')
            with mock.patch.object(prep.importlib.metadata, 'version') as version:
                with self.assertRaisesRegex(ValueError, 'verification'):
                    prep.prepare(source, root / 'result')
                version.assert_not_called()
            self.assertFalse((root / 'result').exists())

    def test_worker_cannot_overwrite_source_or_existing_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            source = root / 'source'
            source.mkdir()
            (source / prep.MODELS[0]).write_bytes(b'original')
            for destination in (source, root):
                with self.assertRaises(ValueError):
                    prep.worker(source, destination, prep.MODELS[0])
            self.assertEqual((source / prep.MODELS[0]).read_bytes(), b'original')

    def test_failed_worker_never_publishes_or_removes_source(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            source = root / 'source'
            source.mkdir()
            (source / 'keep').write_bytes(b'original')
            with mock.patch.object(prep, 'verify_source'), mock.patch.object(
                prep.importlib.metadata, 'version', side_effect=lambda name: prep.VERSIONS[name]
            ), mock.patch.object(prep.subprocess, 'run', side_effect=RuntimeError('worker failure')):
                with self.assertRaises(RuntimeError):
                    prep.prepare(source, root / 'result')
            self.assertEqual([p.name for p in root.iterdir()], ['source'])
            self.assertEqual((source / 'keep').read_bytes(), b'original')

    def test_streaming_hash(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = pathlib.Path(tmp) / 'input'
            p.write_bytes(b'abc')
            self.assertEqual(prep.sha256(p), hashlib.sha256(b'abc').hexdigest())


if __name__ == '__main__':
    unittest.main()
