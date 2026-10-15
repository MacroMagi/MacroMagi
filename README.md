# MacroMagi

**An auto clicker when you want one click repeated, a visual macro editor when you want a bot.**

MacroMagi is a small, portable desktop tool for automating keyboard, mouse and controller input. Use the one-screen **Auto Clicker**, or switch to the **Macro** tab and wire up blocks that click, type, wait, loop, switch windows, replay recordings and react to what is on screen.

<p>
  <img src="docs/images/macro-editor.png" alt="The Macro tab: a node graph that watches for two buttons and reacts to each, with the settings of an Image Detection block open on the right" width="100%">
</p>
<p>
  <img src="docs/images/auto-clicker.png" alt="The Auto Clicker tab" width="31%">
  <img src="docs/images/recording.png" alt="A Play Recording block with its recording editor open, listing every recorded input" width="67%">
</p>

> **Status: early development (0.1.0, unreleased).** The features below work and are covered by tests, but things will change. Windows is the tested platform; see [Platform support](#platform-support).

## Contents

- [Features](#features)
- [Getting MacroMagi](#getting-macromagi)
- [Using it](#using-it)
  - [Auto Clicker](#auto-clicker)
  - [Macros](#macros)
  - [Recording](#recording)
  - [Reacting to the screen](#reacting-to-the-screen)
  - [Games and windows](#games-and-windows)
  - [Hotkeys](#hotkeys)
  - [Options](#options)
  - [Appearance](#appearance)
  - [Engines: Python and native](#engines-python-and-native)
  - [Command line](#command-line)
- [Files: `.magi` and `.mp`](#files-magi-and-mp)
- [Where MacroMagi keeps its data](#where-macromagi-keeps-its-data)
- [Platform support](#platform-support)
- [Building from source](#building-from-source)
- [Releases](#releases)
- [Project layout](#project-layout)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [Using MacroMagi responsibly](#using-macromagi-responsibly)
- [Terms](#terms)

## Features

**Auto Clicker**

- Click any mouse button, press any key, or press **two keys** – together or alternating.
- Set the speed as clicks per second, minute, hour or day, or as the time between clicks – from 1 ms to 365 days.
- Single, double or triple clicks with an adjustable gap; a fixed hold time or a **duty cycle** (hold for a share of each interval).
- **Speed variation** so the rhythm is not mechanical, and **course correction** that keeps the average speed exact even when the system is busy.
- **High-speed mode** for up to 1000 clicks per second, with a warning first; hold times are shortened automatically so those speeds are reachable.
- Stop after a number of clicks or a time limit, or when the pointer reaches a **screen corner or edge** you choose.
- Click at the pointer or at a position picked on screen. Start with a hotkey that toggles or works while held.
- Separate **Start** and **Stop** buttons, undo/redo, presets, and **Open as macro** to turn the settings into an Auto Click block.

**Macro editor**

- A node graph that reads like a flowchart: *flow goes down, values go across*.
- 16 blocks – Start, Wait, Repeat, Decision, Wait Until, Stop, Log, Input, Auto Click, Play Recording, Image Detection, Color Detection, Switch to Window, Logic, Random Chance and Python Script. See the [block reference](docs/NODES.md).
- Any number of **Start** blocks, each with its own hotkey, running side by side.
- **Decision** blocks with as many cases as you like; **Input** blocks that press several things at once (`Ctrl + C`, "hold W and click").
- Blocks light up as they run; errors point at the block that caused them.
- Multi-select, copy and paste (images and recordings travel along), duplicate, disable, undo and redo, "run from here", block presets.
- **View → Screen targets** shows every click position and search area on the real screen, numbered in run order, and lets you drag them into place.

**Recording**

- Records keyboard, mouse buttons, movement, scroll wheel and gamepad, with *when each input was pressed and released*.
- **Smart recording** notices games that lock the pointer (first-person cameras) and records how far the mouse moved instead of where the pointer was, so the camera turns on replay. Older recordings can be converted.
- A spreadsheet-style editor for every kind of input, with a timeline you **trim with handles**, speed changes, and a **pop-out window** for long recordings.

**Seeing the screen**

- **Image Detection** finds a picture on screen, with an accuracy from 0% to 100% (default 80%) and an optional search area.
- **Color Detection** finds a colour, with a tolerance.
- Both give a true/false result for Decisions and a position you can wire straight into a click.

**Control and safety**

- Hotkeys from any keyboard key, mouse button or gamepad button, alone or combined – `Escape`, `F6`, `Ctrl + Alt + Y`, `Mouse Back`.
- A stop-everything hotkey, extra stop hotkeys, and optionally stopping on **Alt+Tab / Win+Tab**.
- **Program rules**: a whitelist or blacklist of programs, each set to stop or pause automation when it is (or is not) in front.
- Every key a macro was holding is released when it stops. **Dry run** mode runs a macro without sending any input.

**Look and feel**

- Monochrome by default. Pick a **theme colour** and everything – icons, blocks, wires – follows it. Light, dark or follow the system.
- An optional **background picture or animated GIF/WebP**, from a file or a web address, with adjustable opacity, blur, darkness and panel opacity.
- A resizable settings window, adjustable interface size, always-on-top, and a one-click switch to hide MacroMagi from screen captures.

**Under the hood**

- Two interchangeable engines: **Python** (so macros can contain your own Python code) and **native Rust** (nothing to install). One test suite holds both to the same behaviour.
- Light on memory: an idle Python process is shut down, and memory is handed back to the system while the window is minimised.
- Portable: one executable and a `data` folder next to it.

## Getting MacroMagi

Download the package for your system from the [Releases](https://github.com/MacroMagi/MacroMagi/releases) page, unzip it anywhere and run `macromagi`. There is no installer. *Options → Create desktop shortcut* adds a shortcut if you want one.

| System | Package |
| --- | --- |
| Windows 10/11 (64-bit) | `MacroMagi-<version>-windows-x64.zip` |
| Linux (64-bit, X11) | `MacroMagi-<version>-linux-x64.tar.gz` |
| macOS (Apple silicon) | `MacroMagi-<version>-macos-arm64.tar.gz` |
| macOS (Intel) | `MacroMagi-<version>-macos-x64.tar.gz` |

Until the first version is tagged there are no packages yet – [build it from source](#building-from-source), which is one command once Rust is installed.

## Using it

The banner at the top holds everything for the open tab: **File**, **Options**, **Presets**, then **Open as macro** (Auto Clicker) or **Add** and **View** (Macro), and undo/redo. On the right are Settings, *keep on top*, and *hide from screen captures*.

### Auto Clicker

1. Choose what to click with, how fast, for how long and where.
2. Press **Start** or the hotkey (`F6` by default). Press **Stop** or the hotkey to stop.

The form is locked while clicking so a stray click cannot change it, and the start button ignores the clicks that land on it.

### Macros

A macro is a graph of **blocks** joined by **wires**.

- **Flow wires** (top to bottom) say what happens next. A block's flow input is on its top edge, its outputs on the bottom edge.
- **Value wires** (left to right) carry a result – true/false, a screen position, a number – from one block into another.

A first macro:

1. Open the **Macro** tab. A **Start** block is already there.
2. Press **Add** (or double-click the canvas) and add an **Input** block. Drag from the bottom of Start to the top of Input.
3. Select the Input block to choose what it does. Use **Add input** to make it do several things at once.
4. Press **Run** (`F7`).

Getting around the editor:

| To… | Do this |
| --- | --- |
| Add a block | **Add**, double-click the canvas, right-click → *Add block*, or drag a wire into empty space. Type to search, arrow keys and `Enter` to pick. |
| Connect | Drag from one port to another |
| Disconnect | Right-click a port, right-click a wire → *Delete wire*, or `Alt`+click a wire |
| Move around | Drag with the middle or right mouse button, or hold `Space` and drag |
| Zoom | Mouse wheel; `F` fits everything in view |
| Select several | Drag a box on the canvas; `Shift`/`Ctrl`+click adds; `Ctrl+A` selects all |
| Edit a block | Select it (wide window) or double-click it (small window) |
| Copy, cut, paste, duplicate | `Ctrl+C`, `Ctrl+X`, `Ctrl+V`, `Ctrl+D` |
| Delete | `Delete` or `Backspace` |
| Undo, redo | `Ctrl+Z`, `Ctrl+Y` (on both tabs) |
| Save, open, new, export | `Ctrl+S`, `Ctrl+O`, `Ctrl+N`, `Ctrl+E` |

*File → Examples* has three macros to take apart.

### Recording

Press **Record** (`F9`), do the thing, press it again. A **Play Recording** block appears with its recording open beside it: a timeline and a table listing every input – what it was, where, when it was pressed and how long it was held.

- Every column can be edited, for every kind of input: keys, clicks and drags, movement, mouse-look, scrolling, gamepad buttons and sticks. A row's type can be changed, and rows can be added, duplicated and removed.
- Drag the handles at either end of the timeline to choose the part to keep, then press **Trim**.
- **½× / 2× time** changes the speed; **Smart** converts game-camera movement to relative motion.
- The arrow button opens the recording in **its own window**, which you can make as large as you like.
- The record and stop hotkeys are never part of a recording. If the record hotkey has been removed, MacroMagi asks before recording, since the click on Stop would be recorded too.

> **A recording contains everything you type while it runs – including passwords.** Recordings are saved inside your `.magi` file in readable form. Check them before sharing a macro.

### Reacting to the screen

Add an **Image Detection** block, press *Capture from screen* and drag a box around the thing to look for. Then:

- wire **Found** into a **Decision** case (or a **Wait Until**) to branch on whether it is there;
- wire **Position** into an **Input** block to click it, wherever it shows up.

*Accuracy* sets how closely the screen must match: 100% means pixel-identical, 80% tolerates small rendering differences. Limiting the *search area* is faster and avoids matches elsewhere. Small, featureless images can match by accident – capture something distinctive.

**View → Screen targets** shows every position the macro clicks and every area it searches over a frozen picture of your screen, numbered in the order a run reaches them. Drag a point or an area to move it, or an area's corner to resize it.

MacroMagi keeps its own window out of screen captures by default, so it can neither hide what a macro is looking for nor end up in a captured image. Use the eye button in the banner to show it in screenshots and screen sharing again.

### Games and windows

- **Switch to Window** brings a program to the front before the next step – by window title or program name, picked from the running programs. It knows that exclusive-fullscreen games take longer to come back than windowed ones, or works it out itself.
- Games with a first-person camera keep moving the pointer back to the middle. **Smart recording** (the default, *Settings → Recording*) detects this and records relative motion; **Play Recording** can also be told to replay all movement as relative motion. Right-click a Play Recording block → *Convert to Smart Recording* fixes an older recording.

### Hotkeys

| Hotkey | Default | Active |
| --- | --- | --- |
| Start/stop the Auto Clicker | `F6` | Auto Clicker tab |
| Run/stop the macro | `F7` | Macro tab |
| A Start block | none | Macro tab |
| Stop everything | `F8` | Always |
| Start/stop recording | `F9` | Always |
| Extra stop hotkeys | none | Always |

All of them are listed in *Settings → Hotkeys*, where they can be changed or reset, and conflicts are pointed out. A hotkey can be any keyboard key – `Escape` and the function keys included – any mouse button, any gamepad button, or several held together (`Ctrl + Alt + Y`). Click a hotkey field and press the keys; the mouse button next to it picks a mouse button directly. Changes work immediately.

Hotkeys work while MacroMagi is minimised, and are *observed*, not swallowed: the key still reaches whatever program has focus.

### Options

The **Options** menu holds rules shared by the Auto Clicker and macros:

| Option | Does |
| --- | --- |
| Strict hotkey modifiers | On: a hotkey works only when exactly its modifier keys are held (`Ctrl+F6` never triggers `F6`). Off (default): extra held modifiers are ignored, so `F6` still works while you hold `Shift` in a game. |
| Stop on task switch | Stop everything on `Alt+Tab` or `Win+Tab`. |
| Auto-stop hotkeys | More key combinations that stop everything. |
| Program rules | A whitelist (input only reaches these programs) or blacklist (never reaches them), with search over the running programs. Each program can *stop* the clicker or macro, or *pause* it until an allowed program is in front again. Off by default. |
| Behaviours | *Stop the auto clicker when editing a hotkey* (on by default), so you can press the new hotkey. |
| Dry run | Run without sending any input; the console lists what would have been sent. |
| Create desktop shortcut | Puts a shortcut to MacroMagi on the desktop. |

Options can travel inside a `.magi` file (see [Files](#files-magi-and-mp)).

### Appearance

*Settings → Appearance*:

- **Theme**: System (default), Light or Dark.
- **Theme colour**: monochrome by default; a swatch or any colour recolours the whole interface, including icons, block categories and wires.
- **Background**: a PNG, JPEG, BMP, GIF or WebP – animated GIFs and WebPs play – chosen from a file or downloaded from a web address. Adjust how strongly it shows, its blur and darkness, and how opaque the panels over it are.
- **Interface size** (in *General*): drag the slider or type a percentage; the size changes once you let go.

### Engines: Python and native

Macros can run on either of two engines. They do the same thing – the same test suite runs every scenario on both – and you can switch at any time in Settings.

| | Python engine | Native engine |
| --- | --- | --- |
| Needs | Python 3.8 or newer | Nothing |
| Python Script blocks | Yes | No |
| Used when | Python is found (the *Automatic* setting) or chosen explicitly | Python is not found, or chosen explicitly |
| Auto Clicker | – | Always |

The Python engine needs **no packages** – only the standard library. It decides what runs next and runs your scripts; the actual clicking, typing and screen reading is done by MacroMagi itself, so both engines behave identically at the device level. An unused Python process is stopped after a few minutes and started again on the next run.

MacroMagi looks for Python in this order: the interpreter chosen in Settings, a `python` folder next to the executable, then `py -3` / `python` / `python3` on the system.

Inside a **Python Script** block you get a `magi` object:

```python
seen = magi.find_image("Accept button")      # an image of this macro, by name
if seen:
    magi.click(seen.x, seen.y)
else:
    magi.press("ctrl", "r")
    magi.sleep(500)
result = bool(seen)                          # goes out of the block's Result output
```

The full API is in [docs/PYTHON_ENGINE.md](docs/PYTHON_ENGINE.md).

### Command line

```text
macromagi [FILE.magi]              Open the editor, optionally with a macro
macromagi run FILE.magi [OPTIONS]  Run a macro without opening the window
macromagi nodes                    Print every node definition as JSON
macromagi example N FILE.magi      Save built-in example number N as a file

Options for run:
    --engine auto|python|native
    --dry-run                      Send nothing; print what would be sent
```

## Files: `.magi` and `.mp`

A **`.magi`** file is a macro. It is an ordinary ZIP archive – rename it to `.zip` to look inside:

```text
project.json        the macro: a readable text file with the blocks and wires
assets/….png        images linked from blocks
assets/….json       recordings linked from blocks
presets/….mp        presets bundled with the macro (optional)
```

**File → Export** saves a `.magi` and asks what else to put in it: your **presets**, your **Options** (stop hotkeys, program rules…) and your **Auto Clicker settings**. The defaults for that dialog are in *Settings → General*. When you open a file that carries Options or Auto Clicker settings, MacroMagi asks whether to use them.

A **`.mp`** file is a preset: a small JSON file holding either Auto Clicker settings or a group of configured blocks (with the images and recordings they use embedded). Presets live in `data/presets`.

Both formats are specified in [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md).

## Where MacroMagi keeps its data

Everything MacroMagi saves goes into a `data` folder next to the executable: `settings.json`, your `presets`, `backgrounds`, and the unpacked Python runtime. Set the `MACROMAGI_DATA_DIR` environment variable to use another folder. If the folder cannot be written to, MacroMagi still runs and says that settings will not be saved.

MacroMagi connects to the network only to download a background picture from an address you enter.

## Platform support

| | Status | Notes |
| --- | --- | --- |
| **Windows 10/11** | Tested | Primary platform. Everything works. |
| **Linux (X11)** | Implemented, not yet tested | Wayland sessions cannot be listened to (hotkeys, recording) or injected into by the current backends. |
| **macOS** | Implemented, not yet tested | Needs the *Accessibility* permission (input) and *Screen Recording* permission (image detection). |

"Implemented, not yet tested" means the code paths exist and use cross-platform libraries, but nobody has run them yet – expect rough edges, and please [report what you find](https://github.com/MacroMagi/MacroMagi/issues).

Windows only, for now: **Switch to Window**, **program rules** (elsewhere the program in front cannot be determined, so input is always allowed), and **smart recording** (elsewhere movement is recorded as screen positions).

Known limitations everywhere:

- **Sending gamepad input is not supported yet.** Gamepads can trigger macros and be recorded.
- Whole-screen searches, the screen picker and the screen-targets view use the primary monitor.
- On Windows, a program running as administrator ignores input from one that is not. To automate such a program, run MacroMagi as administrator too.
- Some games block synthetic input or forbid automation outright.

## Building from source

You need [Rust](https://rustup.rs). The repository pins the exact toolchain in `rust-toolchain.toml`; `rustup` installs it automatically on first build.

```bash
git clone https://github.com/MacroMagi/MacroMagi.git
cd MacroMagi
cargo build --release
```

The result is a single file: `target/release/macromagi` (`macromagi.exe` on Windows). Copy it anywhere and run it.

To run from the source tree while developing:

```bash
cargo run -p magi-app
```

**Linux** needs development headers for X11, Wayland, PipeWire and udev. On Debian or Ubuntu:

```bash
sudo apt install pkg-config libclang-dev libx11-dev libxi-dev libxtst-dev libxcb1-dev libxrandr-dev \
    libxkbcommon-dev libwayland-dev libegl-dev libdbus-1-dev libpipewire-0.3-dev libudev-dev
```

**macOS** needs the Xcode command line tools (`xcode-select --install`).

Python is **not** needed to build. It is only needed at run time if you want the Python engine, and for the part of the test suite that exercises it.

Tests:

```bash
cargo test --workspace
```

More in [CONTRIBUTING.md](CONTRIBUTING.md).

## Releases

Releases are built by GitHub Actions ([`.github/workflows/release.yml`](.github/workflows/release.yml)) when a version tag is pushed, or from *Actions → Release → Run workflow*:

```bash
git tag v0.2.0
git push origin v0.2.0
```

The workflow builds the packages for Windows, Linux and macOS (Apple silicon and Intel). Only when **every** build succeeds does it publish a GitHub release with the packages and their SHA-256 checksums; if one fails, nothing is published.

## Project layout

```text
crates/
  magi-core/      The data model: graphs, block definitions, recordings, options, file formats. No OS or UI code.
  magi-vision/    Image matching and colour search on raw pixels.
  magi-platform/  Everything that touches the OS: sending input, listening, gamepads, screen capture, windows.
  magi-engine/    Runs macros: the native interpreter and the bridge to Python.
  magi-app/       The desktop application (egui).
runtime/python/   The Python engine's source. Compiled into the app; unpacked on first use.
docs/             Architecture, file formats, block reference, guides.
tools/            Scripts that generate documentation.
```

Each layer only depends on the ones below it, and everything a block *is* (its ports, settings and documentation) is plain data in `magi-core`. That is what keeps features removable and is the groundwork for add-ons. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains the design and where to extend it.

## Roadmap

Not promises, but the direction:

- **Add-ons.** User-made blocks, written in Python, described by the same definition format the built-in blocks use.
- **A local vision model** block: answer "is there a … on screen?" from a screenshot, a reference image or a text prompt, without sending anything to a server.
- **Sending gamepad input.** This needs a virtual controller device, which on Windows means a driver – at odds with being portable. The plan is to use one if it is already installed.
- **More blocks**: variables and counters, maths and comparisons, clipboard, sounds and notifications.
- **Editor**: comment frames and groups, a minimap, aligning and tidying, highlighting the path a run took, searching large graphs.
- **Recording**: window-relative coordinates, rescaling to another screen size, merging and splitting recordings.
- **All monitors** in the screen picker, whole-screen searches and the screen-targets view.
- Switch to Window, program rules and smart recording on Linux and macOS.

## Contributing

Contributions are welcome – bug reports, platform testing (especially Linux and macOS), documentation and code. Start with [CONTRIBUTING.md](CONTRIBUTING.md); to add a block, follow [docs/ADDING_A_NODE.md](docs/ADDING_A_NODE.md). AI coding agents should read [AGENTS.md](AGENTS.md).

## Using MacroMagi responsibly

MacroMagi sends real input to your computer.

- Know your stop hotkey (`F8`) before you run a macro that loops. Try unfamiliar macros with **Dry run** first.
- A `.magi` file from someone else can contain a Python Script block, which is code that runs with your permissions. Read it before you run it, exactly as you would any script.
- Automation is against the rules of many online games and services. Whether you may use it is between you and them.

## Terms

MacroMagi is **source available**: you may read the code, build it yourself, modify it for your own use and contribute back – but you may not sell the program or its code, or distribute builds of it. The full terms, including how contributions are licensed and the code of conduct, are in [TERMS.md](TERMS.md).
