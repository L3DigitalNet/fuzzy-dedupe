// SPDX-FileCopyrightText: 2026 L3Digital LLC
// SPDX-License-Identifier: GPL-3.0-only

//! PV-T34-002 (plan T34): one icon-glyph registry, installed before the
//! first pass.
//!
//! Every test sits in the `glyph_registry` module so
//! `-E 'test(glyph_registry)'` selects them. The first-frame tests run the
//! whole app on the design scenario under `egui_kittest`'s default
//! `MissingGlyphPolicy::Panic`, so laying out a code point whose rasterizer
//! is not yet active fails the test by panicking. They compare the
//! private-use characters painted on the first frame after
//! `FuzzyDedupeApp::new` with those painted once the UI has settled: a view
//! that waits a frame for its glyphs paints fewer icons on the first.
//!
//! The structural tests read `src/**` as text. They are the gate that keeps
//! glyph registration in `ui/glyphs.rs`: a view that registers its own
//! rasterizer or assigns its own code point fails here.

mod glyph_registry {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use egui::Vec2;
    use egui::epaint::Shape;
    use egui_kittest::Harness;
    use fuzzy_dedupe_app::app::{AppDeps, Clock, FuzzyDedupeApp};
    use fuzzy_dedupe_app::measure::NoMeasurement;
    use fuzzy_dedupe_app::presentation::commands::IntentQueue;
    use fuzzy_dedupe_app::presentation::state::{DialogKind, DialogSubject};
    use fuzzy_dedupe_app::ui::glyphs;
    use fuzzy_dedupe_app::ui::theme::{self, Icon};
    use fuzzy_dedupe_app::ui::widgets::dialog_host;
    use fuzzy_dedupe_domain::QueueScope;
    use fuzzy_dedupe_engine::service::EngineConfig;
    use fuzzy_dedupe_platform::dirs::AppDirs;
    use fuzzy_dedupe_platform::fonts::SystemFontLookup;
    use fuzzy_dedupe_platform::log_retention::LogDate;
    use fuzzy_dedupe_platform::open::{Location, OpenHandle, Opener};
    use fuzzy_dedupe_platform::picker::{FolderPicker, PickHandle, PickOutcome, PickRequest};

    /// Step bound for every `Harness::run` here, in place of kittest's
    /// default of 4.
    ///
    /// A result from another thread (engine work, the folder picker, the
    /// opener) may arrive part-way through a run, and the widgets it draws for
    /// the first time each ask egui for up to two more frames (`widget_style`).
    /// On slow CI runners that overran 4 steps (PR #39, "would have settled
    /// after 6 steps"). 16 covers it with margin. A continuous repaint, from an
    /// animation or a repaint loop, still exhausts 16, so `run()` keeps
    /// catching runaway repaints.
    ///
    /// Each harness test file under `tests/` declares its own copy, because
    /// they share no module; keep the copies equal.
    const SETTLE_STEPS: u64 = 16;

    // ── Doubles ──────────────────────────────────────────────────────────

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

    /// A fresh home under the test's own target directory, unique per run
    /// because tests may not delete what an earlier run left behind.
    fn dirs_for(name: &str) -> AppDirs {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after 1970")
            .as_nanos();
        let root: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("glyph_registry")
            .join(format!("{name}-{nanos}"));
        let state = root.join("state");
        AppDirs::resolve(
            move |var| (var == "XDG_STATE_HOME").then(|| state.clone().into_os_string()),
            &root.join("home"),
        )
    }

    // ── First frame ──────────────────────────────────────────────────────

    type Shell = Harness<'static, Option<FuzzyDedupeApp>>;

    /// The app on the design scenario, built but not yet drawn: the harness
    /// ran its own initialization frame before the app existed, so the next
    /// `step` is the first frame after `FuzzyDedupeApp::new`. With `popup`,
    /// that popup is already open when the first frame draws.
    fn launch(name: &str, popup: Option<(DialogKind, DialogSubject)>) -> Shell {
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
            .with_size(Vec2::new(1600.0, 960.0))
            .build_ui_state(
                |ui, app: &mut Option<FuzzyDedupeApp>| {
                    if let Some(app) = app.as_mut() {
                        app.run_logic(ui.ctx());
                        app.draw(ui);
                    }
                },
                None,
            );
        let mut app = FuzzyDedupeApp::new(&harness.ctx, deps).expect("engine starts");
        if let Some((kind, subject)) = popup {
            // Opened on the state directly rather than through a chord or an
            // intent, either of which takes effect only after a frame has
            // drawn. The follow-up intents `open` may queue are not needed:
            // these popups draw their icons from the state they open on.
            dialog_host::open(
                &mut app.state,
                kind,
                subject,
                None,
                &mut IntentQueue::default(),
            )
            .expect("the popup opens");
        }
        *harness.state_mut() = Some(app);
        harness
    }

    /// Private-use characters (U+E000–U+F8FF) in every text shape of the
    /// last frame's output: the icon glyphs it painted.
    fn painted_glyphs(h: &Shell) -> BTreeSet<char> {
        fn walk(shape: &Shape, found: &mut BTreeSet<char>) {
            match shape {
                Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, found)),
                Shape::Text(text) => found.extend(
                    text.galley
                        .job
                        .text
                        .chars()
                        .filter(|c| ('\u{E000}'..='\u{F8FF}').contains(c)),
                ),
                _ => {}
            }
        }
        let mut found = BTreeSet::new();
        for clipped in &h.output().shapes {
            walk(&clipped.shape, &mut found);
        }
        found
    }

    /// The first frame after `FuzzyDedupeApp::new` paints every icon the
    /// settled view paints. Returns the settled set.
    #[track_caller]
    fn assert_first_frame_paints_every_icon(mut h: Shell, view: &str) -> BTreeSet<char> {
        h.step();
        let first = painted_glyphs(&h);
        h.run();
        let settled = painted_glyphs(&h);
        assert!(!settled.is_empty(), "{view} paints no icon glyph at all");
        let missing: Vec<String> = settled
            .difference(&first)
            .map(|c| format!("U+{:04X}", u32::from(*c)))
            .collect();
        assert!(
            missing.is_empty(),
            "{view}: the first frame did not paint {missing:?}, painted later"
        );
        settled
    }

    #[test]
    fn glyph_registry_shell_and_findings_draw_every_icon_on_the_first_frame() {
        let settled =
            assert_first_frame_paints_every_icon(launch("shell", None), "the shell and Findings");
        // Rail (Findings, Compare, Menu), the Settings gear, and the table's
        // chevrons, file rows and row menus: well over five distinct icons.
        assert!(settled.len() >= 5, "only {} icons painted", settled.len());
    }

    #[test]
    fn glyph_registry_matching_rules_draws_its_check_on_the_first_frame() {
        let popup = (DialogKind::MatchingRules, DialogSubject::None);
        assert_first_frame_paints_every_icon(launch("rules", Some(popup)), "Matching rules");
    }

    #[test]
    fn glyph_registry_new_scan_draws_its_chevron_on_the_first_frame() {
        let popup = (DialogKind::NewScan, DialogSubject::None);
        assert_first_frame_paints_every_icon(launch("new-scan", Some(popup)), "New scan");
    }

    #[test]
    fn glyph_registry_queue_identical_draws_its_checks_on_the_first_frame() {
        let popup = (
            DialogKind::QueueIdentical,
            DialogSubject::QueueIdentical(QueueScope::AllByteIdenticalGroups),
        );
        assert_first_frame_paints_every_icon(
            launch("queue-identical", Some(popup)),
            "Queue identical copies",
        );
    }

    // ── Structure ────────────────────────────────────────────────────────

    fn src_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    /// Every `.rs` file under `src/`, with its path relative to `src/` and
    /// its text with `//` comments removed, so prose that names a call or a
    /// code point does not count as one.
    fn sources() -> Vec<(String, String)> {
        fn visit(dir: &Path, out: &mut Vec<PathBuf>) {
            let entries = std::fs::read_dir(dir)
                .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()));
            for entry in entries {
                let path = entry.expect("directory entry").path();
                if path.is_dir() {
                    visit(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let root = src_dir();
        let mut paths = Vec::new();
        visit(&root, &mut paths);
        assert!(paths.len() > 20, "found only {} source files", paths.len());
        paths
            .into_iter()
            .map(|path| {
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
                let code: String = text
                    .lines()
                    .map(|line| line.split_once("//").map_or(line, |(code, _)| code))
                    .collect::<Vec<_>>()
                    .join("\n");
                let relative = path
                    .strip_prefix(&root)
                    .expect("under src")
                    .to_string_lossy()
                    .replace('\\', "/");
                (relative, code)
            })
            .collect()
    }

    const REGISTRY: &str = "ui/glyphs.rs";

    #[test]
    fn glyph_registry_rasterizers_are_registered_only_in_ui_glyphs() {
        // Every route egui offers for adding a rasterizer, on the context or
        // on the fonts, plus the `SvgGlyph` conversion that builds one.
        const ROUTES: [&str; 4] = [
            "add_glyph_rasterizer",
            "set_glyph_rasterizers",
            "with_glyph_rasterizer",
            "into_rasterizer",
        ];
        let elsewhere: Vec<String> = sources()
            .into_iter()
            .filter(|(path, code)| path != REGISTRY && ROUTES.iter().any(|r| code.contains(r)))
            .map(|(path, _)| path)
            .collect();
        assert!(
            elsewhere.is_empty(),
            "glyph rasterizers registered outside {REGISTRY}: {elsewhere:?}"
        );
        let registry = sources()
            .into_iter()
            .find(|(path, _)| path == REGISTRY)
            .expect("the registry exists");
        assert!(registry.1.contains("add_glyph_rasterizer"));
    }

    /// The private-use code point literals (`'\u{E…}'` to `'\u{F8FF}'`) in
    /// `code`, each with the `Icon::` variant named earlier on its line.
    fn assignments(code: &str) -> Vec<(Option<String>, char)> {
        let mut found = Vec::new();
        for line in code.lines() {
            let mut rest = line;
            while let Some(at) = rest.find("'\\u{") {
                let literal = &rest[at + 4..];
                let Some(end) = literal.find('}') else { break };
                let value = u32::from_str_radix(&literal[..end], 16)
                    .ok()
                    .and_then(char::from_u32);
                if let Some(ch) = value.filter(|c| ('\u{E000}'..='\u{F8FF}').contains(c)) {
                    let before = &line[..line.len() - rest.len() + at];
                    let icon = before.rfind("Icon::").map(|i| {
                        before[i + 6..]
                            .chars()
                            .take_while(char::is_ascii_alphanumeric)
                            .collect::<String>()
                    });
                    found.push((icon, ch));
                }
                rest = &literal[end..];
            }
        }
        found
    }

    #[test]
    fn glyph_registry_private_use_code_points_are_assigned_only_in_ui_glyphs() {
        let elsewhere: Vec<String> = sources()
            .into_iter()
            .filter(|(path, code)| path != REGISTRY && !assignments(code).is_empty())
            .map(|(path, _)| path)
            .collect();
        assert!(
            elsewhere.is_empty(),
            "private-use code points assigned outside {REGISTRY}: {elsewhere:?}"
        );
    }

    #[test]
    fn glyph_registry_each_icon_has_one_code_point_and_each_code_point_one_icon() {
        let mut by_icon: BTreeMap<String, BTreeSet<char>> = BTreeMap::new();
        let mut by_char: BTreeMap<char, BTreeSet<String>> = BTreeMap::new();
        for (_, code) in sources() {
            for (icon, ch) in assignments(&code) {
                let icon = icon.unwrap_or_else(|| "<unnamed>".to_owned());
                by_icon.entry(icon.clone()).or_default().insert(ch);
                by_char.entry(ch).or_default().insert(icon);
            }
        }
        let shared_icons: Vec<_> = by_icon.iter().filter(|(_, c)| c.len() > 1).collect();
        assert!(
            shared_icons.is_empty(),
            "icons with more than one code point: {shared_icons:?}"
        );
        let shared_chars: Vec<_> = by_char.iter().filter(|(_, i)| i.len() > 1).collect();
        assert!(
            shared_chars.is_empty(),
            "code points with more than one icon: {shared_chars:?}"
        );
        let every: BTreeSet<String> = Icon::ALL.iter().map(|i| format!("{i:?}")).collect();
        let assigned: BTreeSet<String> = by_icon.keys().cloned().collect();
        assert_eq!(assigned, every, "every icon has its one code point");
    }

    #[test]
    fn glyph_registry_code_points_are_distinct_and_private_use() {
        let points: BTreeSet<char> = Icon::ALL.iter().map(|&i| glyphs::code_point(i)).collect();
        assert_eq!(points.len(), Icon::ALL.len(), "no two icons share one");
        for &icon in Icon::ALL {
            let ch = glyphs::code_point(icon);
            assert!(
                ('\u{E000}'..='\u{F8FF}').contains(&ch),
                "{icon:?} on U+{:04X}, outside the Private Use Area",
                u32::from(ch)
            );
        }
    }

    #[test]
    fn glyph_registry_draws_nothing_until_installed_then_every_icon() {
        let ctx = egui::Context::default();
        theme::install(&ctx, Vec::new());
        for &icon in Icon::ALL {
            assert_eq!(glyphs::text(&ctx, icon), None, "{icon:?} before install");
        }
        glyphs::install(&ctx);
        glyphs::install(&ctx);
        for &icon in Icon::ALL {
            assert_eq!(
                glyphs::text(&ctx, icon),
                Some(glyphs::code_point(icon).to_string()),
                "{icon:?} after install"
            );
        }
    }
}
