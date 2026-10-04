# Default font attribution — epaint_default_fonts 0.36.2

This notice covers only the four font assets embedded by `epaint_default_fonts 0.36.2`. The font binaries are not copied into this directory. Copyright and attribution statements below come from the upstream license texts or the actual TTF `name` tables, not from inferred ownership or the crate author's identity.

## Fixed source and package evidence

- Published package: <https://crates.io/crates/epaint_default_fonts/0.36.2>.
- Registry source: `registry+https://github.com/rust-lang/crates.io-index`.
- egui commit: `49682f8baa058bf49e011035cfbd6e825f88a5ef`.
- Fixed source directory: <https://github.com/emilk/egui/tree/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/epaint_default_fonts/fonts>.
- The published package's `.cargo_vcs_info.json` records that commit and `path_in_vcs = "crates/epaint_default_fonts"`. Version `0.36.2` is confirmed by Cargo metadata, the local manifest, and `Cargo.lock`.
- The local cached `.crate` archive has SHA-256 `773fa9c96dd0dbef887e39d0ed6177f141cce4d68e6041df77570aa3702dfa13`, matching its `Cargo.lock` checksum. All four local TTF files and all four license texts were compared byte-for-byte with that archive.
- Metadata and the normalized package manifest both declare `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0`; `license_file` in metadata is `null`. This aggregate crate expression is recorded as evidence, not as a replacement for the individual font licenses. In particular, Hack's accompanying Bitstream Vera terms must also be retained; the fonts are not all offered under an MIT/Apache choice.
- The package's [src/lib.rs](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/epaint_default_fonts/src/lib.rs) embeds exactly the four TTF assets listed below via `include_bytes!`.

All SHA-256 values in this document are over complete, unmodified file bytes. The fixed GitHub URLs identify the source revision recorded in the published package; this review used the local locked package and did not independently fetch GitHub files.

## Hack Regular

- Asset: [fonts/Hack-Regular.ttf](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/epaint_default_fonts/fonts/Hack-Regular.ttf), 309408 bytes.
- TTF SHA-256: `15f55cc0c85a2988d2b4b3a8cdb5d77fdfbaf319e1bb5309d725db9818fb7125`.
- Full license: [Hack-Regular.txt](Hack-Regular.txt), copied unchanged from [upstream](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/epaint_default_fonts/fonts/Hack-Regular.txt). This includes **both MIT and Bitstream Vera**, plus the DejaVu public-domain statement.
- License SHA-256: `47c0cccbeec7e8614548cc485588b28149e7874188df5f41b36efebcee285c87`.

Upstream license attribution, verbatim:

```text
The work in the Hack project is Copyright 2018 Source Foundry Authors and licensed under the MIT License

The work in the DejaVu project was committed to the public domain.

Bitstream Vera Sans Mono Copyright 2003 Bitstream Inc. and licensed under the Bitstream Vera License with Reserved Font Names "Bitstream" and "Vera"
```

TTF `name` ID 0, verbatim:

```text
Copyright (c) 2018 Source Foundry Authors / Copyright (c) 2003 by Bitstream, Inc. All Rights Reserved.
```

The accompanying license also states, verbatim:

```text
Copyright (c) 2003 by Bitstream, Inc. All Rights Reserved. Bitstream Vera is a trademark of Bitstream, Inc.
```

TTF ID 13 contains the MIT and Bitstream Vera license text, including the same attribution and reserved names; its whitespace differs from the companion text. ID 14 is `https://github.com/source-foundry/Hack/blob/master/LICENSE.md` (an embedded historical URL, not this review's revision pin).

## Noto Emoji

- Asset: [fonts/NotoEmoji-Regular.ttf](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/epaint_default_fonts/fonts/NotoEmoji-Regular.ttf), 418804 bytes.
- TTF SHA-256: `415dc6290378574135b64c808dc640c1df7531973290c4970c51fdeb849cb0c5`.
- License: **SIL Open Font License 1.1**; full text [OFL.txt](OFL.txt), copied unchanged from [upstream](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/epaint_default_fonts/fonts/OFL.txt).
- License SHA-256: `6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2`.

The companion `OFL.txt` does not supply the font's copyright holder. The following notice is taken directly from this TTF's `name` ID 0 and must accompany the license:

```text
Copyright 2013 Google Inc. All Rights Reserved.
```

TTF ID 7, verbatim:

```text
Noto is a trademark of Google Inc.
```

TTF ID 13, verbatim:

```text
This Font Software is licensed under the SIL Open Font License, Version 1.1. This Font Software is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the SIL Open Font License for the specific language, permissions and limitations governing your use of this Font Software.
```

TTF ID 14: `http://scripts.sil.org/OFL`. ID 8 identifies the manufacturer as `Monotype Imaging Inc.` and ID 9 the designer as `Monotype Design Team`; neither field is substituted for the copyright notice.

## Ubuntu Light

- Asset: [fonts/Ubuntu-Light.ttf](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/epaint_default_fonts/fonts/Ubuntu-Light.ttf), 361676 bytes.
- TTF SHA-256: `80307b8da7649aa4ee4d484b232140e3ce1ec0ca093073d3c53c8f5a5ced7a70`.
- License: **Ubuntu Font Licence 1.0**; full text [UFL.txt](UFL.txt), copied unchanged from [upstream](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/epaint_default_fonts/fonts/UFL.txt).
- License SHA-256: `2f0015108d68627bd788d313f529c21ff4da2c2c42a5e1f3883acc83480f9002`.

The companion `UFL.txt` does not supply the font's copyright holder. TTF `name` ID 0 supplies both copyright and license identification, verbatim (including the two spaces after `Ltd.`):

```text
Copyright 2011 Canonical Ltd.  Licensed under the Ubuntu Font Licence 1.0
```

TTF ID 7, verbatim:

```text
Ubuntu and Canonical are registered trademarks of Canonical Ltd.
```

TTF IDs 8 and 9 both contain `Dalton Maag Ltd` (manufacturer and designer, respectively), not a replacement copyright holder. This TTF has no ID 13 or 14 record; the license identification is in ID 0 and the complete terms are in `UFL.txt`.

## emoji-icon-font

- Asset: [fonts/emoji-icon-font.ttf](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/epaint_default_fonts/fonts/emoji-icon-font.ttf), 324132 bytes.
- TTF SHA-256: `f426bec371f484646d002717e2c555a95cd5145e635d83a48bbef83f3abf18ca`.
- License: **MIT**; full text [emoji-icon-font-mit-license.txt](emoji-icon-font-mit-license.txt), copied unchanged from [upstream](https://github.com/emilk/egui/blob/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/epaint_default_fonts/fonts/emoji-icon-font-mit-license.txt).
- License SHA-256: `b9d2c1d909aa149996fd4c91dcb92b2362a04431640c1d200959da94caf8cde1`.

Copyright notice from that companion license, verbatim:

```text
Copyright (c) 2014 John Slegers
```

The TTF identifies itself as `emoji`. It has no `name` ID 0, 13 or 14 record; copyright and MIT attribution are therefore sourced from the companion license, not invented from the font name. The crate's `src/lib.rs` links to `https://github.com/jslegers/emoji-icon-font` as the font project.

## Actual TTF naming records

Parsed with Python standard-library `struct` from each TTF's sfnt table directory and format-0 `name` table. The following records use platform 3, encoding 1, language 1033 (`0x0409`), decoded as UTF-16BE. The emoji font also contains platform 1, encoding 0, language 0 records, decoded as Mac Roman, with identical values for these IDs.

| Asset | ID 1: family | ID 2: subfamily | ID 4: full name | ID 6: PostScript name |
|---|---|---|---|---|
| Hack-Regular.ttf | Hack | Regular | Hack Regular | Hack-Regular |
| NotoEmoji-Regular.ttf | Noto Emoji | Regular | Noto Emoji | NotoEmoji |
| Ubuntu-Light.ttf | Ubuntu Light | Regular | Ubuntu Light | Ubuntu-Light |
| emoji-icon-font.ttf | emoji | Regular | emoji | emoji |

ID 5 version strings, verbatim:

```text
Hack-Regular.ttf: Version 3.003;[3114f1256]-release; ttfautohint (v1.7) -l 6 -r 50 -G 200 -x 10 -H 181 -D latn -f latn -m "Hack-Regular-TA.txt" -w G -W -t -X ""
NotoEmoji-Regular.ttf: Version 1.05 uh
Ubuntu-Light.ttf: 0.83
emoji-icon-font.ttf: Version 1.1
```

## Review boundary

The source, license-text collection, copyright attribution, TTF naming records and hashes for these four default fonts have been reviewed. Retain this attribution and all four complete license files with distributions containing these fonts, including embedded copies. This is **not a whole-executable or whole-release compliance declaration**, does not relicense any font, and does not verify a final build or release archive. Other dependencies, system/user fonts, models, runtime components and final packaging remain outside this review. See [README.md](README.md) for the local evidence and verification procedure.
