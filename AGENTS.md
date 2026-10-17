# Working on MacroMagi – guide for AI agents

This file is for AI coding agents (Claude, Codex, Copilot and others) and for the people directing them. It says how to make changes that fit the project, and keeps a log of the revisions agents have made. Human contributors will find the same rules, at more length, in [CONTRIBUTING.md](CONTRIBUTING.md).

Read these before changing anything substantial:

1. [README.md](README.md) – what the app does, from the user's side.
2. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) – how it is built and why.
3. [CONTRIBUTING.md](CONTRIBUTING.md) – commands, code guidelines, testing.
4. [TERMS.md](TERMS.md) – the project is source available, not open source.

## Ground rules

- **Layers point down.** `magi-app` → `magi-engine` / `magi-platform` → `magi-core`; `magi-engine` also uses the pure-computation crates `magi-vision` (pictures), `magi-audio` (sound) and `magi-ml` (AI models), and `magi-app` uses `magi-ml` for the Magic tab. `magi-core` has no OS or UI code; only `magi-platform` talks to the operating system; only `magi-app` draws.
- **Blocks are data.** A block's ports, settings and documentation are a `NodeDef` in `magi-core/src/nodes/`. Code that is not about one specific block must not name one.
- **Two engines, one behaviour.** Every block has a native implementation (`magi-engine/src/native.rs`) and a Python one (`runtime/python/magi/nodes/`). Anything touching devices goes in `magi-engine/src/ops.rs`, shared by both. Every behaviour change gets a scenario in `crates/magi-engine/tests/engines.rs` that runs on both engines.
- **Portable.** Write only inside the data folder (`Paths::data_dir`). No registry, no user-profile files, no services. The only exception is a desktop shortcut the user explicitly asks for.
- **No network** except downloading a background picture from an address the user typed, and an optional part the user installs from *Settings → AI → Downloads*, fetched from its publisher. No telemetry, no update checks.
- **Files keep loading.** `.magi` and `.mp` changes are additive (new optional fields with defaults). Old files must still load – add a test with an old file's JSON. Document the change in [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md).
- **Release builds are single files.** The executable needs nothing beside it; the Windows package adds Python in a `python` folder, used when present. Anything large or optional is a download (`magi-app/src/components.rs`) loaded at run time, never linked. Think twice before adding a dependency; check its licence and size.
- **Clean room for other projects.** Other projects may be read, and run, to learn facts – formats, input shapes, numbers. Never copy their code into MacroMagi, and never commit their models or data: those are the user's files, loaded at run time.

## Commands

```bash
cargo run -p magi-app                                   # run the app (data in target/debug/data)
cargo test --workspace                                  # all tests; must pass
cargo clippy --workspace --all-targets                  # must be warning-free
cargo fmt --all                                         # rustfmt decides formatting
cargo test -p magi-app system_tests -- --ignored --test-threads=1        # real-machine hotkey tests
cargo test -p magi-platform --test system -- --ignored --test-threads=1  # real-machine platform tests
cargo run -p magi-app -- nodes | python tools/node_docs.py > docs/NODES.md   # after changing a block
```

Set `MACROMAGI_DATA_DIR` to a scratch folder when running the app for experiments or screenshots, so the user's own settings are untouched. Screenshot commands are in CONTRIBUTING.md; they only ever capture MacroMagi's own window.

Ignored tests press F13/F14 on the real machine (keys with no default action) and run the controller in dry-run mode. Do not run them while the user is typing, and never add a default-run test that sends real input.

## How to make common changes

| Change | Where |
| --- | --- |
| A new block | Definition in `magi-core/src/nodes/<category>.rs`, native code in `native.rs`, Python in `runtime/python/magi/nodes/`, an engine scenario, regenerate `docs/NODES.md`. Step by step: [docs/ADDING_A_NODE.md](docs/ADDING_A_NODE.md). |
| A new kind of block setting | A `ParamKind` variant in `magi-core` (and in `ParamKind::bindable` if it can follow a variable), one editor in `magi-app/src/ui/inspector.rs`, a name in `tools/node_docs.py`. |
| Anything with screen positions | Store them display-relative (`Point::on`, `Rect.display`) and resolve them with `Displays` just before use. Pickers and overlays work per display (`magi-app/src/displays.rs`). |
| Something a macro can do to the machine | A method on `Host` (with a default), `SystemHost` and `SimHost`, then an operation in `ops.rs`. |
| An Auto Clicker feature | `ClickerSettings` in `magi-core/src/autoclicker.rs` (with a serde default), the loop in `ops.rs` (`autoclick`), the form in `magi-app/src/ui/clicker_form.rs`. The tab and the Auto Click block share all three. |
| A rule about when input may be sent | `AutomationOptions` in `magi-core/src/options.rs`; enforcement in the controller (hotkeys) or `RunCx::send` (input). UI in `magi-app/src/ui/options.rs`. |
| A setting of the app | `Settings` in `magi-app/src/settings.rs` (with a default; old settings files must load), a control on a page of `ui/settings_window.rs`. |
| A hotkey | A `HotkeyTarget`, a binding in `controller.rs` (`State::rebind`), a row on the Hotkeys settings page, and a default in `Settings::reset_hotkeys`. |
| An AI feature | Running models: `magi-ml`. The `.magic` description: `magi-core/src/magic.rs` (with serde defaults, like every format). What the AI blocks do: `ai_look`/`ai_watch` in `ops.rs`. The tab: `magi-app/src/ui/magic_tab.rs` and `magi-app/src/magic/`. |
| Something to download | A `Component` in `magi-app/src/components.rs`: its name, description, size, folder, how it is installed, detected and removed. It appears on the AI settings page by itself. |
| Colors or styling | `magi-app/src/theme.rs` only. UI code asks for colors by role (`theme::text()`, `theme::accent()`) – never write a color literal in a UI module, or it will ignore the user's theme. |

UI modules edit the data they are handed and return a `UiRequest` (or their own event type) for anything else – picking a point on screen, listening for a hotkey, opening a window. They never reach into `MagiApp`.

## Style

- **Comments explain why**, not what. Every module starts with a doc comment saying what it is for.
- **User-facing text is plain, specific and short.** Say what happened and what to do. No marketing, no jargon, and no boilerplate about where files are stored or what is left behind – a professional tool does not narrate that.
- **Documentation reads like a finished GitHub project**: headings, tables, commands that work as written. Update the README, CHANGELOG and docs in the same change as the code.
- Name tests as statements of fact (`hotkeys_set_in_the_ui_take_effect_at_once`). Test through public interfaces. UI flows can be driven headlessly with `egui_kittest` – see the tests at the bottom of `app.rs` and `ui/graph.rs`.

## Before you finish

- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets` and `cargo test --workspace` are clean.
- [ ] Both engines behave the same, with a shared scenario for anything new.
- [ ] Old `.magi`, `.mp` and settings files still load.
- [ ] README, CHANGELOG (*Unreleased*), docs and `docs/NODES.md` are up to date.
- [ ] Nothing is committed, tagged or pushed unless the user asked for it.
- [ ] A line is added to the revision log below.

## Revision log

Newest first. One entry per piece of work: the date, who made it, and what changed. Keep entries short – the commit history has the details.

| Date | By | Revision |
| --- | --- | --- |
| 2026-10-17 | Claude (Opus 5.5) | **0.1.3 fixes.** Links open the browser (eframe `links` feature). The bin next to an image or sound deletes it from the macro when no other block uses it. Repository links point to github.com/MagiGun/MacroMagi. Release build made. |
| 2026-10-17 | Claude (Opus 5.5) | **0.1.3.** New blocks Auto Typer (speed, variation, punctuation pauses, typos, trim/skip/limit, line breaks) and Text Scan (Windows OCR via `Host::read_text`, find, position, first number). Nested groups (`parent`, one direct group per block, old files converted), group selection, Select everything in it, Move the selection into, Take out one level. AI Watch is a loop with an `acted` output (fixes outputs never reaching other blocks). Keys sent as scan codes on Windows. Image/sound lists: groups and items in one list with drag and drop. Searchable key pickers with Press a key… and mouse buttons (Input rows, model key). Settings: Engine page checks only on Check (or once on the first run; remembered), "(auto)" shows its interpreter, Python download/uninstall moved there; AI page without Python, Check again at the top, Uninstall, progress bars; PyTorch into the macros' Python; Store Pythons found; no console windows. Lightweight mode, panel close buttons, About/credits (logo and Settings), Comment color picker, wrapping button rows, interface-size reset arrow, swatch margin, "color" spelling, "→" replaced (missing from the font). Release workflow Windows only. Docs and README updated. |
| 2026-10-15 | Claude (Opus 5.5) | **0.1.2.** Magic tab for small vision AI models: import an ONNX model or a project folder/ZIP (reads class tables and code dictionaries, act flags, normalisation, screen area, key, and settings from `settings.json` or typed code declarations – never runs them), configure, try live, save `.magic` files (model, blocks, training/fine-tuning pictures, include toggles), Use in Macro. New crate `magi-ml` (tract built in; ONNX Runtime CPU/DirectML loaded at run time; Auto/CPU/GPU with benchmarking). AI Watch and AI Model blocks with per-block setting sliders. Downloads on the AI settings page (ONNX Runtime, its GPU build, Python, PyTorch: install, remove, detect). Python drop-down, Download Python, Python bundled in the Windows release. Image Detection: several images, groups, per-image/group areas, any/all, Detail. Sound Recognition: several sounds, groups, recognise long sounds while playing; microphone choice. Variables keep their value unless set to reset. Python-only blocks marked and skipped on the native engine. Screen picker bar draggable and near the pointer; Behaviours section; console Clear; empty Log default; logo in the theme color; narrow Presets menu. Docs and README updated. Verified on the DBD Auto Skill Check repo and app package (clean room: facts only). |
| 2026-10-15 | Claude (Opus 5.5) | **0.1.1.** New icon everywhere (window, exe, shortcut). Hotkeys can be assigned and fire while MacroMagi is focused (Raw Input keyboard registration no longer suppresses the hook), with a system test. Panels and windows keep their size; narrower Options menu. Several displays: positions and areas store their display, display pickers, per-display screen picker and Screen targets, display strokes in recordings. Screen targets: deferred viewport, no dimming, double-click shows the block. View: grid, block activity, variables, find. Auto Clicker: reset buttons, singular units, Variation under High-speed, clearer limit note, Two keys alternate first, mouse-only panels hidden for keys. Macro: every setting resettable and linkable to variables; Variables panel with live values; blocks Rate (+ Variable Rate Editor), Definition, Function, Variable, Set Variable, Counter, Math, Compare, Text, Clipboard, Notification, Comment, Sound Detection, Sound Recognition (new crate `magi-audio`); Repeat until/while; Stop next round/leave loop/go to; groups; block list shows everything, connectable first; recordings with down/up strokes when presses overlap; Recording Editor window with drag-to-reorder, Trim and Cut; recording options beside Record; notify when a macro finishes; Python scripts share variables. New example. Docs and README updated. |
| 2026-10-14 | Claude (Opus 5.5) | **Second update.** Themes: monochrome default, theme color that recolors everything, Light/Dark/System, background pictures (GIF, WebP, from files or web addresses) with opacity, blur, darkness and panel opacity. Hotkeys: Windows hook watchdog and Raw Input, any key or chord (Escape, F-keys, Ctrl+Alt+Y, mouse buttons), Run hotkey, extra stop hotkeys, stop on Alt+Tab/Win+Tab, strict modifiers, all hotkeys and resets in Settings. Auto Clicker: rate or interval in ms–days, variation, duty cycle, course correction, 1000 cps mode with warning and hold clamping, dual keys, click and time limits, corner and edge stops, separate Start/Stop, undo/redo; the clicker is now the Auto Click block. Options menu, program whitelist/blacklist with stop/pause, desktop shortcut. Macro: banner menus (File, Options, Presets, Add, View, undo/redo), fixed block list (opening, sizing, keyboard), Switch to Window block, smart recording for games, spreadsheet recording editor with trim handles and pop-out window, View → Screen targets, Run/Record hotkey warnings, Export dialog with presets/options/clicker. Resizable settings window, debounced interface size. Lower memory use (idle Python shut down, memory trimmed while hidden). Release workflow publishes only when every build succeeds; CI test workflow removed. TERMS.md, AGENTS.md. |
| 2026-10-13 | Claude | **Initial program.** Cargo workspace (core, vision, platform, engine, app); egui app with Auto Clicker and node-graph Macro tabs; Python and native engines with shared device operations and tests; recording with press/release timing; Image and Color Detection; `.magi` and `.mp` formats; portable data folder; documentation. |
