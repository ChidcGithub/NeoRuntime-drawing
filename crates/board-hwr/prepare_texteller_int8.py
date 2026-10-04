"""Offline INT8 preparation for pinned TexTeller weights; run on a high-memory PC.

Requires Python 3.11/3.12 and requirements-quantize.txt. Never downloads models,
executes model-provided code, or overwrites the source/destination. Recognition
quality must be evaluated separately; INT8 is not an accuracy guarantee.
"""
import argparse
import hashlib
import importlib.metadata
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile

from download_texteller import FILES, REVISION, verified

VERSIONS = {"onnx": "1.18.0", "onnxruntime": "1.22.0"}
MODELS = ("encoder_model.onnx", "decoder_model.onnx")


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def validate_paths(source, output):
    source = source.resolve(strict=True)
    output = output.resolve()
    if not source.is_dir() or output == source or source in output.parents or output in source.parents:
        raise ValueError("source and output must be separate, non-nested directories")
    if output.exists():
        raise FileExistsError("output already exists; choose a new directory")
    if not output.parent.is_dir():
        raise ValueError("output parent must already exist")
    return source, output


def verify_source(source):
    for name, (size, algorithm, digest) in FILES.items():
        path = source / name
        if path.is_symlink() or not verified(path, size, algorithm, digest):
            raise ValueError(f"pinned source verification failed: {name}")


def reject_external(message):
    # Inspect every nested TensorProto, including tensors in attribute subgraphs.
    # Never let ONNX resolve model-supplied filesystem paths.
    if message.DESCRIPTOR.full_name == "onnx.TensorProto":
        if message.external_data or message.data_location != 0:
            raise ValueError("external tensor data is not supported")
    for field, value in message.ListFields():
        if field.type == field.TYPE_MESSAGE:
            if field.is_repeated:
                for child in value:
                    reject_external(child)
            else:
                reject_external(value)


def signature(model):
    return {
        key: [(v.name, v.type.SerializeToString().hex()) for v in getattr(model.graph, key)]
        for key in ("input", "output")
    }


def worker(source, output, name):
    source = source.resolve(strict=True)
    output = output.resolve(strict=True)
    if source == output or source in output.parents or output in source.parents:
        raise ValueError("worker destination must be separate from source")
    if not output.is_dir() or any(output.iterdir()):
        # Parent uses a separate empty worker directory for each model.
        raise ValueError("worker destination must be an empty directory")
    # Recheck the exact official file in each fresh worker before parsing protobuf.
    size, algorithm, digest = FILES[name]
    if (source / name).is_symlink() or not verified(source / name, size, algorithm, digest):
        raise ValueError("source model changed before conversion")
    import onnx
    from onnxruntime.quantization import QuantType, quantize_dynamic
    model = onnx.load_model(source / name, load_external_data=False)
    reject_external(model)
    original_signature = signature(model)
    quantize_dynamic(
        model, str(output / name), weight_type=QuantType.QInt8,
        op_types_to_quantize=["MatMul", "Gemm"], per_channel=True,
        reduce_range=True, use_external_data_format=False,
        extra_options={"MatMulConstBOnly": True},
    )
    del model
    converted = onnx.load_model(output / name, load_external_data=False)
    reject_external(converted)
    if signature(converted) != original_signature:
        raise ValueError("conversion changed model I/O signature")
    nodes = sum(n.op_type in ("MatMulInteger", "DynamicQuantizeLinear") for n in converted.graph.node)
    if not nodes:
        raise ValueError("conversion produced no dynamic quantization nodes")
    onnx.checker.check_model(converted)
    (output / (name + ".json")).write_text(json.dumps({
        "source_sha256": sha256(source / name), "sha256": sha256(output / name),
        "bytes": (output / name).stat().st_size, "dynamic_quantization_nodes": nodes,
    }, indent=2), encoding="utf-8")


def prepare(source, output):
    source, output = validate_paths(source, output)
    verify_source(source)
    versions = {name: importlib.metadata.version(name) for name in VERSIONS}
    if versions != VERSIONS:
        raise ValueError(f"use pinned conversion environment: {VERSIONS}; found {versions}")
    # Conversion needs far more memory than inference. Processes run sequentially
    # so large ONNX protobuf/quantization buffers cannot accumulate across models.
    with tempfile.TemporaryDirectory(prefix=".texteller-int8-", dir=output.parent) as tmp:
        staging = pathlib.Path(tmp)
        for name in MODELS:
            work = staging / (name + ".work")
            work.mkdir()
            subprocess.run([sys.executable, str(pathlib.Path(__file__).resolve()),
                            "--worker", name, "--source", str(source), "--output", str(work)],
                           check=True, timeout=900)
            for file in work.iterdir():
                file.rename(staging / file.name)
            work.rmdir()
        manifest = {"format": "texteller-dynamic-int8-v1", "source_revision": REVISION,
                    "tools": versions, "operators": ["MatMul", "Gemm"],
                    "weight_type": "QInt8", "per_channel": True, "reduce_range": True,
                    "quality_validated": False, "files": {}}
        for name in MODELS:
            report = staging / (name + ".json")
            manifest["files"][name] = json.loads(report.read_text(encoding="utf-8"))
            report.unlink()
        for name in sorted(FILES.keys() - set(MODELS)):
            size, algorithm, digest = FILES[name]
            if (source / name).is_symlink() or not verified(source / name, size, algorithm, digest):
                raise ValueError(f"source configuration changed: {name}")
            shutil.copy2(source / name, staging / name)
            if not verified(staging / name, size, algorithm, digest):
                raise ValueError(f"copied configuration failed verification: {name}")
            manifest["files"][name] = {"sha256": sha256(staging / name), "bytes": (staging / name).stat().st_size}
        (staging / "optimization.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
        # All files and checks succeed before making the new directory visible.
        # On Windows rename refuses an existing destination; repeat the check for other hosts.
        if output.exists():
            raise FileExistsError("output appeared during conversion; refusing replacement")
        staging.rename(output)
    print(f"Prepared {output}; original weights unchanged; evaluate accuracy before use")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--worker", choices=MODELS, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.worker:
        worker(args.source, args.output, args.worker)
    else:
        prepare(args.source, args.output)


if __name__ == "__main__":
    main()
