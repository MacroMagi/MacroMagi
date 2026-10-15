# Changelog

Notable changes to MacroMagi. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

File-format changes are called out explicitly; see [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md) for the compatibility rules.

## 0.1.1 – Unreleased

### Added

- **Variables.** A macro has named variables with starting values. Any block setting can follow a variable through the link button beside it, so several blocks share one value and change together. New blocks: **Variable** (read one), **Set Variable** (set, add, subtract, multiply, divide, append – from the setting or from a wired value) and **Counter** (count passes, with a limit and a *Reached* output). A **Variables** panel lists them with their starting values and their values in the latest run. Python scripts read and change them through `magi.vars`, and a Python Script block can store its result in one.
- **Logic for values**: **Math** (add, subtract, multiply, divide, remainder, power, min, max, rounding, random…), **Compare** (equal, less, greater, contains, starts with, ends with) and **Text** (join, length, upper/lower case, trim, replace, read as a number).
- **Rate** block: a list of inputs done one after another, a set time apart. The time can follow a variable or be wired in from another block. **Variable Rate Editor** window for its timing and steps.
- **Definition** and **Function** blocks: build a piece of a macro once under a Definition and run it from any number of Function blocks, passing a value in through a variable.
- **Repeat until / while** a condition, checked before every round.
- **Stop** can now end only the current round of a loop, leave the loop, or continue from another chosen block.
- **Clipboard** block (the text on the clipboard) and **Notification** block (a sound and a desktop notification). *Options → Notify when a macro finishes* does the same whenever a macro ends by itself.
- **Sound Detection** (loud enough, optionally within a pitch range) and **Sound Recognition** (an uploaded WAV, MP3, OGG or FLAC sound, recognised with an adjustable accuracy at any volume), listening to what the computer plays or to the microphone.
- **Comments**: notes on the canvas that fold up and resize by their corner, in any colour.
- **Groups**: put blocks in a named, coloured box (Ctrl+G); drag the box to move them together, and turn the whole group off to skip every block in it.
- **Find** (Ctrl+F): search blocks by name, custom name, type or settings, and groups by name.
- **Several displays**: every click position and search area remembers the display it is on, chosen in its settings; Image and Color Detection search a chosen display; the screen picker and Screen targets open on the display MacroMagi is on and switch between displays; recordings note when the pointer moves to another display.
- **Recording Editor** window: drag rows by their grip to reorder them, **Cut** (remove the part between the handles and close the gap) beside **Trim**, and rows for key down/up, button down/up and display changes. Recordings use *down* and *up* rows only when other inputs happen while a key or button is held; otherwise one row with its hold time.
- **Recording options** in a menu beside Record: what to record and how to record movement (Smart Recording by default).
- **Reset buttons** beside every block setting and every Auto Clicker value that differs from its default.
- **View** menu: show or hide the grid, the block activity glow and the variables.
- The block list shows every block. When a wire is dropped on the canvas, the blocks it can connect to come first and the others after them.
- A fourth example, *Variables, functions and groups*.
- A new icon, used for the window, the executable and desktop shortcuts.

### Changed

- **Auto Clicker**: the units after "per" read second, minute, hour, day; Variation sits under High-speed mode; the speed-limit note says to turn on High-speed mode to go faster; Two keys alternate by default, with *Alternating* first; Key and Two keys hide the settings that only matter for the mouse.
- A Play Recording block's information card shows what the recording holds and a **Recording Editor** button; the block's settings show the timeline, handles, Trim, Cut, speed and Smart, without the rows.
- **Screen targets** no longer darkens the screen, redraws without flickering, and a double-click on a target or its line in the list shows its block in the editor.
- The Options menu is narrower; the console, the block settings panel, the settings window and the program rules window keep the size you give them.
- Number settings with a wide range (such as loudness or pitch) change at a sensible pace when dragged.

### Fixed

- Hotkeys could not be assigned, and did not fire, while MacroMagi's own window was in front. The window library's Raw Input keyboard registration made Windows skip MacroMagi's keyboard hook; MacroMagi now takes Raw Input over itself.
- Panels and windows grew wider by themselves.
- The screen picker and Screen targets could crash on very large displays.

### File format

All additions are optional; files from 0.1.0 load unchanged. Projects may have `variables` and graph `groups`; any setting may be a variable binding (`{"$var": "name"}`); positions and areas may have a `display`; recordings may contain `key_down`, `key_up`, `mouse_down`, `mouse_up` and `display` strokes; assets may be `audio` (WAV). See [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md).

### Known limitations

- Listening to what the computer plays needs a loopback or monitor input on Linux and macOS (Windows has one built in).
- Switch to Window, program rules and smart recording are still Windows-only; gamepad input still cannot be sent.

## 0.1.0

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
- Whole-screen searches, the screen picker and the screen-targets view use the primary monitor only (lifted in 0.1.1).
