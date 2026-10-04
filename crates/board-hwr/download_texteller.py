"""Install the pinned TexTeller ONNX files (no remote code or credentials)."""
import argparse
import hashlib
import json
import pathlib
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor

REVISION = "7b96df06b9d81cdb129c3bef68b7250bc3e2b0ea"
FILES = {
    "encoder_model.onnx": (343553824, "sha256", "071afc0a0c92b2ee3847612127eb9e82b1c39413725e692f4405e5ea9e265bf0"),
    "decoder_model.onnx": (908716081, "sha256", "288cb8e37cdda66f725f2739efd2e7846636666a815e27533891c68389ec3659"),
    "config.json": (4504, "git", "45365ba45b979b9aff53dafd81571e8dc162f437"),
    "generation_config.json": (154, "git", "c9716daf378288b85faff33bb35b560f5eaa064c"),
    "tokenizer.json": (1044328, "git", "a507d83d4947b079dba2567d2767af97be5b0bcb"),
}


def verified(path, size, algorithm, expected):
    if not path.is_file() or path.stat().st_size != size:
        return False
    h = hashlib.sha1() if algorithm == "git" else hashlib.new(algorithm)
    if algorithm == "git":
        h.update(f"blob {size}\0".encode())
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            h.update(chunk)
    return h.hexdigest() == expected


def download(path, urls, size, algorithm, expected, opener):
    if verified(path, size, algorithm, expected):
        print(f"Verified {path.name}", flush=True)
        return
    part = path.with_suffix(path.suffix + ".part")
    for attempt in range(8):
        if part.exists() and part.stat().st_size >= size:
            if verified(part, size, algorithm, expected):
                part.replace(path)
                return
            part.unlink()
        offset = part.stat().st_size if part.exists() else 0
        url = urls[min(attempt, len(urls) - 1)]
        headers = {"User-Agent": "NeoRuntime-TexTeller/1", "Accept-Encoding": "identity"}
        if offset:
            headers["Range"] = f"bytes={offset}-"
        try:
            print(f"Download {path.name} from byte {offset}, attempt {attempt + 1}", flush=True)
            with opener.open(urllib.request.Request(url, headers=headers), timeout=60) as response:
                if offset and response.status == 206:
                    if not response.headers.get("Content-Range", "").startswith(f"bytes {offset}-"):
                        raise ValueError("Invalid Content-Range")
                elif response.status == 200:
                    offset = 0
                else:
                    raise ValueError(f"Unexpected HTTP {response.status}")
                mode = "ab" if offset else "wb"
                next_report = offset + 64 * 1024 * 1024
                with part.open(mode) as output:
                    while chunk := response.read(1024 * 1024):
                        offset += len(chunk)
                        if offset > size:
                            raise ValueError("Download exceeds pinned size")
                        output.write(chunk)
                        if offset >= next_report:
                            print(f"{path.name}: {offset}/{size}", flush=True)
                            next_report = offset + 64 * 1024 * 1024
            if not verified(part, size, algorithm, expected):
                raise ValueError("Size/hash mismatch")
            part.replace(path)
            print(f"Verified {path.name}", flush=True)
            return
        except (OSError, ValueError, urllib.error.URLError) as error:
            # Do not print signed redirect URLs, proxy credentials or request headers.
            print(f"{path.name}: {type(error).__name__}; retrying", flush=True)
            time.sleep(min(attempt + 1, 5))
    raise RuntimeError(f"Could not install {path.name}; partial download retained")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dir", type=pathlib.Path, default=pathlib.Path(__file__).resolve().parents[2] / "models" / "texteller")
    parser.add_argument("--mirror", action="store_true", help="Use hf-mirror.com after direct access has failed")
    args = parser.parse_args()
    args.dir.mkdir(parents=True, exist_ok=True)
    # Never forward ambient proxy/authentication configuration to public model hosts.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    hosts = ["https://huggingface.co", "https://hf-mirror.com"]
    if args.mirror:
        hosts = hosts[1:]
    def install(item):
        name, (size, algorithm, digest) = item
        urls = [f"{host}/OleehyO/TexTeller/resolve/{REVISION}/{name}?download=true" for host in hosts]
        download(args.dir / name, urls, size, algorithm, digest, opener)
    with ThreadPoolExecutor(max_workers=2) as pool:
        list(pool.map(install, FILES.items()))
    print(f"Installed verified model in {args.dir.resolve()}", flush=True)


if __name__ == "__main__":
    main()
