# Changelog

Notable changes to MacroMagi. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

File-format changes are called out explicitly; see [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md) for the compatibility rules.

## 0.1.2 – Unreleased

### Added

- **Magic tab**, beside Auto Clicker and Macro, for small vision AI models that macros use.
  - **Import** an ONNX model, or a project folder or ZIP that contains one. The importer reads the project's files – it never runs them – for the class names, which classes call for action, the colour normalisation, the screen area, the key and the adjustable settings, and lists what it found and what it guessed.
  - **Configure** the name and description; the screen area (a square in the middle of a display that scales with it, or a fixed area picked on screen); each answer's name, whether it acts, how sure the model must be and how long to wait; the key, how long it is held and the cooldown; adjustable settings that move several values at once; how pictures are prepared; and the pictures to keep for training and fine-tuning.
  - **Try it**: watch the screen live and see what the model sees and how sure it is of each answer, with or without pressing the key, or try it on a picture file.
  - **Save** `.magic` files: the model, its setup, ready-made blocks and its pictures, each optional.
  - **Use in Macro** adds the model to the open macro with blocks that run it. Using it again updates the model and the blocks that use it.
- **Models made for the DBD Auto Skill Check project** run as they do there: importing its repository or its packaged app (folder or ZIP) finds its ten answers, the five that act, the 51% confidence, the 5 ms frontier delay and its screen area, with sliders for the three settings it offers.
- **AI blocks**: **AI Watch** keeps looking and presses the model's key when it should (or only watches), for a time or until stopped, as often as allowed; **AI Model** looks once and gives the answer, how sure the model is, and whether to act. Each block can change the model's adjustable settings for itself, load a `.magic` file, or open its model in the Magic tab.
- **Settings → AI**: run models **automatically** (each way is measured on the model and the fastest kept; the CPU when a graphics card is not faster), on the **CPU**, or on a chosen **graphics card**.
- **Downloads** on the AI settings page (also *Options → Downloads…*): ONNX Runtime (faster on the CPU), ONNX Runtime for graphics cards (DirectX 12, Windows), Python and PyTorch, each with its size, Install and Remove. Copies already on the computer are detected.
- **Python**: the Windows package now comes with Python. *Settings → Engine* lists every Python found, with its version, in a drop-down, and offers *Download Python* when there is none.
- **Image Detection** looks for several images in one block, in order of priority. Images can be put in groups; an image or a group can have its own search area, otherwise the block's is used. *Found when* any or every image is there; **Detail** (Full, Half, A third, A quarter) trades accuracy for speed. New outputs: *Which image*, *Its group*, *How many*. The block shows up to four of its images, and Find (Ctrl+F) finds blocks by the names of their images and groups.
- **Sound Recognition** listens for several sounds in one block, in groups that can have their own accuracy; *Found when* any or every sound has been heard; *Recognise from the first* recognises long sounds while they are still playing. New outputs: *Which sound*, *Its group*, *How many*.
- **Microphone choice** for Sound Detection and Sound Recognition; the system's default when none is chosen.
- **Clear** button in the console.
- Variables can keep their value from one run to the next: *Reset at the start of each run* in the Variables panel. New variables keep their value and start empty; a kept value can be emptied from the panel.
- Blocks that need the Python engine show a warning sign when it is not available; it explains why and opens *Settings → Engine*. The native engine skips them, with a warning in the console, instead of stopping.
- A `.magic` file opened with MacroMagi, or named on the command line, opens in the Magic tab.

### Changed

- The screen picker's display bar can be dragged, opens under the pointer, and stays inside the display.
- *Strict hotkey modifiers* and *Stop on task switch* are under *Behaviours* in the Options menu.
- The Log block's message is empty by default, so it logs the value wired into it.
- The banner logo takes the theme colour, like the buttons.
- Sound recognition is cheaper on long sounds: what has been heard is analysed once, not on every look.

### Fixed

- The Presets menu became very wide once it held a preset.

### File format

All additions are optional; files from 0.1.0 and 0.1.1 load unchanged. New `.magic` format for AI models. In `.magi` files: `model` assets; Image Detection has `images`, `groups`, `mode` and `detail` (the single `image` of older files is still read); Sound Recognition has `sounds`, `groups`, `mode` and `match_ms` (likewise for `sound`); sound blocks have `device`; variables have `reset` (`true` when missing). Presets may embed models. See [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md).

### Known limitations

- Training and fine-tuning are not in the Magic tab yet. Pictures can already be collected into `.magic` files by class, and PyTorch can be downloaded for it.
- Graphics cards are used on Windows only; elsewhere models run on the CPU.
- Image Detection runs on the CPU, where its search is fastest.
- A model keeps the licence of the project it came from; the Magic tab shows the licence an import found. Models imported from DBD Auto Skill Check are GPL-3.0, so `.magic` files and macros that contain one carry those terms when shared.

## 0.1.1

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
