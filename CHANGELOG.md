# Changelog

Notable changes to MacroMagi. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

File-format changes are called out explicitly; see [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md) for the compatibility rules.

## Unreleased

The first version. Everything is new.

### Added

- **Auto Clicker** tab: mouse button, key or two keys (together or alternating); single/double/triple clicks with an adjustable gap; speed as clicks per second/minute/hour/day or as an interval from milliseconds to 365 days; speed variation; fixed hold time or duty cycle; course correction (on by default); high-speed mode up to 1000 clicks per second, with a warning and automatic hold shortening; click limit, time limit, and corner and edge stops; click at the pointer or a picked position; toggle or hold-to-run hotkey; separate Start and Stop buttons; undo/redo; presets; "Open as macro".
- **Macro** tab: a node graph editor with pan and zoom, box and multi-selection, copy/paste (carrying images and recordings), duplicate, disable, undo/redo, a searchable, keyboard-driven and resizable block list, a settings panel generated from block definitions, a console, and three built-in examples.
- **Banner menus** per tab: File (both tabs), Options, Presets, Open as macro or Add and View, undo/redo; buttons for settings, keep-on-top and hide-from-screen-captures.
- **Blocks**: Start, Wait, Repeat, Decision, Wait Until, Stop, Log, Input, Auto Click, Play Recording, Image Detection, Color Detection, Switch to Window, Logic, Random Chance, Python Script.
- **Switch to Window**: bring a program to the front by title or program name, aware of windowed, borderless and exclusive fullscreen, with a "Not found" branch.
- **View → Screen targets**: every click position and search area shown over a frozen screenshot, numbered in run order, draggable and resizable.
- **Recording** of keyboard, mouse buttons, mouse movement, scroll wheel and gamepad input, stored as press time plus hold duration per input. **Smart recording** detects games that lock the pointer and records relative motion; Play Recording can replay movement as recorded, as screen positions or as relative motion; older recordings convert from the block's context menu.
- **Recording editor**: a spreadsheet of every input with every field editable for every kind of input, changeable row types, add/duplicate/delete, a timeline with trim handles, speed changes, and a pop-out window.
- **Image Detection** by colour-aware template matching with a coarse-to-fine search, an adjustable accuracy threshold (default 80%), an optional search area, and a screen picker for capturing images, areas, points and colours.
- **Two engines** with identical behaviour: Python (standard library only, auto-detected, with Python Script blocks and a `magi` scripting API) and native Rust.
- **Hotkeys** from any keyboard key (Escape and function keys included), mouse button or gamepad button, alone or combined; Auto Clicker, Run, Record, stop-everything and any number of extra stop hotkeys; all listed in Settings with conflict warnings and a reset button. Run and Record show their hotkeys and ask before starting when theirs has been removed.
- **Options** menu: strict hotkey modifiers, stop on task switch (Alt+Tab, Win+Tab), auto-stop hotkeys, program rules (whitelist or blacklist with search and per-program stop or pause), stopping the Auto Clicker while a hotkey is edited, dry run, and creating a desktop shortcut.
- **Appearance**: monochrome by default; a theme colour that recolours the whole interface; Light, Dark or System theme (System by default); a background picture or animated GIF/WebP from a file or web address, with opacity, blur, darkness and panel opacity.
- **Settings window**: movable and resizable, in pages (General, Appearance, Hotkeys, Recording, Engine), with "Reset all hotkeys" and "Reset all settings".
- **Export** dialog: choose to include presets, Options and Auto Clicker settings in a `.magi` file; files that carry them offer to apply them on opening.
- **Safety**: keys and buttons held by a macro are released when it stops; dry-run mode; on Windows, input sent by MacroMagi never triggers its own hotkeys or ends up in its own recordings.
- **Memory**: an idle Python process is stopped after a few minutes, and memory is handed back to the system while the window is minimised.
- **Files**: the `.magi` project format (a ZIP with a readable `project.json` and linked assets) and the `.mp` preset format (JSON), both at format version 1. *Format:* projects may carry `options` and `clicker`; recordings may contain `look` strokes and clicks with `here`; Auto Clicker settings gained the fields listed in FILE_FORMAT.md. All additions are optional; earlier files load unchanged.
- **Command line**: `run` (with `--engine` and `--dry-run`), `nodes`, `example`.
- **Releases**: a GitHub Actions workflow builds Windows, Linux and macOS packages and publishes a release only when every build succeeds.
- **Terms**: MacroMagi is source available under [TERMS.md](TERMS.md). [AGENTS.md](AGENTS.md) guides AI coding agents and logs their revisions.

### Fixed

- Hotkeys that never took effect, or only after a restart: Windows silently removes slow keyboard hooks, which left only mouse hotkeys working. The listener now runs at high priority and a watchdog reinstalls the hooks. Any key and combination can be assigned and works immediately.
- The *Add* button and the *Add block* context-menu entry opened the block list and closed it again at once.
- The first entry of the block list was drawn as tall as the whole list.
- Typing a percentage into *Interface size* set the wrong size; the size now changes only after the slider is released.
- *Trim start* in the recording editor appeared to do nothing (recordings already start at the first input); trimming is now done with handles on the timeline.
- Clicks landing on the Auto Clicker's start button no longer stop it.

### Known limitations

- Linux and macOS builds are implemented but untested. Switch to Window, program rules and smart recording are Windows-only for now.
- Gamepad input can trigger macros and be recorded, but cannot be sent.
- Whole-screen searches, the screen picker and the screen-targets view use the primary monitor only.
