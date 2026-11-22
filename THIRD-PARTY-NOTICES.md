# Third-party notices

## IBM Plex fonts

The four font files below are bundled unmodified, exactly as published in the upstream release archives, under the SIL Open Font License, Version 1.1 (OFL-1.1). The license text is at `LICENSES/OFL-1.1.txt`.

- Copyright: Copyright © 2017 IBM Corp. with Reserved Font Name "Plex"
- License: OFL-1.1
- Reserved Font Name: "Plex"

### IBM Plex Sans

- Release tag: `@ibm/plex-sans@1.3.6`
- Source: <https://github.com/IBM/plex/releases/download/%40ibm/plex-sans%401.3.6/ibm-plex-sans.zip>

| File | SHA-256 |
| --- | --- |
| `crates/fuzzy-dedupe-app/assets/fonts/IBMPlexSans-Regular.ttf` | `b47ebd72b5ce53b60f909033ff84d6fc12358cc147e2981f8c321ad3f36246b6` |
| `crates/fuzzy-dedupe-app/assets/fonts/IBMPlexSans-Medium.ttf` | `a494d5a69fa094f7071a9ffbc98e86c85337d1a29f3fc1ff7c2e1c46161ddf27` |
| `crates/fuzzy-dedupe-app/assets/fonts/IBMPlexSans-SemiBold.ttf` | `c51119b25f4a296da2b66edf3201b9aa1735091ed7efc1fca0313346f3851a4d` |

### IBM Plex Mono

- Release tag: `@ibm/plex-mono@2.5.0`
- Source: <https://github.com/IBM/plex/releases/download/%40ibm/plex-mono%402.5.0/ibm-plex-mono.zip>

| File | SHA-256 |
| --- | --- |
| `crates/fuzzy-dedupe-app/assets/fonts/IBMPlexMono-Regular.ttf` | `7c6fbddca4b700be918f5f6183d9bd4464fa427fe435f0b480d77fe2bb8c5a43` |

## Noto Emoji

The monochrome Noto Emoji font is bundled unmodified, as the emoji fallback after IBM Plex in every font family, under the SIL Open Font License, Version 1.1 (OFL-1.1). The license text is at `LICENSES/OFL-1.1.txt`. Only the file name differs from upstream, where it is `NotoEmoji[wght].ttf`; the bytes are identical.

- Copyright: Copyright 2013 Google LLC
- License: OFL-1.1
- Release: Noto Emoji version 3.002 (name table: `3.002;GOOG;NotoEmoji-Regular`), as Google Fonts publishes it in google/fonts commit `b979dba422e445492b0eb9951ac52ee0b4d648c3` ("Update Noto Emoji to the 3.002 release")
- Source: <https://raw.githubusercontent.com/google/fonts/b979dba422e445492b0eb9951ac52ee0b4d648c3/ofl/notoemoji/NotoEmoji%5Bwght%5D.ttf> (Git blob `c2c26ab612a88a8610ff9cfbb89299bf2aea6c7a`)
- The upstream release archive Google Fonts records, `googlefonts/emoji-bw` release `v3.000`, is no longer downloadable, and the `googlefonts/noto-emoji` releases ship only the color fonts, so the pinned Google Fonts commit is the source of record.

| File | SHA-256 |
| --- | --- |
| `crates/fuzzy-dedupe-app/assets/fonts/NotoEmoji-Regular.ttf` | `de6c18832938afc99caf132b39d6a30a19bac7f2e812e28db2535b4608d27551` |

## Lucide icons

The SVG icons below are bundled unmodified from the Lucide `1.77.0` release, under the ISC License. The license text is at `LICENSES/ISC.txt`.

- Copyright: Copyright (c) 2026 Lucide Icons and Contributors
- License: ISC
- Release tag: `1.77.0`
- Source: <https://github.com/lucide-icons/lucide/releases/download/1.77.0/lucide-icons-1.77.0.zip> (SHA-256 `f0ebddc04170f3880f82e91879f4f66d96574e88151d27c930983b802c8235f9`)

Lucide's license states that some of its icons are derived from the Feather project and covers those under Feather's MIT License as well. The icons marked "ISC AND MIT" below are among them, listed in Lucide's license under their earlier names where Lucide has since renamed them (`columns`, `more-horizontal`, `alert-triangle`). Feather's notice, as Lucide's license reproduces it:

```text
The MIT License (MIT) (for the icons listed above)

Copyright (c) 2013-present Cole Bemis

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

| File | SHA-256 | License |
| --- | --- | --- |
| `crates/fuzzy-dedupe-app/assets/icons/anchor.svg` | `dcb3f2de41e067983f31b35d896b41ae0e5c1782b527289d17d6efeec1ae8239` | ISC |
| `crates/fuzzy-dedupe-app/assets/icons/check.svg` | `7f33acc9a77a61659531044525fc008edebe215bf4dcf1c789c8674ad3277db0` | ISC AND MIT |
| `crates/fuzzy-dedupe-app/assets/icons/chevron-down.svg` | `66ea878e72ed3488bb3b464c39dfdccee8d1f78e560dccea40e5e12da0e87e87` | ISC AND MIT |
| `crates/fuzzy-dedupe-app/assets/icons/chevron-right.svg` | `2758143d7b2434e4aa7307dfd34405c87909ff4052f21b5f3f40d45224b4f19b` | ISC AND MIT |
| `crates/fuzzy-dedupe-app/assets/icons/columns-2.svg` | `bddf3348a9c82d111a060d118299c90755b1ae8411bd16ea1d5ddb83c5f98a27` | ISC AND MIT |
| `crates/fuzzy-dedupe-app/assets/icons/ellipsis.svg` | `4f495cc72013ffdfec677f03b33a150f7b4dd741979283fd6853a09024bca112` | ISC AND MIT |
| `crates/fuzzy-dedupe-app/assets/icons/file-text.svg` | `52ca55846c202335fb870c8aa23e9345630ec10489303756d62e750f7109605c` | ISC |
| `crates/fuzzy-dedupe-app/assets/icons/lock.svg` | `d2cfe60cac8deaef36442b5c8218c1e61abe18222784b2a370ea090d13fca840` | ISC AND MIT |
| `crates/fuzzy-dedupe-app/assets/icons/menu.svg` | `af6491988ac1ba30dc5414f485bc48713e957e1edb0fbef4a0dd8691c3c21598` | ISC |
| `crates/fuzzy-dedupe-app/assets/icons/search.svg` | `283d371c2e433817bb9c0c8310caa6c77fa4177c0f4f1168d9c83b97af7389dc` | ISC AND MIT |
| `crates/fuzzy-dedupe-app/assets/icons/settings.svg` | `0ae27fd0f81999229e3127ac96c5b32edfea448e291d509e76212b917551d66b` | ISC |
| `crates/fuzzy-dedupe-app/assets/icons/table-2.svg` | `7b1808e170670d644af828bed77022fc8baba27a654e0805f3fb98e0bec62077` | ISC AND MIT |
| `crates/fuzzy-dedupe-app/assets/icons/triangle-exclamation-point.svg` | `4866f38b8560d410f21e3226413e0b77997b6dfbb6931fadfe0a0d5aef9ffeb4` | ISC AND MIT |
