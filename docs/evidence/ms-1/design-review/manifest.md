<!--
SPDX-FileCopyrightText: 2026 L3Digital LLC
SPDX-License-Identifier: GPL-3.0-only
-->

# MS-1 design review captures (plan T24)

The captures EV-002 compares with the reference frames in `docs/design/reference/`. Each of the 16 non-dark review states of `crates/fuzzy-dedupe-app/tests/snapshots.rs` is captured in the light and dark themes at 100% zoom, and in the light theme at 200% in-app zoom: 48 PNGs, each with a `.geometry.json` accessibility dump beside it. This file lists them and how they were made; the review itself is EV-002.

## Binding

| Field | Value |
| --- | --- |
| `application_commit` | `23b3562c78d691c804574847c17994f6ddd02767` |
| `crates_tree` | `5b0a0a5d418ebf186b55544e844624a1bf75f95c` |
| `cargo_lock` | `5b86f8bade9f841adf89679cac394a98a3245d08` |
| `cargo_toml` | `a94ccf938ee056051158410c860a9cdbea464214` |
| `rust_toolchain` | `e50dfd8fe46b8b05bd33c47b9523b2768270fef7` |
| Build profile | `dev` (unoptimized with debug info) |
| Provenance | Built and run locally on the reference workstation from a checkout whose tree equals `application_commit`'s, under the plan §3 reference-workstation exception; `rustc 1.96.0 (ac68faa20 2026-05-25)`, `cargo 1.96.0 (30a34c682 2026-05-25)` |

The four object IDs are `git rev-parse "$C":crates`, `"$C":Cargo.lock`, `"$C":Cargo.toml` and `"$C":rust-toolchain.toml` with `C` set to `application_commit`.

## Run conditions

- Round 5: on T43's corrected main.
- Application commit: `23b3562c78d691c804574847c17994f6ddd02767`, the T43 squash on `main` (PR #64). The captures were rendered from its checkpoint `84f10e3c149f19ed41abb5190c8874531c631531`, whose tree is identical.
- Date: 2026-11-24.
- Machine: the reference workstation: Intel Core Ultra 7 155H with integrated Intel Arc graphics, Fedora Linux 45.
- Adapter: `Intel(R) Arc(tm) Graphics (MTL)`, integrated GPU, Vulkan backend, driver `Intel open-source Mesa driver`, Mesa 26.2.7 (`mesa-vulkan-drivers-26.2.7-1.fc45`), Xe kernel driver. The capture program refuses CPU adapters, so Mesa lavapipe, which is also installed, was not used.
- Desktop: KDE Plasma 6.8.3 (`plasma-workspace-6.8.3-1.fc45`, `kwin-6.8.3-2.fc45`), Wayland session (`XDG_SESSION_TYPE=wayland`, `XDG_CURRENT_DESKTOP=KDE`). The captures render off-screen, so no window was opened.
- fontconfig: 2.18.3 (`fontconfig-2.18.3-2.fc45`). The production font lookup returned these fallback families, in this order: Noto Sans CJK JP, Noto Sans Arabic, DejaVu Sans, Noto Sans Thai.
- Determinism: two full runs of the program produced byte-identical PNGs and geometry files.

## How the captures were made

Kind: an `egui_kittest` wgpu render of the whole app (`FuzzyDedupeApp::new`) on the `design` scenario, not a screenshot of the launched binary. The rig and the 16 state preparations are copied from `tests/snapshots.rs` at T41, which T42 and T43 left unchanged: the app draws into a child spanning the whole content rect, not inside kittest's 8-unit margin; New scan opens on a seeded setup of three crate-relative folders (`src/` protected, `tests/`, `assets/`) and the design fixture's file types; Scanning clears the queue through `ClearForNewScan`, drops the results, and asserts that the progress segment is painted on the track at least a quarter of its width; Findings and the main-menu frame expand the frame's groups and focus `Design_Spec_draft.txt`. Each state is driven and asserted through the accessibility tree, then the pointer is removed and egui's input time is pinned to 1000.8 s before rendering. 1000.8 s is half a 1.6 s sweep past a whole number of sweeps, so the Scanning frame's progress segment sits mid-track; the Scanning preparation already runs at that time, so the segment it checks is the one captured. It differs from the CI snapshots in three ways:

1. Fonts come from the production `SystemFontLookup`, not the embedded-only lookup. kittest's default missing-glyph policy stayed on, and it panics on any character no installed font can draw. All 48 renders completed, so none drew a missing-glyph box.
2. Rendering uses this machine's hardware Vulkan adapter, not the CPU rasterizer kittest prefers by default.
3. The 200% captures set the zoom preference to 200 after launch, and `app.rs` applies it through `theme::apply_zoom`. On the zoom change egui rescales the window from 1600 × 960 to 800 × 480 points (512 × 300 for the 1024 × 600 frames) and renders at 2 pixels per point. That matches a real window of the frame's pixel size at 200% zoom, so each zoom PNG has the frame's pixel size and the UI has half the room in points. The program asserts both the point size and the pixel size.

Window sizes are 1600 × 960, except `29:2214` and `29:3648` at 1024 × 600. The reference PNGs of popup frames include a shadow margin, while every capture here is the whole window.

## Geometry files

Beside each PNG, `<capture>.geometry.json` lists every accessibility node that has a label or one of the roles Button, CheckBox, TextInput, Row, Cell, ColumnHeader, Group, Label and Tooltip, plus Dialog containers, in tree order. The program also lists Menu containers, but egui's main menu exposes no Menu node, so no file has one: in `44:3954` the menu's items sit in the window's tree with `container` `null`, and the menu has no `popup_rect_px`. Each node has its `role`, `label`, `value`, `disabled` and `toggled` state, `rect_px` as `[x0, y0, x1, y1]` in PNG pixels (egui's accesskit bounds, which are points times pixels per point), `parent` (the nearest listed ancestor) and `container` (the nearest Dialog or Tooltip ancestor, so a popup's nodes can be told from the window behind it). accesskit gives egui's modal Dialog node no bounds, so the file's top-level `popup_rect_px` carries the open popup's rect, read from the modal area's remembered rect; it is `null` when no popup is open. The capture program replaces the real home directory in node text with `~`, and the files hold no home path: the `~/…` folders in them are the design scenario's own display text. The files are formatted with Prettier.

## Captures

| File | Frame | Theme | Zoom | Pixels | SHA-256 | Geometry SHA-256 |
| --- | --- | --- | --- | --- | --- | --- |
| [`13-648-findings-light.png`](13-648-findings-light.png) | `13:648` | Light | 100% | 1600 × 960 | `a3e7e35ac10ce2d0d1a6e6511ec07bd18501d5eb98e036890019590055851dc7` | [`27de1a4e90e9cc5eaf5c991795b5503f432cd29b2eea16a57570351fb7598a0e`](13-648-findings-light.geometry.json) |
| [`13-648-findings-dark.png`](13-648-findings-dark.png) | `13:648` | Dark | 100% | 1600 × 960 | `16ee37852fa1c666e402e1e9d36b6769a371f2a24ea7bf29f31f0e830195e8c6` | [`5e560763a30b16de96472df1f03be0972b42f0396c2518960a212444eaa291e1`](13-648-findings-dark.geometry.json) |
| [`13-648-findings-zoom200.png`](13-648-findings-zoom200.png) | `13:648` | Light | 200% | 1600 × 960 | `6cacfc8118683e3a0da787bdd1c17fd5817b7058b19e76117d138e91b096ae2c` | [`f138bf93c5ee5cc5fd693c05dfd6234631cbb54c183d999c2099b62b4294e244`](13-648-findings-zoom200.geometry.json) |
| [`13-1304-compare-light.png`](13-1304-compare-light.png) | `13:1304` | Light | 100% | 1600 × 960 | `89d10f36062a2bd460e79155c2519d15a7a4423eb872f8331417104c5fe77fd7` | [`b1bbd1d137e2d67f937ef2ef02797ad087ddc799821ecf7daee33a07836acee1`](13-1304-compare-light.geometry.json) |
| [`13-1304-compare-dark.png`](13-1304-compare-dark.png) | `13:1304` | Dark | 100% | 1600 × 960 | `4cad9151bd2f64d82ff8ecbbad35891eb44faecebff794deb2c03c63f8a2b8a4` | [`0f980021d5ae58d08d11b1c7cf2b3901076ed6fff9900ec9a159989ec3b9e44d`](13-1304-compare-dark.geometry.json) |
| [`13-1304-compare-zoom200.png`](13-1304-compare-zoom200.png) | `13:1304` | Light | 200% | 1600 × 960 | `154c17a2a442ce905c7fa835cce515df67394ffcf50ac839bf3dacad992ece66` | [`adf2387597a1d204706b37cd87564fc1aa6ec0ab8d89f48120efad2e61d20340`](13-1304-compare-zoom200.geometry.json) |
| [`16-735-matching-rules-light.png`](16-735-matching-rules-light.png) | `16:735` | Light | 100% | 1600 × 960 | `b7afdea00df733644dc75fee7d88cbfa24ec06191dec73159ec47b3cb3b2f26f` | [`27456e3fca7b88419fbb822bd682bc3c95385054d7ef15f04842941d67f2027c`](16-735-matching-rules-light.geometry.json) |
| [`16-735-matching-rules-dark.png`](16-735-matching-rules-dark.png) | `16:735` | Dark | 100% | 1600 × 960 | `c2be81ecf4b5078f73b7975864bd4262c73eee7aaf140f31a939b6736f79cbc4` | [`507c25d52a7962649798ffa113858dcd6daabbb3b3094da7ea12f0a7ec1982df`](16-735-matching-rules-dark.geometry.json) |
| [`16-735-matching-rules-zoom200.png`](16-735-matching-rules-zoom200.png) | `16:735` | Light | 200% | 1600 × 960 | `bb74959eed9ccd6bed3ac22bd0478535bee1d1d6f29410f0d6faefb179d7ac3c` | [`bceecb14a7c10f4ed63b726729edeba54321da7395b73b3f3b1868fbbf300b0f`](16-735-matching-rules-zoom200.geometry.json) |
| [`16-806-new-scan-light.png`](16-806-new-scan-light.png) | `16:806` | Light | 100% | 1600 × 960 | `2faf31dd531d1e096aaae9bcf505a56d1d5e41aeeb4d4ad97bfbed418bda3483` | [`68b0144a6915f1eb101edde24c3f3319c81be6a23e9370542da4ae87a693511a`](16-806-new-scan-light.geometry.json) |
| [`16-806-new-scan-dark.png`](16-806-new-scan-dark.png) | `16:806` | Dark | 100% | 1600 × 960 | `3d4ebee64b418db509f695f3e6b607756317506efab36507c005ac77d7046d22` | [`92cae31862170f28e1fcd1e023a67495e3a274f0e3e3572557a44e48a317d84a`](16-806-new-scan-dark.geometry.json) |
| [`16-806-new-scan-zoom200.png`](16-806-new-scan-zoom200.png) | `16:806` | Light | 200% | 1600 × 960 | `e8118ff6d4744cbd1e3f4489da1a6042cae685f66158b84e25b32585558936bb` | [`1997a727920b5f019cb149695e9201106c686353f9688122d0296d05b23d894c`](16-806-new-scan-zoom200.geometry.json) |
| [`16-932-review-unique-text-light.png`](16-932-review-unique-text-light.png) | `16:932` | Light | 100% | 1600 × 960 | `26b53a5bc207ad2e531829f1db4897d6d5b65b0734fea69a9f085d0f4418b333` | [`b610f1ec7c9a9e671b3e4aa950e3e62413cd3a8e218c5ca7b8963169a3b7d18b`](16-932-review-unique-text-light.geometry.json) |
| [`16-932-review-unique-text-dark.png`](16-932-review-unique-text-dark.png) | `16:932` | Dark | 100% | 1600 × 960 | `410b9f03784a24fcce3ca7f422065d5330c019356b7e233819ff811a1586f95b` | [`29635eb876b1d28b22a56279b8046bd06c014fccb8c940d0f506fe7f1fa1081b`](16-932-review-unique-text-dark.geometry.json) |
| [`16-932-review-unique-text-zoom200.png`](16-932-review-unique-text-zoom200.png) | `16:932` | Light | 200% | 1600 × 960 | `c2a746ddc1707ecc61ed028a5807b552b6c34d7143dbde61fce1298fb1e3880e` | [`e20034d2d66d3d58afd78e70ecd7d06507f2f466f253f649bc22173d1226f3f0`](16-932-review-unique-text-zoom200.geometry.json) |
| [`16-886-review-cleanup-light.png`](16-886-review-cleanup-light.png) | `16:886` | Light | 100% | 1600 × 960 | `d9f1f2fb5476d227fe335ddb997dcaa82e1ee92cd76a757d5a7ab8208ef0e287` | [`8e70b93c96dc5780f64d27b2ce983c7c152815617ed1a4fb10293e0e11eecee3`](16-886-review-cleanup-light.geometry.json) |
| [`16-886-review-cleanup-dark.png`](16-886-review-cleanup-dark.png) | `16:886` | Dark | 100% | 1600 × 960 | `8151b596466a0c7a121b5295f1f4d6dce1ba3c863fb4ad8466efa2fc5655f097` | [`ba1bca1aff95ee5a064651498fc54c1207ad1c551905b562a400264622b576f4`](16-886-review-cleanup-dark.geometry.json) |
| [`16-886-review-cleanup-zoom200.png`](16-886-review-cleanup-zoom200.png) | `16:886` | Light | 200% | 1600 × 960 | `e502883e773f0d96dfa60814071a34c02224a1a0a2968e42e12cad1ba84aeef8` | [`f6a520b131c6d15e6761d7439d13508026fd2b2d882af18c92d99012f3a0a75b`](16-886-review-cleanup-zoom200.geometry.json) |
| [`28-1399-findings-before-first-scan-light.png`](28-1399-findings-before-first-scan-light.png) | `28:1399` | Light | 100% | 1600 × 960 | `841273169e06da0bbe6813d5dd96ad8e9d474a8df2d3467afe16da7c3c03d705` | [`a40ece054e9cd519de028db74da108d3335fc08f2ecab3c3727349fe5c826ee7`](28-1399-findings-before-first-scan-light.geometry.json) |
| [`28-1399-findings-before-first-scan-dark.png`](28-1399-findings-before-first-scan-dark.png) | `28:1399` | Dark | 100% | 1600 × 960 | `6d0b617907407740c047031b774c786080716c4ee854f8a4b853110fcb6d58ba` | [`f21858dfd3014700dddeacda13f939879daa639157076166d324dc83d243bb3b`](28-1399-findings-before-first-scan-dark.geometry.json) |
| [`28-1399-findings-before-first-scan-zoom200.png`](28-1399-findings-before-first-scan-zoom200.png) | `28:1399` | Light | 200% | 1600 × 960 | `2330368941fb5d55704333fb5df255438fb09c10710a4d954957d09a7b51cd4e` | [`a244c20c7c59f137c83d3d9eadd2e4037793d04b923910f5fd3af0e06e0f298e`](28-1399-findings-before-first-scan-zoom200.geometry.json) |
| [`28-2171-findings-scanning-light.png`](28-2171-findings-scanning-light.png) | `28:2171` | Light | 100% | 1600 × 960 | `dd45d7bb49c4e7ec59d8bf081c841d6b2c44ad711a5f1ec008e1fc631f07e8fe` | [`87e0d91a60150b14e5d39c901397e9f7e03dc0ec48864d42eb2512088b533a27`](28-2171-findings-scanning-light.geometry.json) |
| [`28-2171-findings-scanning-dark.png`](28-2171-findings-scanning-dark.png) | `28:2171` | Dark | 100% | 1600 × 960 | `60527d3d3ced28e8bbf20d0f5ac071e5f68850bef70f258bd4b58fe3da467921` | [`8b13c08025505c3bee80a83dcaf84a7dce96a4dbb9b46510178da9505c33db54`](28-2171-findings-scanning-dark.geometry.json) |
| [`28-2171-findings-scanning-zoom200.png`](28-2171-findings-scanning-zoom200.png) | `28:2171` | Light | 200% | 1600 × 960 | `dfa1dde0ab3a8d22e781e056fbce2a126439c8fb038617118b74ec0726293d74` | [`8f65bba6624dd4333f6e27b38f071a1b52de19456457152daf48855fb975c899`](28-2171-findings-scanning-zoom200.geometry.json) |
| [`30-2370-findings-no-search-matches-light.png`](30-2370-findings-no-search-matches-light.png) | `30:2370` | Light | 100% | 1600 × 960 | `ef7179d89d875042bbd77d69775859389da45e3979ed524627b6a3b2fa27f898` | [`373834c587eb136083ae787fcfbadd0d081d3d2d1f62a2e96dbb9c3909b19213`](30-2370-findings-no-search-matches-light.geometry.json) |
| [`30-2370-findings-no-search-matches-dark.png`](30-2370-findings-no-search-matches-dark.png) | `30:2370` | Dark | 100% | 1600 × 960 | `351b520740c85ffd3f74605c60a0445a9da6d7edd071f567af14bb668a9ae244` | [`59718ba560fff36bf32582950c44f8f4f3069d6927267bbff5884e9e89a2ee28`](30-2370-findings-no-search-matches-dark.geometry.json) |
| [`30-2370-findings-no-search-matches-zoom200.png`](30-2370-findings-no-search-matches-zoom200.png) | `30:2370` | Light | 200% | 1600 × 960 | `5dbb62a5ecd827afe40850a18a903ef7090717f9261cd4d665ee654504dc81ef` | [`904489bbc23212765fac5579798cc6cc8c838e91fc91307abb004cc902151c67`](30-2370-findings-no-search-matches-zoom200.geometry.json) |
| [`27-1390-cleanup-running-light.png`](27-1390-cleanup-running-light.png) | `27:1390` | Light | 100% | 1600 × 960 | `e43a1b764d749754360279f13aa43a88e545ab7757920824b49795a1db1a1114` | [`0297d1f175cccef3ec68416c991601dc6392bd88a5e7b98b128f45dffe10118d`](27-1390-cleanup-running-light.geometry.json) |
| [`27-1390-cleanup-running-dark.png`](27-1390-cleanup-running-dark.png) | `27:1390` | Dark | 100% | 1600 × 960 | `cdce17ae52bd2aa72f0ec51752e1e9799bc43c43757c2459f880d0523d4c51b9` | [`99c3aa6acfa6c99c36074f26f39cd7338be4d0ade54b0111f467745c35c2d305`](27-1390-cleanup-running-dark.geometry.json) |
| [`27-1390-cleanup-running-zoom200.png`](27-1390-cleanup-running-zoom200.png) | `27:1390` | Light | 200% | 1600 × 960 | `e74bba533ef06e077929d51c303f99c6e223e859bf94ccaebdc434d6401c7ce4` | [`4438250db59b9df44ecc140f677e44cc9126555eb025488c46bad4a962f3fd1b`](27-1390-cleanup-running-zoom200.geometry.json) |
| [`27-1533-cleanup-finished-light.png`](27-1533-cleanup-finished-light.png) | `27:1533` | Light | 100% | 1600 × 960 | `67fec8bbde80f0855ec112f8135b49255d15d95f6206d1903ce7b0ddd119108b` | [`e5e4e9205e7d90f6ee2d38227afca32a9b49fc52968f32260d4eb672b15c2632`](27-1533-cleanup-finished-light.geometry.json) |
| [`27-1533-cleanup-finished-dark.png`](27-1533-cleanup-finished-dark.png) | `27:1533` | Dark | 100% | 1600 × 960 | `aedda9cfc7b4915029cafdc7c589f4651722c4622cac505b10e6610465fd30d6` | [`d70ef3e5d8ab020b9efd7296ec33bbe265f71aa191653f6545d7d7b5948cb763`](27-1533-cleanup-finished-dark.geometry.json) |
| [`27-1533-cleanup-finished-zoom200.png`](27-1533-cleanup-finished-zoom200.png) | `27:1533` | Light | 200% | 1600 × 960 | `b81f23fb470222de0c2d79c40a6437515d97334cc3e0c81fc047361839adfad9` | [`0e98f1c27b44c747f85cd1f0003ff563af91f0aaa19deaa1ab9fdcfd139c4b82`](27-1533-cleanup-finished-zoom200.geometry.json) |
| [`29-2214-findings-1024x600-light.png`](29-2214-findings-1024x600-light.png) | `29:2214` | Light | 100% | 1024 × 600 | `db5a6cff55ed8cb10b0d688025419a29a49e1f8a343945c475eb60b06b2e829c` | [`274a09e57a4e54daeff92dc84232e231c5f2fd696f7158e143c4de809de56e3e`](29-2214-findings-1024x600-light.geometry.json) |
| [`29-2214-findings-1024x600-dark.png`](29-2214-findings-1024x600-dark.png) | `29:2214` | Dark | 100% | 1024 × 600 | `a39d66904397ec05c79ce70674f10dcc3355420c6dae72359d09805d82c7ab15` | [`ea0fa7deaef6f4db38b7030e42ad33f5cbf801615775fe82ed5faef3e3548590`](29-2214-findings-1024x600-dark.geometry.json) |
| [`29-2214-findings-1024x600-zoom200.png`](29-2214-findings-1024x600-zoom200.png) | `29:2214` | Light | 200% | 1024 × 600 | `887de170621db53efb1006579cb9945cb21c15a91148b4472d0e1939ff62a4d8` | [`a8a61899e4941fc40b43588a30cb17fb9cfaebb9a3cf0a9951382cec24333ff1`](29-2214-findings-1024x600-zoom200.geometry.json) |
| [`29-3648-compare-1024x600-light.png`](29-3648-compare-1024x600-light.png) | `29:3648` | Light | 100% | 1024 × 600 | `51640bbddaee26c7087c1433e660da7da818428d960a9e7aeeab0777192099de` | [`09bebf66877a71c9cb0c3738ef5be0ada1cbcd1a059db9271b30ac2ba72d780b`](29-3648-compare-1024x600-light.geometry.json) |
| [`29-3648-compare-1024x600-dark.png`](29-3648-compare-1024x600-dark.png) | `29:3648` | Dark | 100% | 1024 × 600 | `f1760014f0162e63600d37758b6a6fcc290b94796a497918abaf1de97b7a30c5` | [`ec4eb9be6c4399871d25c965401f3dd4bec5657c9698ee6f54eb0d6d8485b117`](29-3648-compare-1024x600-dark.geometry.json) |
| [`29-3648-compare-1024x600-zoom200.png`](29-3648-compare-1024x600-zoom200.png) | `29:3648` | Light | 200% | 1024 × 600 | `aacf8e114e3c01cb1e7709c35ad67805fcd916d37dd395f27161783995835215` | [`f013afb7e05e852243880761486d379f364d7a5df765933af4c8e2d61259aed1`](29-3648-compare-1024x600-zoom200.geometry.json) |
| [`44-3954-findings-main-menu-open-light.png`](44-3954-findings-main-menu-open-light.png) | `44:3954` | Light | 100% | 1600 × 960 | `4e610910c3c976f9f78964d9f7d32143a4f001fa898993af7a497ac087c0c97c` | [`8e26df76e918a7511d4db4909ec7ec0f4d9ee1aa2291455ec1c752a22a2ff063`](44-3954-findings-main-menu-open-light.geometry.json) |
| [`44-3954-findings-main-menu-open-dark.png`](44-3954-findings-main-menu-open-dark.png) | `44:3954` | Dark | 100% | 1600 × 960 | `58e04fc6edb3dda1aacfa5d7c62d7d66b29740e6e14578800318fc806849608f` | [`fb4b327855163143906d25e207cc03a1ea49ad7f2cd113eb16500b071361fa42`](44-3954-findings-main-menu-open-dark.geometry.json) |
| [`44-3954-findings-main-menu-open-zoom200.png`](44-3954-findings-main-menu-open-zoom200.png) | `44:3954` | Light | 200% | 1600 × 960 | `0905259cb7ec6bce4a201a606b815439a505046ae4a9388f238b14e7b9ff8aa9` | [`9242b007782658a087541cdadecbee3abe5d45e4d2cab590ad371d95e305d367`](44-3954-findings-main-menu-open-zoom200.geometry.json) |
| [`44-3681-keyboard-shortcuts-light.png`](44-3681-keyboard-shortcuts-light.png) | `44:3681` | Light | 100% | 1600 × 960 | `c917b16510e2ec841d9c57e39535faee56be5fc2bc27ab872665d3402db01d0b` | [`450d50627ef049f4295486b962784ef677536f80152761436e2902ee43486686`](44-3681-keyboard-shortcuts-light.geometry.json) |
| [`44-3681-keyboard-shortcuts-dark.png`](44-3681-keyboard-shortcuts-dark.png) | `44:3681` | Dark | 100% | 1600 × 960 | `7e7b6e84ca62df5a36763404151bb164dbc4ad2c202c348efaedc81ce7cd847a` | [`9de1bd2a69c0d47e143c8abcf3305bb8f633aa97cb779650e4f9f42f893b9842`](44-3681-keyboard-shortcuts-dark.geometry.json) |
| [`44-3681-keyboard-shortcuts-zoom200.png`](44-3681-keyboard-shortcuts-zoom200.png) | `44:3681` | Light | 200% | 1600 × 960 | `48ddf9f3d10b22f037ba4893ba911fbe35bd6a3922b3ade8eb5d06e31a878fa7` | [`c6ee7866e70422d02161d10959fe013d1043379271cbb4fcb80332313ab29fc1`](44-3681-keyboard-shortcuts-zoom200.geometry.json) |
| [`44-3858-about-light.png`](44-3858-about-light.png) | `44:3858` | Light | 100% | 1600 × 960 | `252dd7a1d6606e1208f205082003f7f0728b2b3935e00f634c52690df0cbb1a7` | [`db4dc3f221dd361cd47224c18f65986a8e83899882582addf33dce728daba332`](44-3858-about-light.geometry.json) |
| [`44-3858-about-dark.png`](44-3858-about-dark.png) | `44:3858` | Dark | 100% | 1600 × 960 | `981dae937822d4387147c91c8ff65036cfa4fe4a6a51f7a8ee83f461e8cddfcf` | [`3479807b0e328cfd3d1c21b7f9c47e256623e7eacbf50f271095e24f8422eb9e`](44-3858-about-dark.geometry.json) |
| [`44-3858-about-zoom200.png`](44-3858-about-zoom200.png) | `44:3858` | Light | 200% | 1600 × 960 | `6535e3f3a95280a3e2e780cfe458823ef0f06a9f81a6d3fee075362fb52d7ad9` | [`bca0e7ea46303d009cfa94ccae42d35521d3c0d043c0e314a138aea76da4a22d`](44-3858-about-zoom200.geometry.json) |

## Capture program

A scratch binary crate outside the repository at `/tmp/fzdp-ms1/t24-capture/`, with its own target directory, a copy of the checkout's `Cargo.lock` and `rust-toolchain.toml`, and path dependencies on the checkout's crates. Resolving it added the scratch package to the lock and dropped the checkout's packages the scratch crate does not reach (dev-dependencies such as `proptest` and `dify`); every package it kept is at the checkout's version. It was built with `nice -n 10 cargo build` and run, from any directory, as `<target>/debug/t24-capture <output-dir> <checkout>/crates/fuzzy-dedupe-app`; the program enters the app crate's directory itself, because New scan's seeded folders are crate-relative. `<checkout>` stands for the checkout's path.

```toml
[package]
name = "t24-capture"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
fuzzy-dedupe-app = { path = "<checkout>/crates/fuzzy-dedupe-app" }
fuzzy-dedupe-engine = { path = "<checkout>/crates/fuzzy-dedupe-engine" }
fuzzy-dedupe-platform = { path = "<checkout>/crates/fuzzy-dedupe-platform" }
fuzzy-dedupe-domain = { path = "<checkout>/crates/fuzzy-dedupe-domain" }
egui = { version = "=0.37.0", default-features = false }
egui_kittest = { version = "=0.37.0", features = ["wgpu"] }
egui-wgpu = { version = "=0.37.0", default-features = false }
image = { version = "0.25.10", default-features = false, features = ["png"] }

[workspace]
```

```rust
//! Scratch capture for EV-002 (plan T24): renders the 16 non-dark review
//! states of crates/fuzzy-dedupe-app/tests/snapshots.rs through the whole app
//! (`FuzzyDedupeApp::new`) on the `design` scenario, in the light and dark
//! themes and at 200% in-app zoom (light), and writes one PNG per capture.
//!
//! Differences from tests/snapshots.rs, each deliberate:
//! - Fonts come from the production `SystemFontLookup` (fontconfig), not the
//!   embedded-only lookup the CI baselines need.
//! - The renderer is wgpu on this machine's hardware Vulkan adapter. kittest's
//!   default setup prefers a CPU device, which here would be Mesa lavapipe,
//!   so the selector below refuses CPU adapters and fails if none is left.
//! - At 200% zoom the window keeps the frame's pixel size and the UI gets
//!   half the room in points, as in a real 1600 x 960 window: egui-winit sends
//!   screen size in points = physical size / (native scale x zoom). Here egui
//!   does it itself: kittest sends `screen_rect` only on the first pass
//!   (`RawInput::take`), and on a zoom change egui rescales the rect it kept
//!   by old/new zoom. Calling `set_size(size / zoom)` as well would halve it
//!   twice. The asserts in `shoot` pin both the point size and the PNG size.
//! - kittest's default missing-glyph policy stays on: a missing glyph panics,
//!   so a completed capture has no tofu.
//!
//! Round 3: the rig draws into the full content rect, and the frame states
//! are those of tests/snapshots.rs at T41 (seeded New scan, Scanning with the
//! results cleared and its progress segment checked on the track, the
//! Findings view state), with input time pinned to 1000.8 s, mid-sweep. Beside every PNG it writes
//! `<capture>.geometry.json`: the accesskit nodes a reviewer measures, with
//! bounds in pixels.
//!
//! Usage: `t24-capture <output-dir> <app-crate-dir> [stem-filter]`. The
//! working directory is set to `<app-crate-dir>` before any capture, because
//! New scan's seeded folders (`src`, `tests`, `assets`) are crate-relative,
//! exactly as tests/snapshots.rs resolves them under cargo; from any other
//! directory the popup would mark them "Folder not found".
//!
//! Requirements: a hardware Vulkan adapter, fontconfig at run time, a
//! writable /tmp for the app's throwaway state directories, and `HOME` set,
//! so the geometry dump can strip the real home path from node text.
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use egui::accesskit::{Role, Toggled};
use egui::{Key, Theme, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_wgpu::wgpu;
use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
use fuzzy_dedupe_app::measure::NoMeasurement;
use fuzzy_dedupe_app::presentation::commands::UiIntent;
use fuzzy_dedupe_app::presentation::state::{
    CleanupPhase, DialogKind, DialogSubject, MenuKind, PresentationState, RowId, ScanStage,
};
use fuzzy_dedupe_app::presentation::{events, projections};
use fuzzy_dedupe_app::ui::dialogs::new_scan;
use fuzzy_dedupe_app::ui::theme::{self, Zoom};
use fuzzy_dedupe_domain::{CompareMode, MemberRole, QueueOrigin, ScanFolder, ScanSetup};
use fuzzy_dedupe_engine::cleanup_sim::CleanupEvent;
use fuzzy_dedupe_engine::fixtures::design;
use fuzzy_dedupe_engine::service::{
    EngineConfig, EngineEvent, EngineRequest, ExecuteApprovedPlan, Origin, ReviewCommand,
};
use fuzzy_dedupe_platform::dirs::AppDirs;
use fuzzy_dedupe_platform::fonts::{FontLookup, SystemFontLookup};
use fuzzy_dedupe_platform::log_retention::LogDate;
use fuzzy_dedupe_platform::open::{Location, OpenHandle, Opener};
use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

// Same values as tests/snapshots.rs.
const SETTLE_STEPS: u64 = 16;
/// 1000.8 is half a 1.6 s sweep past a whole number of sweeps, so 01b's
/// progress segment sits mid-track (T41); 1000.0 is phase 0, with the
/// segment wholly left of the track.
const CAPTURE_TIME: f64 = 1000.8;
const FULL: Vec2 = Vec2::new(1600.0, 960.0);
const MINIMUM: Vec2 = Vec2::new(1024.0, 600.0);
const FUZZY_ROW: &str = "Design_Spec_draft.txt";
const STATE_ROOT: &str = "/tmp/fzdp-ms1/t24-capture-state";

static ADAPTER: Mutex<Option<String>> = Mutex::new(None);

struct NoOpener;
impl Opener for NoOpener {
    fn open(&self, _: Location, notify: Box<dyn FnOnce() + Send>) -> OpenHandle {
        notify();
        OpenHandle::ready(Ok(()))
    }
}

struct DismissPicker;
impl FolderPicker for DismissPicker {
    fn pick_folders(&self, _: PickRequest, notify: Box<dyn FnOnce() + Send>) -> PickHandle {
        PickHandle::spawn(|| PickOutcome::Dismissed, notify)
    }
}

struct FixedClock;
impl Clock for FixedClock {
    fn today(&self) -> LogDate {
        LogDate::new(2026, 11, 23).expect("valid date")
    }
}

fn dirs_for(name: &str) -> AppDirs {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
    let root: PathBuf = Path::new(STATE_ROOT).join(format!("{name}-{nanos}"));
    let state = root.join("state");
    AppDirs::resolve(
        move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
        &root.join("home"),
    )
}

/// kittest's headless setup with a selector that takes the first hardware
/// Vulkan adapter instead of preferring a CPU rasterizer.
fn hardware_setup() -> egui_wgpu::WgpuSetup {
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = wgpu::Backends::VULKAN;
    setup.native_adapter_selector = Some(Arc::new(|adapters, _surface| {
        let mut hw: Vec<_> = adapters
            .iter()
            .filter(|a| {
                let i = a.get_info();
                i.backend == wgpu::Backend::Vulkan
                    && matches!(
                        i.device_type,
                        wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::DiscreteGpu
                    )
            })
            .collect();
        hw.sort_by_key(|a| match a.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu => 0,
            _ => 1,
        });
        let chosen = hw.first().ok_or_else(|| "no hardware Vulkan adapter".to_owned())?;
        let i = chosen.get_info();
        let desc = format!(
            "{} ({:?}, {:?}), driver {} / {}",
            i.name, i.device_type, i.backend, i.driver, i.driver_info
        );
        let mut slot = ADAPTER.lock().unwrap();
        if slot.is_none() {
            println!("adapter: {desc}");
        }
        *slot = Some(desc);
        Ok((*chosen).clone())
    }));
    egui_wgpu::WgpuSetup::CreateNew(setup)
}

type Shell = Harness<'static, Option<FuzzyDedupeApp>>;

fn launch(name: &str, size: Vec2, theme: Theme) -> Shell {
    let deps = AppDeps {
        dirs: dirs_for(name),
        opener: Box::new(NoOpener),
        picker: Box::new(DismissPicker),
        fonts: Box::new(SystemFontLookup),
        clock: Box::new(FixedClock),
        log_expiry: Box::new(|_| {}),
        engine: EngineConfig::default(),
        measure: Box::new(NoMeasurement),
    };
    let mut harness = Harness::builder()
        .with_max_steps(SETTLE_STEPS)
        .with_size(size)
        .with_theme(theme)
        .with_render_cursor(false)
        .wgpu_setup(hardware_setup())
        .build_ui_state(
            |ui, app: &mut Option<FuzzyDedupeApp>| {
                if let Some(app) = app.as_mut() {
                    app.run_logic(ui.ctx());
                    // kittest's `build_ui_state` Ui sits inside an 8-unit
                    // margin; eframe gives the app the whole client area.
                    // Same rig as tests/snapshots.rs and tests/layout.rs (T40
                    // R1): draw into a child spanning the content rect.
                    let full = ui.ctx().content_rect();
                    let mut window = ui.new_child(egui::UiBuilder::new().max_rect(full));
                    app.draw(&mut window);
                }
            },
            None,
        );
    harness.input_mut().system_theme = Some(theme);
    let app = FuzzyDedupeApp::new(&harness.ctx, deps).expect("engine starts");
    *harness.state_mut() = Some(app);
    harness.run();
    harness
}

fn app(h: &Shell) -> &FuzzyDedupeApp {
    h.state().as_ref().expect("app")
}
fn state(h: &Shell) -> &PresentationState {
    &app(h).state
}
fn state_mut(h: &mut Shell) -> &mut PresentationState {
    &mut h.state_mut().as_mut().expect("app").state
}
fn push(h: &mut Shell, intent: UiIntent) {
    h.state_mut().as_mut().expect("app").push_intent(intent);
}

fn run_until(h: &mut Shell, what: &str, done: impl Fn(&PresentationState) -> bool) {
    for _ in 0..500 {
        h.step();
        if done(state(h)) {
            h.run();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

fn open_dialog(h: &mut Shell, kind: DialogKind) {
    push(h, UiIntent::OpenDialog { kind, subject: DialogSubject::None, invoker: None });
    h.run();
    assert_eq!(state(h).dialog.dialog().map(|d| d.kind), Some(kind));
}

fn open_cleanup(h: &mut Shell) {
    open_dialog(h, DialogKind::Cleanup);
    run_until(h, "the cleanup plan", |s| s.operation.phase == CleanupPhase::Review);
}

#[track_caller]
fn assert_shown(h: &Shell, text: &str) {
    assert!(h.query_all_by_label(text).next().is_some(), "{text:?} is not shown");
}

// The 16 review states, copied from tests/snapshots.rs `Frame`.
#[derive(Clone, Copy, Debug)]
enum Frame {
    Findings,
    Compare,
    MatchingRules,
    NewScan,
    ReviewUniqueText,
    ReviewCleanup,
    BeforeFirstScan,
    Scanning,
    NoSearchMatches,
    CleanupRunning,
    CleanupFinished,
    FindingsMinimum,
    CompareMinimum,
    MainMenuOpen,
    KeyboardShortcuts,
    About,
}

const FRAMES: &[Frame] = &[
    Frame::Findings,
    Frame::Compare,
    Frame::MatchingRules,
    Frame::NewScan,
    Frame::ReviewUniqueText,
    Frame::ReviewCleanup,
    Frame::BeforeFirstScan,
    Frame::Scanning,
    Frame::NoSearchMatches,
    Frame::CleanupRunning,
    Frame::CleanupFinished,
    Frame::FindingsMinimum,
    Frame::CompareMinimum,
    Frame::MainMenuOpen,
    Frame::KeyboardShortcuts,
    Frame::About,
];

impl Frame {
    const fn stem(self) -> &'static str {
        match self {
            Self::Findings => "13-648-findings",
            Self::Compare => "13-1304-compare",
            Self::MatchingRules => "16-735-matching-rules",
            Self::NewScan => "16-806-new-scan",
            Self::ReviewUniqueText => "16-932-review-unique-text",
            Self::ReviewCleanup => "16-886-review-cleanup",
            Self::BeforeFirstScan => "28-1399-findings-before-first-scan",
            Self::Scanning => "28-2171-findings-scanning",
            Self::NoSearchMatches => "30-2370-findings-no-search-matches",
            Self::CleanupRunning => "27-1390-cleanup-running",
            Self::CleanupFinished => "27-1533-cleanup-finished",
            Self::FindingsMinimum => "29-2214-findings-1024x600",
            Self::CompareMinimum => "29-3648-compare-1024x600",
            Self::MainMenuOpen => "44-3954-findings-main-menu-open",
            Self::KeyboardShortcuts => "44-3681-keyboard-shortcuts",
            Self::About => "44-3858-about",
        }
    }

    const fn size(self) -> Vec2 {
        match self {
            Self::FindingsMinimum | Self::CompareMinimum => MINIMUM,
            _ => FULL,
        }
    }

    fn prepare(self, h: &mut Shell) {
        match self {
            Self::Findings => focus_frame_row(h),
            Self::FindingsMinimum => {
                assert_eq!(state(h).scan.stage, ScanStage::Completed);
                assert!(state(h).scan.results.is_some(), "design results");
            }
            Self::Compare | Self::CompareMinimum => open_design_pair(h),
            Self::MatchingRules => open_dialog(h, DialogKind::MatchingRules),
            Self::NewScan => open_seeded_new_scan(h),
            Self::KeyboardShortcuts => open_dialog(h, DialogKind::KeyboardShortcuts),
            Self::About => open_dialog(h, DialogKind::About),
            Self::ReviewUniqueText => open_unique_text_review(h),
            Self::ReviewCleanup => {
                open_cleanup(h);
                assert_shown(h, "Review cleanup");
            }
            Self::BeforeFirstScan => {
                let preferences = state(h).preferences.clone();
                *state_mut(h) = PresentationState { preferences, ..PresentationState::default() };
                h.run();
                assert_shown(h, "No findings yet");
            }
            Self::Scanning => scanning(h),
            Self::NoSearchMatches => {
                let search = design::sample().search_without_matches;
                state_mut(h).findings.search.clone_from(&search);
                h.run();
                assert_shown(h, &format!("No files match “{search}”"));
            }
            Self::CleanupRunning => cleanup_running(h),
            Self::CleanupFinished => {
                open_cleanup(h);
                h.get_by_role_and_label(Role::Button, "Move 3 files to Trash").click_accesskit();
                h.run();
                run_until(h, "the run's end", |s| s.operation.phase == CleanupPhase::Finished);
                assert_shown(h, "Cleanup finished");
            }
            Self::MainMenuOpen => {
                focus_frame_row(h);
                h.key_press(Key::F10);
                h.run();
                assert_eq!(state(h).dialog.menu(), Some(MenuKind::Main));
            }
        }
    }
}

// Copied verbatim from tests/snapshots.rs at T41 (T40 R2 to R4, and T41's
// pinned capture time with the Scanning frame's segment check).

/// The view state of frames `13:648`, `37:2466` and `44:3954`: the
/// design scenario's expanded groups and its focused row,
/// `Design_Spec_draft.txt` in G004, which enables the toolbar's Compare.
///
/// The expanded set is the fixture's (`DesignSample::expanded_groups`):
/// G003, G002, G004 and G009, plus G001 and G005, which the reference
/// PNG also shows expanded; every other group stays collapsed. The row
/// is focused in Findings' own state with `reveal_focus`, as a keyboard
/// move leaves it, so the table scrolls it into view. Keyboard focus is
/// deliberately not given to the table: the frames show the row's fill
/// without the focus ring or the full-path tooltip that table focus
/// draws (F12).
fn focus_frame_row(h: &mut Shell) {
    assert_eq!(state(h).scan.stage, ScanStage::Completed);
    let sample = design::sample();
    let file = sample
        .focused_file
        .expect("the design scenario's focused row");
    let results = state(h).scan.results.clone().expect("design results");
    let group = results.group_of(file).expect("the focused row's group");
    let member = group.member(file).expect("the focused member");
    assert_eq!(member.name, FUZZY_ROW, "the frame focuses {FUZZY_ROW}");

    let expanded: BTreeSet<String> = results
        .groups
        .iter()
        .filter(|g| state(h).findings.expanded.contains(&g.id))
        .map(|g| g.label.clone())
        .collect();
    let frame: BTreeSet<String> = ["G001", "G005", "G003", "G002", "G004", "G009"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(expanded, frame, "the frame's expanded groups, no others");
    assert_eq!(
        state(h).findings.expanded.len(),
        frame.len(),
        "no expanded group outside these results"
    );

    let findings = &mut state_mut(h).findings;
    findings.focused = Some(RowId::Member {
        group: group.id,
        file,
    });
    findings.reveal_focus = true;
    h.run();
    assert_shown(h, FUZZY_ROW);
    assert_shown(h, &projections::group_label(group));
    // The toolbar's Compare is the rightmost button of that name; the
    // rail's Compare entry shares the name.
    let compare = h
        .query_all_by_role_and_label(Role::Button, "Compare")
        .max_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .expect("the toolbar's Compare");
    assert!(
        !compare.accesskit_node().is_disabled(),
        "Compare is disabled with {FUZZY_ROW} focused"
    );
}

/// Frame `16:806`: New scan opened on a stored setup with three
/// folders, the first protected, and the design fixture's explicit file
/// types, committed through the same `SetScanSetup` intent Start scan
/// sends. The empty-list "All plain-text files" state is
/// `conform_popups`' business, not this frame's.
///
/// The folders are crate-relative (`src`, `tests`, `assets`), resolved
/// against the package root that cargo and nextest run tests from. The
/// popup checks each folder's existence when it opens and marks a
/// missing one in red, so the folders must exist; and the listed path
/// is drawn verbatim, so an absolute path (`CARGO_MANIFEST_DIR`) would
/// put the checkout location into the baseline's pixels. The fixture's
/// own `~/Documents/…` folders exist nowhere and would all show as not
/// found.
fn open_seeded_new_scan(h: &mut Shell) {
    const FOLDERS: [(&str, bool); 3] = [("src", true), ("tests", false), ("assets", false)];
    let file_types = design::sample().new_scan.file_types_shown;
    assert!(!file_types.is_empty(), "an explicit file-type list");
    let setup = ScanSetup {
        folders: FOLDERS
            .iter()
            .map(|(path, protect)| ScanFolder {
                path: PathBuf::from(path),
                protect: *protect,
            })
            .collect(),
        file_types: file_types.clone(),
    };
    push(h, UiIntent::SetScanSetup(setup.clone()));
    h.run();
    assert_eq!(state(h).preferences.scan_setup, setup);
    open_dialog(h, DialogKind::NewScan);

    for (path, protect) in FOLDERS {
        let shown = new_scan::shown_path(Path::new(path));
        assert_shown(h, &shown);
        let toggled = h
            .get_by_role_and_label(Role::CheckBox, &format!("Protect files in {shown}"))
            .accesskit_node()
            .toggled();
        let expected = if protect {
            Toggled::True
        } else {
            Toggled::False
        };
        assert_eq!(toggled, Some(expected), "Protect files in {shown}");
    }
    assert!(
        h.query_by_label_contains(new_scan::MISSING_FOLDER)
            .is_none(),
        "a seeded folder is shown as not found"
    );
    let summary = file_types
        .iter()
        .map(|t| format!(".{t}"))
        .collect::<Vec<_>>()
        .join("   ");
    assert_shown(h, &summary);
    assert!(
        h.query_by_label(new_scan::ALL_FILE_TYPES).is_none(),
        "the explicit list is shown as {:?}",
        new_scan::ALL_FILE_TYPES
    );
}

/// Frame `28:2171`: a scan running after Start scan cleared the queue
/// and the results it replaces (FR-026).
///
/// Not reached through the real Start scan: the simulated scan runs on
/// its own threads for a wall time the rig does not control
/// (`scan_sim::job`, files × a fixed cost), then completes, and its
/// progress events carry the measured counts and elapsed time, so a
/// capture could neither hold the running state nor show stable text.
/// Nor does Start clear the presentation's results: they stay until the
/// scan's `Completed` event replaces them (`events::install_results`).
/// So the queue is cleared by Start scan's own command, `ClearForNewScan`,
/// through the review service, and the rig then drops the results and
/// applies the design sample's progress.
fn scanning(h: &mut Shell) {
    assert!(state(h).queue.queued_count() > 0, "the design queue");
    push(
        h,
        UiIntent::Review {
            command: ReviewCommand::ClearForNewScan,
            invoker: None,
        },
    );
    h.run();
    assert_eq!(state(h).queue.queued_count(), 0, "Start's clear ran");

    let sample = design::sample();
    let s = state_mut(h);
    s.scan.results = None;
    s.scan.stage = ScanStage::Running;
    s.scan.progress = Some(sample.scan_progress);
    s.findings.revision += 1;
    // The animated progress bar never settles; a fixed number of
    // frames draws the scanning view, at the capture's own time so the
    // segment checked here is the one captured.
    h.input_mut().time = Some(CAPTURE_TIME);
    h.run_steps(4);
    assert_shown(h, "Scanning…");
    assert_segment_on_track(h);
    assert!(state(h).scan.results.is_none(), "results cleared");
    let review = h.get_by_role_and_label(Role::Button, "Review cleanup (0)");
    assert!(
        review.accesskit_node().is_disabled(),
        "Review cleanup (0) is enabled"
    );
}

/// The 01b progress segment is painted on the track, in `primary`, at
/// least a quarter of the track wide: visibly moving, not parked off
/// the track at a sweep boundary.
fn assert_segment_on_track(h: &Shell) {
    fn walk(shape: &egui::Shape, out: &mut Vec<egui::epaint::RectShape>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            egui::Shape::Rect(r) => out.push(r.clone()),
            _ => {}
        }
    }
    let track = h.get_by_label("Scan progress").rect();
    let mode = match h.ctx.theme() {
        Theme::Light => theme::Mode::Light,
        Theme::Dark => theme::Mode::Dark,
    };
    let primary = theme::palette(mode).color(theme::Token::Primary);
    let mut rects = Vec::new();
    for clipped in &h.output().shapes {
        walk(&clipped.shape, &mut rects);
    }
    let shown = rects
        .iter()
        .filter(|r| r.fill == primary)
        .map(|r| r.rect.intersect(track).width())
        .fold(0.0, f32::max);
    assert!(
        shown >= track.width() / 4.0,
        "the progress segment shows {shown} of the {} track",
        track.width()
    );
}

fn open_design_pair(h: &mut Shell) {
    let pair = design::sample().last_compared.expect("design pair");
    push(h, UiIntent::OpenCompare { target: pair.b, counterpart: pair.a, return_row: None });
    run_until(h, "Compare's comparison", |s| s.compare.comparison.aligned().is_some());
}

fn open_unique_text_review(h: &mut Shell) {
    let results = state(h).scan.results.clone().expect("results");
    let member = results
        .groups
        .iter()
        .flat_map(|g| g.members.iter())
        .find(|m| m.name == FUZZY_ROW)
        .expect("the fuzzy row in the design scenario");
    let MemberRole::Member(link) = member.role else {
        panic!("{FUZZY_ROW} is a reference");
    };
    push(
        h,
        UiIntent::Review {
            command: ReviewCommand::RequestQueue {
                file: member.file,
                counterpart: link.counterpart,
                origin: QueueOrigin::Shortcut,
                reviewed: None,
            },
            invoker: None,
        },
    );
    h.run();
    let dialog = state(h).dialog.dialog().expect("a popup is open");
    let DialogSubject::UniqueText(subject) = dialog.subject else {
        panic!("Review unique text expected, got {:?}", dialog.kind);
    };
    let key = subject.comparison(CompareMode::Original);
    run_until(h, "the popup's comparison", move |s| {
        s.compare.comparison.matches(&key) && s.compare.comparison.aligned().is_some()
    });
}

fn cleanup_running(h: &mut Shell) {
    open_cleanup(h);
    let s = state_mut(h);
    let plan = s.operation.revision.expect("a plan");
    let items = s.operation.items.clone();
    let request = EngineRequest::ExecuteApprovedPlan(ExecuteApprovedPlan { plan });
    events::note_submitted(s, Origin::Plan(plan), &request);
    for (item, status) in items.iter().zip(&design::sample().cleanup_running) {
        let mut changed = *item;
        changed.status = *status;
        events::apply(s, EngineEvent::Cleanup { plan, event: CleanupEvent::ItemChanged(changed) });
    }
    h.run();
    assert_shown(h, "Moving files to Trash");
    assert_shown(h, "1 of 3 files moved");
}

/// Prepares `frame` and writes its PNG. `zoom` is set through the zoom
/// preference before the state is prepared, so `app.rs` applies it with
/// `theme::apply_zoom` exactly as for a remembered preference.
fn shoot(frame: Frame, theme: Theme, zoom: Option<Zoom>, out_dir: &Path) {
    let suffix = match (theme, zoom) {
        (_, Some(z)) => format!("zoom{}", z.percent()),
        (Theme::Light, None) => "light".to_owned(),
        (Theme::Dark, None) => "dark".to_owned(),
    };
    let name = format!("{}-{suffix}", frame.stem());
    let size = frame.size();
    let mut h = launch(&name, size, theme);
    if let Some(z) = zoom {
        state_mut(&mut h).preferences.zoom = z;
        h.run();
        assert!((h.ctx.zoom_factor() - z.factor()).abs() < 1e-4, "zoom applied");
        let points = h.ctx.content_rect().size();
        assert!((points - size / z.factor()).length() < 0.5, "{name}: {points:?} points");
    }
    frame.prepare(&mut h);
    h.remove_cursor();
    h.input_mut().time = Some(CAPTURE_TIME);
    h.run_steps(2);
    println!(
        "{name}: zoom {} ppp {} content {:?}",
        h.ctx.zoom_factor(),
        h.ctx.pixels_per_point(),
        h.ctx.content_rect().size()
    );
    let image = h.render().expect("render");
    let (w, hgt) = image.dimensions();
    assert_eq!((w as f32, hgt as f32), (size.x, size.y), "{name}: pixel size");
    let path = out_dir.join(format!("{name}.png"));
    image.save(&path).expect("save png");
    println!("wrote {} ({w}x{hgt})", path.display());
    let geometry = out_dir.join(format!("{name}.geometry.json"));
    let json = geometry_json(&h, frame, &name, theme, zoom, (w, hgt));
    std::fs::write(&geometry, json).expect("save geometry");
    println!("wrote {}", geometry.display());
}

/// Roles the geometry dump lists, besides any node with a label. Dialog and
/// Menu are not in the reviewer's measurement list; they are listed so a
/// popup's nodes get a `container`. egui's main menu exposes no Menu node,
/// so no dump has one, and the sheets crop popups by `popup_rect_px`.
const LISTED_ROLES: &[Role] = &[
    Role::Button,
    Role::CheckBox,
    Role::TextInput,
    Role::Row,
    Role::Cell,
    Role::ColumnHeader,
    Role::Group,
    Role::Label,
    Role::Tooltip,
    Role::Dialog,
    Role::Menu,
];

/// A JSON string literal. Hand-rolled because the scratch crate takes no
/// dependency outside the checkout's lock; it escapes every character JSON
/// requires (quote, backslash, U+0000 to U+001F), which is all a valid
/// literal needs, since the output is UTF-8.
fn json_str(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Node text with the real home directory replaced by `~`. The fixtures use
/// `/home/fixture/…`, which stays; the capture's own environment must never
/// reach a committed file. Panics if the home path survives the replacement.
fn scrub(text: String, home: &str) -> String {
    let out = if home.len() > 1 { text.replace(home, "~") } else { text };
    assert!(home.len() <= 1 || !out.contains(home), "home path in node text");
    out
}

fn num(v: f64) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    if r == r.trunc() { format!("{r:.0}") } else { format!("{r}") }
}

/// The accesskit dump written beside each PNG. Bounds are accesskit's own
/// bounding boxes, which egui reports in physical pixels (kittest's
/// `Node::rect` divides them by pixels-per-point to get points), so they
/// are the PNG's pixel coordinates at every zoom. `parent` is the id of the
/// nearest listed ancestor, and `container` the id of the nearest Dialog,
/// Menu or Tooltip ancestor, so a reader can keep a popup's nodes apart from
/// the window drawn behind it.
fn geometry_json(
    h: &Shell,
    frame: Frame,
    name: &str,
    theme: Theme,
    zoom: Option<Zoom>,
    (w, hgt): (u32, u32),
) -> String {
    let home = std::env::var("HOME").expect("HOME is set");
    let mut nodes: Vec<String> = Vec::new();
    // Depth-first, children in tree order: (node, listed parent id, container id).
    let root = h.kittest_state().root();
    let mut stack = vec![(root, None::<usize>, None::<usize>)];
    while let Some((node, parent, container)) = stack.pop() {
        let role = node.role();
        let label = node.label().filter(|l| !l.is_empty());
        let listed = label.is_some() || LISTED_ROLES.contains(&role);
        let mut child_parent = parent;
        let mut child_container = container;
        if listed && !node.is_root() {
            let id = nodes.len();
            let rect = node.bounding_box();
            let rect = match rect {
                Some(r) => format!("[{}, {}, {}, {}]", num(r.x0), num(r.y0), num(r.x1), num(r.y1)),
                None => "null".to_owned(),
            };
            let opt = |v: Option<String>| match v {
                Some(t) => json_str(&scrub(t, &home)),
                None => "null".to_owned(),
            };
            let toggled = match node.toggled() {
                Some(Toggled::True) => "true",
                Some(Toggled::False) => "false",
                Some(Toggled::Mixed) => "\"mixed\"",
                None => "null",
            };
            let idx = |v: Option<usize>| v.map_or("null".to_owned(), |i| i.to_string());
            nodes.push(format!(
                "{{\"id\": {id}, \"parent\": {}, \"container\": {}, \"role\": {}, \"label\": {}, \"value\": {}, \"rect_px\": {rect}, \"disabled\": {}, \"toggled\": {toggled}}}",
                idx(parent),
                idx(container),
                json_str(&format!("{role:?}")),
                opt(label),
                opt(node.value()),
                node.is_disabled(),
            ));
            child_parent = Some(id);
            if matches!(role, Role::Dialog | Role::Menu | Role::Tooltip) {
                child_container = Some(id);
            }
        }
        let children: Vec<_> = node.children().collect();
        for child in children.into_iter().rev() {
            stack.push((child, child_parent, child_container));
        }
    }
    let theme = match theme {
        Theme::Light => "light",
        Theme::Dark => "dark",
    };
    // The open popup's rect. accesskit gives egui's Modal Dialog node no
    // bounds, so the rect comes from the modal Area's remembered rect, under
    // the id dialog_host's `draw` gives it (`Id::unique(("fd.dialog",
    // kind))`); if that id changes there, this lookup finds nothing and the
    // assert below stops the run rather than write a dump without the rect.
    let ppp = f64::from(h.ctx.pixels_per_point());
    let popup = state(h).dialog.dialog().map(|d| {
        let id = egui::Id::unique(("fd.dialog", d.kind));
        let r = h.ctx.memory(|m| m.area_rect(id)).expect("the open popup's area rect");
        format!(
            "[{}, {}, {}, {}]",
            num(f64::from(r.min.x) * ppp),
            num(f64::from(r.min.y) * ppp),
            num(f64::from(r.max.x) * ppp),
            num(f64::from(r.max.y) * ppp)
        )
    });
    let popup = popup.unwrap_or_else(|| "null".to_owned());
    let frame_id = frame.stem().splitn(3, '-').take(2).collect::<Vec<_>>().join(":");
    format!(
        "{{\"capture\": {}, \"frame\": {}, \"theme\": {}, \"zoom_percent\": {}, \"pixels_per_point\": {}, \"window_px\": [{w}, {hgt}], \"units\": \"rect_px and popup_rect_px are [x0, y0, x1, y1] in PNG pixels (points times pixels_per_point)\", \"popup_rect_px\": {popup}, \"nodes\": [{}]}}\n",
        json_str(&format!("{name}.png")),
        json_str(&frame_id),
        json_str(theme),
        zoom.map_or(100, |z| z.percent()),
        num(f64::from(h.ctx.pixels_per_point())),
        nodes.join(", "),
    )
}

fn main() {
    let dir = std::env::args().nth(1).expect("output dir");
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).expect("output dir");
    for font in SystemFontLookup.fallback_fonts() {
        println!("fallback family: {}", font.family);
    }
    let app_dir = std::env::args().nth(2).expect("app crate dir");
    std::env::set_current_dir(&app_dir).expect("enter the app crate dir");
    for folder in ["src", "tests", "assets"] {
        assert!(Path::new(folder).is_dir(), "{folder}/ under {app_dir}");
    }
    let only = std::env::args().nth(3);
    let zoom200 = Zoom::from_percent(200).expect("on the grid");
    for &frame in FRAMES {
        if only.as_deref().is_some_and(|o| !frame.stem().contains(o)) {
            continue;
        }
        shoot(frame, Theme::Light, None, dir);
        shoot(frame, Theme::Dark, None, dir);
        shoot(frame, Theme::Light, Some(zoom200), dir);
    }
}
```
