# 默认字体许可 review 记录

## 状态与范围

**字体范围 review 完成：仅限锁定的 `epaint_default_fonts 0.36.2` 所带四个默认字体。** 已核对来源、四份完整许可原文、上游版权声明、实际 TTF `name` 表和 SHA-256；不表示整个 exe、其他依赖或整个发布包已合规，也不表示正式 build 或最终打包验收完成。

本目录仅含以下六份文档，不包含／复制任何 TTF：

| 文件 | 用途 |
|---|---|
| [ATTRIBUTION.md](ATTRIBUTION.md) | 固定来源、逐字体归属、实际名称、版权原文及字体／许可哈希 |
| [Hack-Regular.txt](Hack-Regular.txt) | 上游完整原文：MIT＋Bitstream Vera＋DejaVu public-domain 声明 |
| [OFL.txt](OFL.txt) | Noto Emoji 的完整 SIL OFL 1.1 |
| [UFL.txt](UFL.txt) | Ubuntu Light 的完整 Ubuntu Font Licence 1.0 |
| [emoji-icon-font-mit-license.txt](emoji-icon-font-mit-license.txt) | 完整 MIT，含 John Slegers 2014 版权声明 |
| `README.md` | 本次范围、证据与复核方法 |

四份 `.txt` 从本地锁定 crate 按原始字节复制，未翻译、删节、改写或规范化换行。`OFL.txt`、`UFL.txt` 本身不含对应字体版权人，故在 `ATTRIBUTION.md` 中另行保留实际 TTF ID 0 的 Google 2013、Canonical 2011 原文。Hack 保留两个许可，不能简化为仅 MIT。emoji 的 TTF 没有 ID 0／13／14，John Slegers 归属来自上游随附许可，不声称来自字体元数据。

## 本次使用的本地证据

从现有 `.dbg/license-metadata-current.json` 的 `packages[]` 精确选择 `name = epaint_default_fonts`、`version = 0.36.2`，使用其 `manifest_path`，没有猜测 Cargo 缓存位置，也没有重新运行 Cargo 或更改锁文件。

```text
Manifest:
D:\rust\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\epaint_default_fonts-0.36.2\Cargo.toml

Cached archive:
D:\rust\.cargo\registry\cache\index.crates.io-1949cf8c6b5b557f\epaint_default_fonts-0.36.2.crate

Metadata package ID:
registry+https://github.com/rust-lang/crates.io-index#epaint_default_fonts@0.36.2

Metadata and manifest license:
(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0

Metadata license_file: null

.cargo_vcs_info.json:
git.sha1 = 49682f8baa058bf49e011035cfbd6e825f88a5ef
path_in_vcs = crates/epaint_default_fonts
```

metadata 的 `resolve.nodes` 包含该精确 package ID；这只证明该 metadata 快照解析到了此包，不是最终 exe 的二进制清单。包内 `src/lib.rs` 的四个 `include_bytes!` 对应 `Hack-Regular.ttf`、`NotoEmoji-Regular.ttf`、`Ubuntu-Light.ttf`、`emoji-icon-font.ttf`。

| 证据文件（crate 内路径除另注外） | 本次读取时 SHA-256 |
|---|---|
| 项目 `.dbg/license-metadata-current.json` | `823e14ec85900e502e2f18c24c570453ed5346afbcf5bb039fa91d5611cfbae6` |
| 项目 `Cargo.lock` | `b148dff3befb112d32993fa08fadc4514df6ce0a45467ef1a3f1c0b4a5d06d1f` |
| `Cargo.toml` | `1f8ca15b386d6b71272af2e324b40339ed0ada3ff38dd25d6dc571b76c7f5e67` |
| `.cargo_vcs_info.json` | `f3fa80ca2d37b6b8790e959c85a012a40d3c64581de86a98bc6817a4569fa02c` |
| `src/lib.rs` | `da81ac92c840666bab7f8fcc223c8e474b642db95b14ee5e5b7e01a1fa9c43b6` |
| 缓存 `epaint_default_fonts-0.36.2.crate` | `773fa9c96dd0dbef887e39d0ed6177f141cce4d68e6041df77570aa3702dfa13` |

缓存 archive 哈希与 `Cargo.lock` 中该包 checksum 一致；四个 TTF 和四份许可文本均已与 archive 成员逐字节比较。固定 egui commit 来自该包的 VCS 信息；本次没有联网重新获取 GitHub 内容。逐文件固定 commit 链接及八个文件哈希见 `ATTRIBUTION.md`。本地绝对路径仅为复核记录，发行时无需携带 Cargo 缓存。

## 复核方法

在项目根目录使用 Python 3.11+ 标准库执行下面代码。它只读现有 metadata、锁文件、缓存及本目录；不运行 Cargo、不解压到磁盘、不写文件、不启动 GUI。它检查包身份、许可表达式、VCS pin、archive checksum、八个源文件、四份许可副本与归属文档中的哈希，并打印实际 `name` 记录以复核版权和命名。

```python
import hashlib
import json
from pathlib import Path
import struct
import tarfile
import tomllib

metadata = json.loads(Path('.dbg/license-metadata-current.json').read_text(encoding='utf-8-sig'))
packages = [p for p in metadata['packages'] if p['name'] == 'epaint_default_fonts']
assert len(packages) == 1
package = packages[0]
assert package['version'] == '0.36.2'
assert package['license_file'] is None
assert any(n['id'] == package['id'] for n in metadata['resolve']['nodes'])
root = Path(package['manifest_path']).parent
manifest = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['package']
assert manifest['name'] == package['name'] and manifest['version'] == package['version']
assert manifest['license'] == package['license'] == '(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0'
vcs = json.loads((root / '.cargo_vcs_info.json').read_text(encoding='utf-8'))
assert vcs['git']['sha1'] == '49682f8baa058bf49e011035cfbd6e825f88a5ef'
assert vcs['path_in_vcs'] == 'crates/epaint_default_fonts'
locked = [p for p in tomllib.loads(Path('Cargo.lock').read_text(encoding='utf-8'))['package']
          if p['name'] == package['name']]
assert len(locked) == 1
assert locked[0]['version'] == package['version'] and locked[0]['source'] == package['source']
archive = root.parent.parent.parent / 'cache' / root.parent.name / (root.name + '.crate')
assert hashlib.sha256(archive.read_bytes()).hexdigest() == locked[0]['checksum']
assert locked[0]['checksum'] == '773fa9c96dd0dbef887e39d0ed6177f141cce4d68e6041df77570aa3702dfa13'
out = Path('distribution/legal/app/fonts')
attribution = (out / 'ATTRIBUTION.md').read_text(encoding='utf-8')
fonts = ('Hack-Regular.ttf', 'NotoEmoji-Regular.ttf', 'Ubuntu-Light.ttf', 'emoji-icon-font.ttf')
texts = ('Hack-Regular.txt', 'OFL.txt', 'UFL.txt', 'emoji-icon-font-mit-license.txt')
assert {p.name for p in (root / 'fonts').iterdir()} == set(fonts + texts)
assert {p.name for p in out.iterdir()} == set(texts + ('ATTRIBUTION.md', 'README.md'))
with tarfile.open(archive) as tar:
    for name in fonts + texts:
        data = (root / 'fonts' / name).read_bytes()
        assert tar.extractfile(root.name + '/fonts/' + name).read() == data
        assert hashlib.sha256(data).hexdigest() in attribution
        if name in texts:
            assert (out / name).read_bytes() == data
        print('Verified:', name, len(data), hashlib.sha256(data).hexdigest())

for name in fonts:
    data = (root / 'fonts' / name).read_bytes()
    table_count = struct.unpack_from('>H', data, 4)[0]
    tables = [struct.unpack_from('>4sIII', data, 12 + 16 * i) for i in range(table_count)]
    _, _, offset, length = next(t for t in tables if t[0] == b'name')
    assert offset + length <= len(data)
    table = data[offset:offset + length]
    fmt, count, strings = struct.unpack_from('>HHH', table)
    assert fmt == 0 and 6 + 12 * count <= strings <= len(table)
    for i in range(count):
        platform, encoding, language, name_id, size, relative = struct.unpack_from('>HHHHHH', table, 6 + 12 * i)
        assert strings + relative + size <= len(table)
        raw = table[strings + relative:strings + relative + size]
        if platform == 0 or (platform == 3 and encoding in (0, 1, 10)):
            text = raw.decode('utf-16-be')
        elif platform == 1 and encoding == 0:
            text = raw.decode('mac_roman')
        else:
            raise ValueError(('Unsupported name encoding', platform, encoding))
        if name_id in (0, 1, 2, 4, 5, 6, 7, 8, 9, 13, 14):
            print(name, platform, encoding, language, name_id, repr(text))
print('PASS: four-font source, metadata, license-copy and hash checks only')
```

## 发布交接与限制

- 随含这些字体的应用分发时，应一并携带本目录的 `ATTRIBUTION.md` 和四份完整许可文本；仅有 SPDX 名称、外链或本 README 不代替许可与版权通知。
- 正式构建／打包负责人仍需确认最终使用的字体字节及版本与本记录一致，并确认通知实际进入最终发行包。变更字体、做子集化、改名或升级 crate 后应重新核对许可条件与哈希；本记录不批准这些变更。
- 本次未构建或检查最终 exe，未审查其他 Rust crate、ONNX Runtime、模型、系统／用户提供字体及其他资源。不以字体 review 推导 whole-exe compliance。
- 本次仅新建本目录内文件，未修改已有文件、根级 review／REVIEWED 记录、Cargo 文件或 Git 状态；未提交、启动 GUI、采集桌面或开启麦克风。
