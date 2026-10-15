# Working on MacroMagi – guide for AI agents

This file is for AI coding agents (Claude, Codex, Copilot and others) and for the people directing them. It says how to make changes that fit the project, and keeps a log of the revisions agents have made. Human contributors will find the same rules, at more length, in [CONTRIBUTING.md](CONTRIBUTING.md).

Read these before changing anything substantial:

1. [README.md](README.md) – what the app does, from the user's side.
2. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) – how it is built and why.
3. [CONTRIBUTING.md](CONTRIBUTING.md) – commands, code guidelines, testing.
4. [TERMS.md](TERMS.md) – the project is source available, not open source.

## Ground rules

- **Layers point down.** `magi-app` → `magi-engine` / `magi-platform` → `magi-core`. `magi-core` has no OS or UI code; only `magi-platform` talks to the operating system; only `magi-app` draws.
- **Blocks are data.** A block's ports, settings and documentation are a `NodeDef` in `magi-core/src/nodes/`. Code that is not about one specific block must not name one.
- **Two engines, one behaviour.** Every block has a native implementation (`magi-engine/src/native.rs`) and a Python one (`runtime/python/magi/nodes/`). Anything touching devices goes in `magi-engine/src/ops.rs`, shared by both. Every behaviour change gets a scenario in `crates/magi-engine/tests/engines.rs` that runs on both engines.
- **Portable.** Write only inside the data folder (`Paths::data_dir`). No registry, no user-profile files, no services. The only exception is a desktop shortcut the user explicitly asks for.
- **No network** except downloading a background picture from an address the user typed. No telemetry, no update checks.
- **Files keep loading.** `.magi` and `.mp` changes are additive (new optional fields with defaults). Old files must still load – add a test with an old file's JSON. Document the change in [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md).
- **Release builds are single files.** Think twice before adding a dependency; check its licence and size.

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
| A new kind of block setting | A `ParamKind` variant in `magi-core`, one editor in `magi-app/src/ui/inspector.rs`, a name in `tools/node_docs.py`. |
| Something a macro can do to the machine | A method on `Host` (with a default), `SystemHost` and `SimHost`, then an operation in `ops.rs`. |
| An Auto Clicker feature | `ClickerSettings` in `magi-core/src/autoclicker.rs` (with a serde default), the loop in `ops.rs` (`autoclick`), the form in `magi-app/src/ui/clicker_form.rs`. The tab and the Auto Click block share all three. |
| A rule about when input may be sent | `AutomationOptions` in `magi-core/src/options.rs`; enforcement in the controller (hotkeys) or `RunCx::send` (input). UI in `magi-app/src/ui/options.rs`. |
| A setting of the app | `Settings` in `magi-app/src/settings.rs` (with a default; old settings files must load), a control on a page of `ui/settings_window.rs`. |
| A hotkey | A `HotkeyTarget`, a binding in `controller.rs` (`State::rebind`), a row on the Hotkeys settings page, and a default in `Settings::reset_hotkeys`. |
| Colours or styling | `magi-app/src/theme.rs` only. UI code asks for colours by role (`theme::text()`, `theme::accent()`) – never write a colour literal in a UI module, or it will ignore the user's theme. |

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
| 2026-10-14 | Claude (Opus 5.5) | **Second update.** Themes: monochrome default, theme colour that recolours everything, Light/Dark/System, background pictures (GIF, WebP, from files or web addresses) with opacity, blur, darkness and panel opacity. Hotkeys: Windows hook watchdog and Raw Input, any key or chord (Escape, F-keys, Ctrl+Alt+Y, mouse buttons), Run hotkey, extra stop hotkeys, stop on Alt+Tab/Win+Tab, strict modifiers, all hotkeys and resets in Settings. Auto Clicker: rate or interval in ms–days, variation, duty cycle, course correction, 1000 cps mode with warning and hold clamping, dual keys, click and time limits, corner and edge stops, separate Start/Stop, undo/redo; the clicker is now the Auto Click block. Options menu, program whitelist/blacklist with stop/pause, desktop shortcut. Macro: banner menus (File, Options, Presets, Add, View, undo/redo), fixed block list (opening, sizing, keyboard), Switch to Window block, smart recording for games, spreadsheet recording editor with trim handles and pop-out window, View → Screen targets, Run/Record hotkey warnings, Export dialog with presets/options/clicker. Resizable settings window, debounced interface size. Lower memory use (idle Python shut down, memory trimmed while hidden). Release workflow publishes only when every build succeeds; CI test workflow removed. TERMS.md, AGENTS.md. |
| 2026-10-13 | Claude | **Initial program.** Cargo workspace (core, vision, platform, engine, app); egui app with Auto Clicker and node-graph Macro tabs; Python and native engines with shared device operations and tests; recording with press/release timing; Image and Color Detection; `.magi` and `.mp` formats; portable data folder; documentation. |
