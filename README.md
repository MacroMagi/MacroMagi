# MacroMagi

**An auto clicker when you want one click repeated, a visual macro editor when you want a bot.**

MacroMagi is a small, portable desktop tool for automating keyboard, mouse and controller input. Use the one-screen **Auto Clicker**, or switch to the **Macro** tab and wire up blocks that click, type, wait, loop, switch windows, replay recordings, keep variables and react to what is on screen – or to what it hears. The **Magic** tab prepares small AI models that recognise what is on screen, for macros to act on.

<p>
  <img src="docs/images/macro-editor.png" alt="The Macro tab: a node graph that watches for two buttons and reacts to each, with the settings of an Image Detection block open on the right" width="500">
</p>
<p>
  <img src="docs/images/auto-clicker.png" alt="The Auto Clicker tab" width="200">
  <img src="docs/images/recording.png" alt="A Play Recording block with its information card, and its timeline, Trim and Cut in the block settings" width="500">
</p>

> **Status: early development (0.1.3, unreleased).** The features below work and are covered by tests, but things will change. Windows is the tested platform; see [Platform support](#platform-support).

## Contents

- [Features](#features)
- [Getting MacroMagi](#getting-macromagi)
- [Using it](#using-it)
  - [Auto Clicker](#auto-clicker)
  - [Macros](#macros)
  - [Variables](#variables)
  - [Functions, rates and loops](#functions-rates-and-loops)
  - [Recording](#recording)
  - [Reacting to the screen](#reacting-to-the-screen)
  - [Reacting to sound](#reacting-to-sound)
  - [Magic: AI models](#magic-ai-models)
  - [Several displays](#several-displays)
  - [Games and windows](#games-and-windows)
  - [Hotkeys](#hotkeys)
  - [Options](#options)
  - [Appearance](#appearance)
  - [Engines: Python and native](#engines-python-and-native)
  - [Command line](#command-line)
- [Files: `.magi`, `.mp` and `.magic`](#files-magi-mp-and-magic)
- [Where MacroMagi keeps its data](#where-macromagi-keeps-its-data)
- [Platform support](#platform-support)
- [Building from source](#building-from-source)
- [Releases](#releases)
- [Project layout](#project-layout)
- [Roadmap](#roadmap)
- [Credits](#credits)
- [Contributing](#contributing)
- [Using MacroMagi responsibly](#using-macromagi-responsibly)
- [Terms](#terms)

## Features

**Auto Clicker**

- Click any mouse button, press any key, or press **two keys** – alternating or together.
- Set the speed as clicks per second, minute, hour or day, or as the time between clicks – from 1 ms to 365 days.
- Single, double or triple clicks with an adjustable gap; a fixed hold time or a **duty cycle** (hold for a share of each interval).
- **Course correction** keeps the average speed exact even when the system is busy; **speed variation** makes the rhythm less mechanical.
- **High-speed mode** for up to 1000 clicks per second, with a warning first; hold times are shortened automatically so those speeds are reachable.
- Stop after a number of clicks or a time limit, or when the pointer reaches a **screen corner or edge** you choose.
- Click at the pointer or at a position picked on any display. Start with a hotkey that toggles or works while held.
- Every value you change gets a **reset button** that puts it back to its default.
- Separate **Start** and **Stop** buttons, undo/redo, presets, and **Open as macro** to turn the settings into an Auto Click block.

**Macro editor**

- A node graph that reads like a flowchart: *flow goes down, values go across*.
- 34 blocks for flow (Start, Wait, Repeat, Decision, Wait Until, Stop, Rate, Definition, Function, Log), input (Input, Auto Click, Auto Typer, Play Recording), the screen (Image and Color Detection, Text Scan), sound (Sound Detection and Recognition), AI (AI Watch, AI Model), windows, variables (Variable, Set Variable, Counter), logic (Logic, Random Chance, Math, Compare, Text), the system (Clipboard, Notification), Python Script and Comment. See the [block reference](docs/NODES.md).
- **Variables** that any block setting can follow, a Variables panel with their live values, and Python scripts that read and change them. A variable can start fresh every run or keep its value from the last one.
- **Functions**: build a piece of a macro once under a Definition block and run it from anywhere.
- Any number of **Start** blocks, each with its own hotkey, running side by side.
- **Comments** that fold and resize, and **groups** – named, colored boxes that move together, can be turned off as a whole, and can hold other groups.
- **Find** (`Ctrl+F`) searches block names, settings, groups, and the names of images and their groups. Every setting has a **reset button** once it differs from its default.
- Blocks light up as they run; errors point at the block that caused them. **Lightweight mode** hides the editor and keeps only Run, Stop and the console.
- Multi-select, copy and paste (images, recordings, sounds and AI models travel along), duplicate, disable, undo and redo, "run from here", block presets.
- **View → Screen targets** shows every click position and search area on the real screen, numbered in run order, and lets you drag them into place.

**Recording**

- Records keyboard, mouse buttons, movement, scroll wheel and gamepad, with *when each input was pressed and released*. Keys held while other things happen are kept as separate *down* and *up* steps, so the order is exact.
- **Smart recording** notices games that lock the pointer (first-person cameras) and records how far the mouse moved instead of where the pointer was, so the camera turns on replay.
- A **Recording Editor** window: every input as a row you can edit and **drag to reorder**, and a timeline whose handles **Trim** (keep the part between them) or **Cut** (remove it).
- Recordings remember which **display** the pointer was on.

**Seeing and hearing**

- **Text Scan** reads the text in an area of the screen – what it says, whether a given word is there and where, and the first number – with the text recognition built into Windows.
- **Image Detection** finds pictures on screen – one or many per block, in groups with search areas of their own – with an accuracy from 0% to 100% (default 80%), and less detail when speed matters more. **Color Detection** finds a color, with a tolerance.
- **Sound Detection** reacts to sound that is loud enough, optionally within a pitch range. **Sound Recognition** reacts when it hears one of the sounds you uploaded – a notification, an alarm, a voice line – at any volume, and recognises long sounds while they play. Both listen to what the computer plays or to a microphone you choose.
- All give true/false results for Decisions; the screen blocks also give a position you can wire straight into a click, and say which image or sound it was.

**Magic: AI models**

- A **Magic** tab for small vision models: import an ONNX model or the project it comes in, configure what it looks at, what its answers mean and what to press, try it live on your screen, and save it as a `.magic` file.
- **AI Watch** and **AI Model** blocks run a model inside any macro, with its adjustable settings (confidence, delays) changeable per block.
- Models run on the CPU or a graphics card – automatically on whichever is faster for that model. Optional parts (ONNX Runtime, Python, PyTorch) are installed and removed from the settings.

**Control and safety**

- Hotkeys from any keyboard key, mouse button or gamepad button, alone or combined – `Escape`, `F6`, `Ctrl + Alt + Y`, `Mouse Back` – that work whether or not MacroMagi is in front.
- A stop-everything hotkey, extra stop hotkeys, and optionally stopping on **Alt+Tab / Win+Tab**.
- **Program rules**: a whitelist or blacklist of programs, each set to stop or pause automation when it is (or is not) in front.
- Every key a macro was holding is released when it stops. **Dry run** mode runs a macro without sending any input. A notification can tell you when a macro has finished.

**Look and feel**

- Monochrome by default. Pick a **theme color** and everything – icons, blocks, wires – follows it. Light, dark or follow the system.
- An optional **background picture or animated GIF/WebP**, from a file or a web address, with adjustable opacity, blur, darkness and panel opacity.
- Panels you can size as you like, a resizable settings window, adjustable interface size, always-on-top, and a one-click switch to hide MacroMagi from screen captures.

**Under the hood**

- Two interchangeable engines: **Python** (so macros can contain your own Python code; included in the Windows package) and **native Rust** (nothing to install). One test suite holds both to the same behaviour.
- Light on memory: an idle Python process is shut down, sound is only listened to while a macro needs it, and memory is handed back to the system while the window is minimised.
- Portable: one executable and a `data` folder next to it.

## Getting MacroMagi

Download the package for your system from the [Releases](https://github.com/MagiGun/MacroMagi/releases) page, unzip it anywhere and run `macromagi`. There is no installer. *Options → Create desktop shortcut* adds a shortcut if you want one. The Windows package has Python in a `python` folder beside the executable, for the Python engine; keep the two together.

| System | Package |
| --- | --- |
| Windows 10/11 (64-bit) | `MacroMagi-<version>-windows-x64.zip` |

Packages are built for Windows only for now; on Linux and macOS, [build it from source](#building-from-source). Until the first version is tagged there are no packages yet – [build it from source](#building-from-source), which is one command once Rust is installed.

**Windows warns about it.** MacroMagi is not yet signed with a code-signing certificate, so Windows SmartScreen shows *"Windows protected your PC"* the first time you run it. Choose *More info → Run anyway*. Check the file against the SHA-256 checksum published with the release if you want to be sure it is the one we built.

## Using it

The banner at the top holds everything for the open tab: **File**, **Options**, **Presets**, then **Open as macro** (Auto Clicker) or **Add** and **View** (Macro), and undo/redo. The Magic tab has its own toolbar under the banner. On the right are Settings, *keep on top*, and *hide from screen captures*.

### Auto Clicker

1. Choose what to click with, how fast, for how long and where.
2. Press **Start** or the hotkey (`F6` by default). Press **Stop** or the hotkey to stop.

The form is locked while clicking so a stray click cannot change it, and the start button ignores the clicks that land on it. A value you have changed shows a small ↺ button after it; click it to go back to the default. With **Key** or **Two keys**, the settings that only matter for the mouse are hidden.

Speeds above 100 clicks per second need **High-speed mode**, which asks before it turns on.

### Macros

A macro is a graph of **blocks** joined by **wires**.

- **Flow wires** (top to bottom) say what happens next. A block's flow input is on its top edge, its outputs on the bottom edge.
- **Value wires** (left to right) carry a result – true/false, a screen position, a number, text – from one block into another.

A first macro:

1. Open the **Macro** tab. A **Start** block is already there.
2. Press **Add** (or double-click the canvas) and add an **Input** block. Drag from the bottom of Start to the top of Input.
3. Select the Input block to choose what it does. Use **Add input** to make it do several things at once.
4. Press **Run** (`F7`).

Getting around the editor:

| To… | Do this |
| --- | --- |
| Add a block | **Add**, double-click the canvas, right-click → *Add block*, or drag a wire into empty space (the blocks it can connect to are listed first). Type to search, arrow keys and `Enter` to pick. |
| Connect | Drag from one port to another |
| Disconnect | Right-click a port, right-click a wire → *Delete wire*, or `Alt`+click a wire |
| Move around | Drag with the middle or right mouse button, or hold `Space` and drag |
| Zoom | Mouse wheel; `F` fits everything in view |
| Select several | Drag a box on the canvas; `Shift`/`Ctrl`+click adds; `Ctrl+A` selects all |
| Edit a block | Select it (wide window) or double-click it (small window) |
| Put back a setting | The ↺ button after it |
| Find a block or group | `Ctrl+F`, or the magnifying glass; `Enter` goes to the next match |
| Group blocks | Select them and press `Ctrl+G`. Grouping blocks of a group, or a group with other blocks, puts the new group inside. Click a group's title to select the group, drag it to move it; its switch turns every block in it off or on; right-click it to rename, recolor, select everything in it, move the selection into it or ungroup. *Take out of its group* (right-click a block) moves a block one level out. |
| Add a note | Add a **Comment** block. Its arrow folds it, its corner resizes it. |
| Copy, cut, paste, duplicate | `Ctrl+C`, `Ctrl+X`, `Ctrl+V`, `Ctrl+D` |
| Delete | `Delete` or `Backspace` |
| Undo, redo | `Ctrl+Z`, `Ctrl+Y` (on both tabs) |
| Save, open, new, export | `Ctrl+S`, `Ctrl+O`, `Ctrl+N`, `Ctrl+E` |

The **View** menu shows or hides the console, the block settings, the variables, the grid and the *block activity* glow.

*File → Examples* has four macros to take apart.

<p>
  <img src="docs/images/functions.png" alt="The Variables, functions and groups example: a Variables panel, a comment, a Function block calling a grouped Definition, and a Wait block whose duration follows the variable “pause”" width="100%">
</p>

### Variables

Variables are named values a macro keeps while it runs – a count, a delay, a piece of text.

- Open the **Variables** panel (the `{ }` button, or *View → Variables*) to add them. While a macro runs, the panel shows their current values.
- **Any setting of any block can follow a variable.** Click the link button beside the setting and pick a variable, or choose *New variable from this setting*. The setting then shows which variable it follows, and every block that follows the same variable changes with it.
- **Set Variable** changes a variable: set it, add to it, subtract, multiply, divide or append text – from its own setting or from whatever is wired into it (a Clipboard block, Math, a Python script…). **Variable** reads one into a value wire. **Counter** counts passes and can take a branch when it reaches a limit.
- **Math**, **Compare** (including *contains*, *starts with*, *ends with*) and **Text** work with the values.
- Python scripts read and change them as `magi.vars["name"]`, and a Python Script block can store its result in one.

A variable keeps its value from one run to the next – a running total, the last thing seen – for as long as MacroMagi is open; its ✕ empties it. Tick *Reset at the start of each run* to have every run start it at a value you set instead (unless another part of the macro is already running).

### Functions, rates and loops

- **Definition** starts a reusable piece of a macro; **Function** runs it and carries on. What is wired into a Function's *Input* is put into the definition's input variable first.
- **Rate** does a list of inputs one after another, a set time apart – a whole chain of Input and Wait blocks in one. The time can follow a variable or be wired in from another block, so it can change while the macro runs. The **Variable Rate Editor** button in its settings opens the timing and steps in a larger window.
- **Repeat** loops a number of times, until stopped, *until* a condition is true or *while* it is true.
- **Stop** ends the run or the whole macro – or only the current round of a loop, or the loop itself, or continues from a block you choose.

### Recording

Press **Record** (`F9`), do the thing, press it again. A **Play Recording** block appears, with an information card beside it. The arrow next to Record chooses what is recorded and how movement is recorded (*Smart Recording* by default).

The block's settings show the recording's **timeline**:

- Drag the handles at either end to choose a part, then **Trim** to keep only that part, or **Cut** to remove it and close the gap.
- The speed buttons make everything faster or slower; **Smart** converts game-camera movement to relative motion.

**Recording Editor** (on the card and in the settings) opens a window listing every input – what it was, where, when it was pressed and how long it was held:

- Every field can be edited, for every kind of input: keys, clicks and drags, movement, mouse-look, scrolling, gamepad buttons and sticks, and display changes. A row's type can be changed, and rows can be added, duplicated and removed.
- **Drag a row by its grip** to move it; it takes a time between its new neighbours, so the new order is also the order of playback.
- A key pressed and released with nothing in between is one row with a hold time. A key held *while* other things happen – Shift while typing a word – becomes a *Key down* and a *Key up* row, so each press keeps its place.

The record and stop hotkeys are never part of a recording. If the record hotkey has been removed, MacroMagi asks before recording, since the click on Stop would be recorded too.

> **A recording contains everything you type while it runs – including passwords.** Recordings are saved inside your `.magi` file in readable form. Check them before sharing a macro.

### Reacting to the screen

Add an **Image Detection** block, press *Capture from screen* and drag a box around the thing to look for – or add image files. Then:

- wire **Found** into a **Decision** case (or a **Wait Until**) to branch on whether it is there;
- wire **Position** into an **Input** block to click it, wherever it shows up.

*Accuracy* sets how closely the screen must match: 100% means pixel-identical, 80% tolerates small rendering differences. Limiting the *search area* is faster and avoids matches elsewhere. Small, featureless images can match by accident – capture something distinctive.

One block can look for **several images**. They are tried in list order and the first one found is reported – *Which image* and *Its group* say which. *Found when* can instead require every image to be there. Put images in **groups** to give several of them one search area, or give a single image its own; anything without an area uses the block's. The groups and their images are one list: add a group with the folder button, then drag an image by its handle onto it. **Detail** shrinks the screen and the images before comparing: *Half* is about four times faster, at some risk of missing small or fine images.

**View → Screen targets** shows every position the macro clicks and every area it searches over a picture of your screen, numbered in the order a run reaches them. Drag a point or an area to move it, or an area's corner to resize it. Double-click a target – on the screen or in the list – to show its block in the editor.

MacroMagi keeps its own window out of screen captures by default, so it can neither hide what a macro is looking for nor end up in a captured image. Use the eye button in the banner to show it in screenshots and screen sharing again.

### Reacting to sound

Both sound blocks listen to *what the computer plays* or to a *microphone* – the system's default, or one you choose – only while a macro is using them.

- **Sound Detection** is true while the sound is louder than a level you set (in dB: 0 is the loudest possible, ordinary sound is around −30, quiet sound around −60) – and, if you want, while its pitch is within a range. It also gives the loudness and the pitch as numbers.
- **Sound Recognition** listens for particular sounds. *Load sound file…* takes WAV, MP3, OGG or FLAC files (the first 10 seconds of each); they are stored inside the macro. *Found* becomes true when what is heard matches one at least as closely as the *accuracy* (75% by default), however loud it is played; *Which sound* says which. Each time a sound plays counts once. Sounds can be grouped, with an accuracy per group – drag a sound by its handle onto a group. For long sounds, *Recognise from the first* a second or two catches them while they are still playing. Short, distinctive sounds work best; lower the accuracy if one is missed, raise it if other sounds set it off.

Wire *Detected* or *Found* into a **Wait Until** to wait for a sound, or into a **Decision**.

### Magic: AI models

Some things on screen are too varied for Image Detection – a gauge that is never drawn the same way twice, an indicator over a moving picture. A small **vision model**, trained to tell such pictures apart, recognises them. The **Magic** tab prepares such models for macros; training them is planned for a later version.

1. **Import** an ONNX model (`.onnx`), or the folder or ZIP of the project it comes from. MacroMagi reads the project's files – it never runs them – for what the answers mean, which ones call for action, how pictures are prepared, the screen area and the key, and lists what it found and what it guessed.
2. **Check and adjust**: the screen area (a square in the middle of a display, which scales with the display, or a fixed area you pick), each answer's name, whether it acts, how sure the model must be and how long to wait, the key with its hold time and cooldown, and *adjustable settings* – sliders that move several values at once, offered again in every block that uses the model.
3. **Try it**: *Watch the screen* shows what the model sees and how sure it is of each answer, live. The key is only pressed when *Press the key when it acts* is on. *Picture…* tries a picture file.
4. **Save** a `.magic` file, and/or press **Use in Macro**: the model goes into the open macro with a Start and an **AI Watch** block ready to run. Using it again after a change updates the model in the macro.

**AI Watch** keeps looking and presses the model's key (or clicks its mouse button) when one of the acting answers is sure enough – or only watches. Each time it acts, the blocks under its *Acted* output run, with *Times acted* and *Last answer* up to date, and then it watches again; when its time is up it continues from *Done*. **AI Model** looks once and gives the answer, how sure the model is and whether to act. Each block's *Model settings* sliders change the model's adjustable settings for that block only; *Open in Magic* opens its model in the tab.

Projects made for **DBD Auto Skill Check** – its repository or its packaged app, as a folder or ZIP – import with their ten answers, the five that press Space, their 51% confidence, the 5 ms delay of the frontier answer, and the three sliders that project offers, and run as they do there.

**Where models run** (*Settings → AI*): *Automatic* measures the CPU and each graphics card on the model and keeps the fastest – for tiny models that is often the CPU – or choose one. MacroMagi runs models with a built-in engine; the optional downloads on the same page add **ONNX Runtime** (faster on the CPU) and **ONNX Runtime for graphics cards** (any DirectX 12 card, Windows). The page also downloads **PyTorch** (for training, later) into the Python macros use, detects copies already on your computer, and uninstalls what it installed (also from *Options → Downloads…*).

> A model keeps the licence of the project it came from; the Magic tab shows the licence an import found. Check it before you share a `.magic` file or a macro that contains a model.

### Several displays

Every click position and search area remembers which display it is on, so a macro keeps working when windows move between displays or the display layout changes. Displays are numbered the same way everywhere: the primary display is 1, the others follow from left to right.

- Positions and areas show a display button next to their numbers; choose another display there, or *All displays* for positions on the whole desktop.
- *Pick* and *Select area* open on the display MacroMagi is on, with buttons at the top for the others. Image and Color Detection search a chosen display when they have no search area.
- Recordings note when the pointer moves to another display (a *Display* row in the Recording Editor).

### Games and windows

- **Switch to Window** brings a program to the front before the next step – by window title or program name, picked from the running programs. It knows that exclusive-fullscreen games take longer to come back than windowed ones, or works it out itself.
- Games with a first-person camera keep moving the pointer back to the middle. **Smart recording** (the default) detects this and records relative motion; **Play Recording** can also be told to replay all movement as relative motion. Right-click a Play Recording block → *Convert to Smart Recording* fixes an older recording.

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

Hotkeys work whether MacroMagi is in front, behind other windows or minimised, and are *observed*, not swallowed: the key still reaches whatever program has focus.

### Options

The **Options** menu holds rules shared by the Auto Clicker and macros:

| Option | Does |
| --- | --- |
| Auto-stop hotkeys | More key combinations that stop everything. |
| Program rules | A whitelist (input only reaches these programs) or blacklist (never reaches them), with search over the running programs. Each program can *stop* the clicker or macro, or *pause* it until an allowed program is in front again. Off by default. |
| Behaviours | *Strict hotkey modifiers* – on: a hotkey works only when exactly its modifier keys are held (`Ctrl+F6` never triggers `F6`); off (default): extra held modifiers are ignored, so `F6` still works while you hold `Shift` in a game. *Stop on task switch* – stop everything on `Alt+Tab` or `Win+Tab`. *Stop the auto clicker when editing a hotkey* (on by default), so you can press the new hotkey. |
| Dry run | Run without sending any input; the console lists what would have been sent. |
| Notify when a macro finishes | A sound and a desktop notification when a macro ends by itself (not when you stop it). For a notification in the middle of a macro, use a **Notification** block. |
| Create desktop shortcut | Puts a shortcut to MacroMagi, with its icon, on the desktop. |
| Downloads… | Opens the optional downloads in *Settings → AI*. |

Options can travel inside a `.magi` file (see [Files](#files-magi-mp-and-magic)).

### Appearance

*Settings → Appearance*:

- **Theme**: System (default), Light or Dark.
- **Theme color**: monochrome by default; a swatch or any color recolors the whole interface, including icons, block categories, wires and the logo.
- **Background**: a PNG, JPEG, BMP, GIF or WebP – animated GIFs and WebPs play – chosen from a file or downloaded from a web address. Adjust how strongly it shows, its blur and darkness, and how opaque the panels over it are.
- **Interface size** (in *General*): drag the slider or type a percentage; the size changes once you let go.

The console, block settings and variables panels, and the settings and program rules windows, keep whatever size you drag them to.

### Engines: Python and native

Macros can run on either of two engines. They do the same thing – the same test suite runs every scenario on both – and you can switch at any time in Settings.

| | Python engine | Native engine |
| --- | --- | --- |
| Needs | Python 3.8 or newer (included in the Windows package) | Nothing |
| Python Script blocks | Yes | Skipped, with a warning |
| Used when | Python is found (the *Automatic* setting) or chosen explicitly | Python is not found, or chosen explicitly |
| Auto Clicker | – | Always |

The Python engine needs **no packages** – only the standard library. It decides what runs next and runs your scripts; the actual clicking, typing, listening and screen reading is done by MacroMagi itself, and variables are kept in one place, so both engines behave identically. An unused Python process is stopped after a few minutes and started again on the next run.

MacroMagi looks for Python in this order: the interpreter chosen in Settings, a `python` folder next to the executable, one downloaded from *Settings → Engine*, then `py -3` / `python` / `python3` on the system. *Settings → Engine* does nothing until you press *Check*, which lists every Python found (it is also done once, the first time a macro runs, and remembered). Choose one, or leave *(auto)*, which shows the Python it stands for. The same page downloads Python and uninstalls it again. Blocks that need Python show a warning sign while it is not available; click it to go to those settings.

Inside a **Python Script** block you get a `magi` object:

```python
seen = magi.find_image("Accept button")      # an image of this macro, by name
if seen:
    magi.click(seen.x, seen.y)
    magi.vars["accepted"] = magi.vars.get("accepted", 0) + 1
else:
    magi.press("ctrl", "r")
    magi.sleep(500)
result = bool(seen)                          # goes out of the block's Result output
```

The full API is in [docs/PYTHON_ENGINE.md](docs/PYTHON_ENGINE.md).

### Command line

```text
macromagi [FILE.magi | FILE.magic] Open the editor, optionally with a macro or a model
macromagi run FILE.magi [OPTIONS]  Run a macro without opening the window
macromagi nodes                    Print every node definition as JSON
macromagi example N FILE.magi      Save built-in example number N as a file

Options for run:
    --engine auto|python|native
    --dry-run                      Send nothing; print what would be sent
```

## Files: `.magi`, `.mp` and `.magic`

A **`.magi`** file is a macro. It is an ordinary ZIP archive – rename it to `.zip` to look inside:

```text
project.json        the macro: a readable text file with the blocks, wires, groups and variables
assets/….png        images linked from blocks
assets/….json       recordings linked from blocks
assets/….wav        sounds linked from Sound Recognition blocks
assets/….magic      AI models linked from AI blocks
presets/….mp        presets bundled with the macro (optional)
```

**File → Export** saves a `.magi` and asks what else to put in it: your **presets**, your **Options** (stop hotkeys, program rules…) and your **Auto Clicker settings**. The defaults for that dialog are in *Settings → General*. When you open a file that carries Options or Auto Clicker settings, MacroMagi asks whether to use them.

A **`.mp`** file is a preset: a small JSON file holding either Auto Clicker settings or a group of configured blocks (with the images, recordings, sounds and models they use embedded). Presets live in `data/presets`.

A **`.magic`** file is an AI model: a ZIP archive with the model (ONNX), a readable `magic.json` describing how to use it, the blocks that go with it, and – if you keep them – pictures for training and fine-tuning. What it includes is chosen in the Magic tab.

All three formats are specified in [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md). Files from 0.1.0 and 0.1.1 open unchanged.

## Where MacroMagi keeps its data

Everything MacroMagi saves goes into a `data` folder next to the executable: `settings.json`, your `presets`, `backgrounds`, the unpacked Python runtime, and the optional downloads in `components`. Set the `MACROMAGI_DATA_DIR` environment variable to use another folder. If the folder cannot be written to, MacroMagi still runs and says that settings will not be saved.

MacroMagi connects to the network only to download a background picture from an address you enter, and an optional part when you press its *Install* button – from its publisher: python.org, Microsoft, or PyTorch's package index.

## Platform support

| | Status | Notes |
| --- | --- | --- |
| **Windows 10/11** | Tested | Primary platform. Everything works. |
| **Linux (X11)** | Implemented, not yet tested | Wayland sessions cannot be listened to (hotkeys, recording) or injected into by the current backends. |
| **macOS** | Implemented, not yet tested | Needs the *Accessibility* permission (input) and *Screen Recording* permission (image detection). |

"Implemented, not yet tested" means the code paths exist and use cross-platform libraries, but nobody has run them yet – expect rough edges, and please [report what you find](https://github.com/MagiGun/MacroMagi/issues).

Windows only, for now: **Switch to Window**, **program rules** (elsewhere the program in front cannot be determined, so input is always allowed), **smart recording** (elsewhere movement is recorded as screen positions), **AI models on a graphics card** (elsewhere they run on the CPU), **Text Scan** (it uses the text recognition built into Windows), and the bundled or downloadable **Python** (elsewhere the system's Python is used). Listening to *what the computer plays* works out of the box on Windows; on Linux and macOS it needs a loopback or monitor input device (the microphone works everywhere).

Known limitations everywhere:

- **Sending gamepad input is not supported yet.** Gamepads can trigger macros and be recorded.
- On Windows, a program running as administrator ignores input from one that is not. To automate such a program, run MacroMagi as administrator too.
- Some games block synthetic input or forbid automation outright.

## Building from source

You need [Rust](https://rustup.rs). The repository pins the exact toolchain in `rust-toolchain.toml`; `rustup` installs it automatically on first build.

```bash
git clone https://github.com/MagiGun/MacroMagi.git
cd MacroMagi
cargo build --release
```

The result is a single file, with the MacroMagi icon on Windows: `target/release/macromagi` (`macromagi.exe` on Windows). Copy it anywhere and run it.

To run from the source tree while developing:

```bash
cargo run -p magi-app
```

**Linux** needs development headers for X11, Wayland, PipeWire, ALSA and udev. On Debian or Ubuntu:

```bash
sudo apt install pkg-config libclang-dev libx11-dev libxi-dev libxtst-dev libxcb1-dev libxrandr-dev \
    libxkbcommon-dev libwayland-dev libegl-dev libdbus-1-dev libpipewire-0.3-dev libudev-dev libasound2-dev
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
git tag v0.1.2
git push origin v0.1.2
```

The workflow builds the Windows package, with Python in it, and publishes a GitHub release with it and its SHA-256 checksum only when the build succeeds. Linux and macOS packages are not built for now.

## Project layout

```text
crates/
  magi-core/      The data model: graphs, block definitions, variables, recordings, options, file formats. No OS or UI code.
  magi-vision/    Image matching and color search on raw pixels.
  magi-audio/     Loudness, pitch and sound recognition on raw samples; reading sound files.
  magi-ml/        AI models: preparing pictures for them and running them, on the CPU or a graphics card.
  magi-platform/  Everything that touches the OS: sending input, listening, gamepads, screen capture, sound, notifications, windows.
  magi-engine/    Runs macros: the native interpreter and the bridge to Python.
  magi-app/       The desktop application (egui), including the Magic tab and optional downloads.
runtime/python/   The Python engine's source. Compiled into the app; unpacked on first use.
assets/icon/      The application icon (SVG source, PNGs and the Windows .ico).
docs/             Architecture, file formats, block reference, guides.
tools/            Scripts that generate documentation.
```

Each layer only depends on the ones below it, and everything a block *is* (its ports, settings and documentation) is plain data in `magi-core`. That is what keeps features removable and is the groundwork for add-ons. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains the design and where to extend it.

## Roadmap

Not promises, but the direction:

- **Add-ons.** User-made blocks, written in Python, described by the same definition format the built-in blocks use.
- **Training in the Magic tab**: label pictures, then train or fine-tune a model on your own computer, without sending anything to a server.
- **Sending gamepad input.** This needs a virtual controller device, which on Windows means a driver – at odds with being portable. The plan is to use one if it is already installed.
- **Editor**: a minimap, aligning and tidying, highlighting the path a run took.
- **Recording**: window-relative coordinates, rescaling to another screen size, merging and splitting recordings.
- **Code signing** for the Windows build, so SmartScreen stops warning.
- Switch to Window, program rules and smart recording on Linux and macOS.

## Credits

MacroMagi is developed and created by [MagiGun](https://github.com/MagiGun) and [uvxszi](https://github.com/uvxszi). The app shows this too: click the logo at the top left, or see *Settings → General → About*.

## Contributing

Contributions are welcome – bug reports, platform testing (especially Linux and macOS), documentation and code. Start with [CONTRIBUTING.md](CONTRIBUTING.md); to add a block, follow [docs/ADDING_A_NODE.md](docs/ADDING_A_NODE.md). AI coding agents should read [AGENTS.md](AGENTS.md).

## Using MacroMagi responsibly

MacroMagi sends real input to your computer.

- Know your stop hotkey (`F8`) before you run a macro that loops. Try unfamiliar macros with **Dry run** first.
- A `.magi` file from someone else can contain a Python Script block, which is code that runs with your permissions. Read it before you run it, exactly as you would any script. A `.magic` file contains no code, but an AI Watch block presses keys on its own – try a new model with *Press the key* off first.
- Sound blocks can listen to your microphone while a macro runs them. Nothing heard is stored or sent anywhere.
- Automation is against the rules of many online games and services. Whether you may use it is between you and them.

## Terms

MacroMagi is **source available**: you may read the code, build it yourself, modify it for your own use and contribute back – but you may not sell the program or its code, or distribute builds of it. The full terms, including how contributions are licensed and the code of conduct, are in [TERMS.md](TERMS.md).
