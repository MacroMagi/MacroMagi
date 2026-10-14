# Changelog

Notable changes to MacroMagi. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

File-format changes are called out explicitly; see [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md) for the compatibility rules.

## Unreleased

The first version. Everything is new.

### Added

- **Auto Clicker** tab: mouse button or key, single/double/triple clicks, interval with random variation, hold time, run forever or a number of times, click at the pointer or a picked position, toggle or hold-to-run hotkey, presets, and "Open as macro".
- **Macro** tab: a node graph editor with pan and zoom, box and multi-selection, copy/paste (carrying images and recordings), duplicate, disable, undo/redo, a searchable block list, a settings panel generated from block definitions, a console, and three built-in examples.
- **Blocks**: Start, Wait, Repeat, Decision, Wait Until, Stop, Log, Input, Play Recording, Image Detection, Color Detection, Logic, Random Chance, Python Script.
- **Recording** of keyboard, mouse buttons, mouse movement, scroll wheel and gamepad input, stored as press time plus hold duration per input, with an editable table and timeline attached to the Play Recording block.
- **Image Detection** by colour-aware template matching with a coarse-to-fine search, an adjustable accuracy threshold (default 80%), an optional search area, and a screen picker for capturing images, areas, points and colours.
- **Two engines** with identical behaviour: Python (standard library only, auto-detected, with Python Script blocks and a `magi` scripting API) and native Rust.
- **Hotkeys** from any keyboard key, mouse button or gamepad button, alone or combined; a stop-everything hotkey; a record hotkey. Hotkeys work while the window is minimised.
- **Safety**: keys and buttons held by a macro are released when it stops; dry-run mode; on Windows, input sent by MacroMagi never triggers its own hotkeys or ends up in its own recordings.
- **Files**: the `.magi` project format (a ZIP with a readable `project.json` and linked assets) and the `.mp` preset format (JSON), both at format version 1.
- **Command line**: `run` (with `--engine` and `--dry-run`), `nodes`, `example`.
- **Portable**: a single executable that keeps everything it writes in a `data` folder beside it.

### Known limitations

- Linux and macOS builds are implemented but untested.
- Gamepad input can trigger macros and be recorded, but cannot be sent.
- Whole-screen searches and the screen picker use the primary monitor only.
