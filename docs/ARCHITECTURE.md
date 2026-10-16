# Architecture

This document explains how MacroMagi is put together and, more importantly, *why* – so that changing it, removing a feature or adding one stays a local job.

## Design goals

1. **Modular.** A feature should live in one place. Removing a block, an engine or a whole capability should mean deleting code, not untangling it.
2. **Extensible by data.** What a block *is* – its ports, settings, documentation – is plain data. Code that is not about a specific block (the editor, the inspector, the file format) never names one.
3. **Portable.** One executable, one data folder next to it, no installation, no traces.
4. **Honest about the machine.** Everything that touches the operating system is behind one interface, so the rest can be tested without a desktop.

## The crates

```text
magi-app                egui UI, controller, CLI, the Magic tab
├── magi-engine         runs macros
│   ├── magi-vision     pictures: image and colour search
│   ├── magi-audio      sound: loudness, pitch, recognising sounds
│   ├── magi-ml         AI models: preparing pictures, running them
│   └── magi-core
├── magi-ml
├── magi-platform       the only crate that talks to the OS
│   └── magi-core
└── magi-core           data model, no OS, no UI
```

| Crate | Owns | Must not |
| --- | --- | --- |
| `magi-core` | Graphs, block definitions, the input vocabulary, recordings, assets, the `.magi`, `.mp` and `.magic` formats, the `Host` interface | Touch the OS or draw anything |
| `magi-vision` | Template matching and colour search on pixel buffers | Know where pixels come from |
| `magi-audio` | Loudness, pitch, sound fingerprints and matching; decoding uploaded sound files | Know where sound comes from |
| `magi-ml` | Running small vision models (ONNX): preparing a picture the way a model expects, the built-in engine, ONNX Runtime when it has been downloaded, choosing the CPU or a graphics card | Capture the screen or know about blocks |
| `magi-platform` | Sending input, listening for input, gamepads, screen capture, listening to sound, notifications, the clipboard, listing graphics cards and sound inputs | Know about graphs or blocks |
| `magi-engine` | Running a graph: the native interpreter, the Python bridge, shared device operations | Touch the OS directly (it goes through `Host`) |
| `magi-app` | The window, the controller thread, the command line, the Magic tab and its importer, optional downloads | Contain block behaviour |

Dependencies point downwards only. `magi-engine` and `magi-platform` do not know about each other: the engine asks a `Host` to do things, the platform crate provides one, and the app hands it over.

The Python runtime in `runtime/python` is an eighth component. Its sources are compiled into `magi-engine` and unpacked into the data folder on first use.

## A block is data

A block type is described by a [`NodeDef`](../crates/magi-core/src/nodes/mod.rs):

- an id such as `flow.delay`, a name, a category, a description, search keywords;
- its ports – whether it has a flow input, its flow outputs, its value inputs and outputs with their types;
- its parameters – each with a key, a label, a default and a **kind** (`Duration`, `Percent`, `Image`, `Hotkey`, …);
- optionally, ports that depend on a parameter (a Decision has one output per case).

A `NodeDef` contains no behaviour and serializes to JSON (`macromagi nodes` prints all of them). Three independent things are keyed by its id:

| What | Where | Lookup |
| --- | --- | --- |
| The definition | `magi-core/src/nodes/` | `Registry` |
| Native behaviour | `magi-engine/src/native.rs` | `NativeNodes` |
| Python behaviour | `runtime/python/magi/nodes/` | the `FLOW` and `VALUE` tables |

The editor draws any block from its definition. The inspector builds a settings form from the parameter list, one editor per parameter *kind* – so a new block gets a complete UI without any UI code, and a new parameter kind is one new editor.

In a saved graph, a node is just `{ id, type, pos, params }`. A node whose type is unknown (say, from an add-on that is not installed) is kept exactly as it is, drawn as a pass-through block, and its wires are preserved.

## Execution model

This is the contract both engines implement. If you change one, change the other; `crates/magi-engine/tests/engines.rs` runs every scenario on both.

**Flow.** Execution starts at a Start block and follows flow wires, one block at a time. A flow output leads to at most one block. When a block's output is not wired, the branch ends.

**Runs.** Each started Start block is a *run*: an independent thread of execution. Runs share nothing but the macro. Stopping a run interrupts whatever it is waiting on.

**Values.** Value blocks (Image Detection, Logic, …) are not part of the flow. When a flow block reads one of its value inputs, the block wired to it is evaluated – recursively – and the result is remembered *for the rest of that step*. So a Decision with three cases fed by one Image Detection takes one screenshot, and the next step looks at the screen afresh.

**Truthiness.** `null`, `false`, `0` and `""` are false; everything else is true. An unwired condition is false.

**Repeat.** Entering a Repeat pushes a loop frame. When the branch under *Loop* ends, execution returns to the innermost Repeat, which either starts the next iteration or pops its frame and continues through *Done*.

**Repeat until / while.** A Repeat in *until* or *while* mode reads its Condition input before every round, including the first.

**Stop.** A Stop block ends its run, or every run; or, inside a Repeat, only the current round (*next round*) or the whole loop (continuing from *Done*); or it continues from another block (*go to*).

**Functions.** A Function block runs the blocks under the Definition block of the same name as a nested call: loops entered inside the definition belong to it, and when its branch ends, execution continues after the Function block. What is wired into the Function's Input is stored in the definition's input variable first. Definition blocks are never started by Run or a hotkey.

**Rate.** A Rate block performs its list of inputs one after another, a set time apart. The time is its own setting, a variable it follows, or a number wired into its Rate input – read once, when the block starts.

**Variables.** A macro's variables live in its session and are shared by all its runs. They start from the project's values whenever a run starts while nothing else is running – except variables set to keep their value, which carry on from the last run, also after the macro is edited, until MacroMagi closes. A setting bound to a variable (`{"$var": name}`) is resolved just before the block runs, converted to the kind of value the setting expects ([`variables.rs`](../crates/magi-core/src/variables.rs)). Both engines keep variables on the Rust side: the Python engine reads and changes them through operations on the pipe, so there is one source of truth, and the monitor shows their current values.

**Groups.** A group that is turned off disables every block in it. The engines never see groups: the frozen program is built from a copy of the graph in which those blocks are disabled.

**Disabled blocks.** A disabled flow block is skipped: execution continues from its `out` output (or its first output if it has no `out`). A disabled value block yields `null`.

**Blocks one engine cannot run.** Python Script blocks need the Python engine. The native engine treats them as disabled and says so once in the console; the editor marks them while the Python engine is not available.

**AI models.** An AI block names a model asset – a `.magic` file embedded in the macro. The program loads a model once, the first time a block uses it, onto the engine chosen under *Settings → AI*. A look captures the model's screen area, prepares the picture as the model's description says, runs the model and decides: the likeliest answer *acts* when its class is marked to act and the model is at least as sure as that class's threshold. The block's own setting values are applied over the model's first. AI Watch repeats this, waits the class's delay before pressing the model's key, and then ignores answers for the cooldown so one event is not acted on twice. Both engines call the same operations, so a model behaves the same on either.

**Errors.** A failure ends the run and is reported against the block it happened in.

**Stopping.** When a run ends for any reason, every key and button it still holds is released.

**Busy loops.** A run that executes thousands of steps without ever waiting is made to yield briefly, so a loop with nothing to wait for cannot pin a CPU core.

**Program rules.** Before any input that *presses* something is sent, the run checks which program is in front against the user's program rules (whitelist or blacklist, [`options.rs`](../crates/magi-core/src/options.rs)). A denied program either stops the run or pauses it until an allowed program is back in front. Releases are always sent, so nothing is left held. The check is cached for 25 ms, so a thousand clicks a second do not mean a thousand system calls.

## Two engines, one set of hands

```text
   native engine                          Python engine
 ┌────────────────┐                ┌──────────────────────────┐
 │ interpreter    │                │ interpreter (Python)     │
 │ (Rust)         │                │  + Python Script blocks  │
 └───────┬────────┘                └────────────┬─────────────┘
         │ direct calls                 JSON lines over a pipe
         └──────────────┬────────────────────────┘
                 ┌──────▼───────┐
                 │  ops (Rust)  │   perform actions, play a recording,
                 └──────┬───────┘   find an image, wait precisely
                        │
                   ┌────▼────┐
                   │  Host   │   send input · pointer position · capture screen
                   └─────────┘
```

Both engines decide *what happens next*. Neither clicks, looks at the screen, listens nor runs an AI model itself: they call the shared device operations in [`ops.rs`](../crates/magi-engine/src/ops.rs). The Python engine does so over its pipe.

Consequences:

- The Python runtime needs no third-party packages. It can run on any Python 3.8+ without `pip`, which is what keeps it portable.
- A click, a recording replay or an image search is the same code whichever engine asked for it. Timing-critical loops (replaying a recording) run entirely in Rust.
- The engines can only differ in control flow, which is small and covered by the shared tests.

The pipe protocol is documented at the top of [`runtime/python/magi/protocol.py`](../runtime/python/magi/protocol.py). One Python process serves the whole application; programs are loaded into it by id and each run is a Python thread paired with a Rust worker thread.

A script stuck in its own loop cannot be interrupted from outside (Python threads cannot be killed). If a run ignores a stop request for a few seconds, the Python process is terminated and restarted on next use.

## The `Host` boundary

```rust
pub trait Host: Send + Sync {
    fn send(&self, event: &InputEvent) -> Result<(), HostError>;
    fn cursor(&self) -> Result<Point, HostError>;
    fn capture(&self, region: Option<Rect>) -> Result<Frame, HostError>;

    // With defaults, so a minimal host only needs the three above:
    fn screens(&self) -> Result<Vec<Rect>, HostError>;          // monitor rectangles
    fn foreground_process(&self) -> Option<String>;            // for program rules
    fn focus_window(&self, query: &WindowQuery) -> Result<Option<WindowInfo>, HostError>;
    fn clipboard_text(&self) -> Result<String, HostError>;
    fn notify(&self, notification: &Notification) -> Result<(), HostError>;
    fn listen(&self, source: SoundSource, device: &str, ms: f64) -> Result<SoundClip, HostError>;  // recent sound
}
```

Three implementations exist:

- `SystemHost` (`magi-platform`) – the real machine.
- `SimHost` (`magi-core`) – a virtual pointer, a log of what was sent, and a picture standing in for the screen. Used by tests.
- Dry-run hosts (`magi-app`) – real screen and pointer, input only logged.

Adding a capability a macro can use means adding a method here, implementing it in `SystemHost` and `SimHost`, and exposing it through an operation in `ops.rs`. *Switch to Window*, the clipboard, notifications and listening to sound were all added this way.

**Sound.** `listen` returns the most recent stretch of sound from what the computer plays (loopback capture) or a microphone – a named one, or the system's default. `SystemHost` opens a source only while a macro listens: a thread owns the audio stream, keeps the last seconds in a ring buffer and closes the stream after a quiet while. Each clip says where it ends in the stream, so a listener knows which part it has already heard. The analysis – loudness, the strongest frequency, and recognising an uploaded sound by comparing the shape of its spectrum over time, independent of volume – is pure code in `magi-audio`. Recognition keeps a running fingerprint of the stream rather than re-analysing it on every look, which keeps long sounds cheap, and can recognise a sound by its first part while it is still playing. A recognised sound counts once: what was heard before the match is not looked at again.

**Images.** An Image Detection block may look for several images. Images that share a search area share one screen capture, and the images are matched in parallel. With less *Detail*, the capture and the images are shrunk before matching; shrunk images are cached by the program, so this costs nothing per look.

## AI models

[`magi-ml`](../crates/magi-ml/src/lib.rs) runs a model on a prepared picture and knows nothing about screens or blocks. It has two engines:

| Engine | What | When |
| --- | --- | --- |
| Built-in | [tract](https://github.com/sonos/tract), pure Rust, compiled in | Always available; the CPU only |
| ONNX Runtime | Microsoft's runtime, loaded from the data folder at run time | Once downloaded on the AI settings page; the CPU, or on Windows any DirectX 12 graphics card through DirectML |

ONNX Runtime is loaded with `ort`'s *load-dynamic* feature: MacroMagi does not link it, so the executable stays a single file and works without it. A process can load it once, so installing or removing it takes effect after a restart.

*Settings → AI → Run models on* is **Automatic**, **CPU** or a particular graphics card. Automatic times each candidate on the model itself (a few warm-up runs, then the median of several) and keeps the fastest, remembering the answer per model; when a graphics card is not faster – small models often are not, because of the copying – the CPU is used.

Image Detection stays on the CPU: its coarse-to-fine search already finishes in a few milliseconds, and a graphics card would spend longer receiving the picture.

`magi_ml::prepare` turns pixels into a model's input: resized with antialiasing, laid out channels-first or channels-last, in RGB or BGR order, each value `(v × scale − mean) / std`. These are the settings in a model's description, so a model trained elsewhere can be matched exactly.

## Several displays

Positions and areas carry an optional display number ([`geom.rs`](../crates/magi-core/src/geom.rs)). `Displays` numbers the connected displays the same way on every machine – the primary display first, then the others left to right, top to bottom – and converts between display-relative and desktop coordinates. Engines resolve positions just before they use them (the list of displays is read at most every two seconds), so a macro made on one setup still lands on the right display on another, and a position on a display that is not connected falls back to the primary one. Recordings store a *display* stroke wherever the pointer moved to another display, with the positions after it relative to that display.

## The application

### The controller

A macro tool spends most of its life minimised behind a game, so nothing that matters may depend on the window being drawn. [`controller.rs`](../crates/magi-app/src/controller.rs) owns hotkeys, recording and engine sessions on its own thread. The UI is one client: it *publishes* the macro being edited and the current settings, and *reads* what is running. A global hotkey is another client. They call the same functions.

```text
 OS input ──► InputHub ──► controller thread ──► HotkeyTracker ──► start / stop / record
                                   ▲                                      │
 UI thread ── publish(macro), set_config(settings) ──────────────────────►│
           ◄── running blocks, logs, finished recordings ─────────────────┘
```

The hotkey table is rebuilt whenever the settings or the macro change: the stop-everything, record and extra stop hotkeys always; Alt+Tab and Win+Tab when *Stop on task switch* is on; the Auto Clicker's hotkey on its tab; the Run hotkey and every Start block's hotkey on the Macro tab. A change applies on the next key press – there is nothing to restart.

A `magi-housekeeping` thread keeps memory low while nothing runs: an unused Python process is stopped after a few minutes (it starts again on the next run), and memory is handed back to the system once the window has been minimised for a while.

**Listening reliably on Windows.** Windows silently removes a low-level hook that is slow to answer, once, and never says so – the classic "hotkeys stopped working" bug. The listener thread runs at time-critical priority, does nothing in the hook but queue the event, and a watchdog compares the system's last-input time with the last event the hooks saw. If input happened that the hooks missed, they are reinstalled. Relative mouse motion (what a game camera reads) comes from Raw Input on a message-only window.

**Raw Input and the keyboard hook.** The window library registers for Raw Input keyboard events. While a window of a process with such a registration is in front, Windows stops calling that same process's low-level keyboard hook – so hotkeys could not be assigned, nor fire, while MacroMagi itself was focused. The listener removes the keyboard registration and claims Raw Input for its own message-only window, re-checking every second in case the window library registers again. Details are at the top of [`listen/windows.rs`](../crates/magi-platform/src/listen/windows.rs); a system test reproduces the situation.

### Documents and undo

Editing code never reports changes. Once per frame [`Document::track`](../crates/magi-app/src/document.rs) compares the project with what it last saw. From that one comparison come the revision number (which tells the controller to pick up the new version), the unsaved-changes marker and the undo steps. A gesture in progress – a drag, a text field with focus – is folded into a single undo step.

### UI modules

Each tab and larger widget is a module under `magi-app/src/ui`. A module edits the data it is given and returns *requests* for anything beyond it (pick a point on screen, listen for a hotkey, open a file dialog). Modules do not reach into application state, which keeps them movable and replaceable.

The Auto Clicker's form ([`clicker_form.rs`](../crates/magi-app/src/ui/clicker_form.rs)) edits a `ClickerSettings` and is shared by the Auto Clicker tab and the settings of the Auto Click block – the tab *is* that block, run on its own.

### Look

Every colour comes from one `Palette` in [`theme.rs`](../crates/magi-app/src/theme.rs), built from three choices: light or dark (or the system's), the theme colour (none = monochrome), and how opaque panels are over a background picture. UI code asks for colours by role – `theme::text()`, `theme::accent()`, `theme::category_color(…)` – never by value, so changing the theme recolours everything at once, block categories and icons included. The banner logo is recoloured from the icon by brightness: its ink takes the colour the buttons have, and its white parts are cut out.

### The Magic tab

The Magic tab edits one model at a time – a [`MagicFile`](../crates/magi-core/src/magic.rs): the ONNX bytes, a description (`MagicModel`) and optional training pictures.

- **Importing** ([`magic/import.rs`](../crates/magi-app/src/magic/import.rs)) takes an `.onnx` file, or a folder or ZIP that contains one, and reads the project's text files for hints: class names (a Markdown table or a dictionary in code), which classes act, colour statistics, the screen area, the key, and adjustable settings (a `settings.json`, or typed declarations in code). It reads files; it never runs them. Everything it found or guessed becomes a note in the tab.
- **Trying** a model runs it on a `magi-magic-test` thread against the same host as macros (so dry run applies), and shows the picture it saw and its answers.
- **Use in Macro** embeds the model as an asset (`.magic` bytes without training pictures, with an id derived from the content). An earlier model of the same name is replaced and the blocks that used it are pointed at the new one; when no block uses the model yet, its blocks are added.
- Replacing the model in the tab asks first, unless its changes are saved or already in the macro.

### Optional downloads

[`components.rs`](../crates/magi-app/src/components.rs) installs parts that some features need and MacroMagi does not ship – ONNX Runtime, ONNX Runtime for graphics cards, Python and PyTorch – into `data/components/<part>`, and removes them again. It also reports copies already on the computer (a system Python, PyTorch in the Python in use). Downloads come from each part's publisher only when the user clicks *Install*. A part in use cannot be deleted on Windows, so it is marked and deleted at the next start, and nothing uses a marked part in the meantime.

### Screen targets

[`targets.rs`](../crates/magi-core/src/targets.rs) lists every position a macro clicks and every area it searches, in the order a run reaches them, and writes edits back to the blocks they came from. *View → Screen targets* draws them over a frozen, undimmed picture of one display; it knows nothing about individual blocks. It is a *deferred* viewport – it redraws on its own input rather than whenever the main window does – and exchanges targets and edits with the main window through shared state ([`overlay.rs`](../crates/magi-app/src/ui/overlay.rs)). Both it and the screen picker open full screen on the display MacroMagi is on, and can switch to another.

The node editor draws nodes by hand instead of composing widgets. Geometry is computed up front, so wires can be drawn beneath nodes in the same frame and cost stays flat with hundreds of nodes.

## Threads

| Thread | Does |
| --- | --- |
| UI | Draws; owns the document and settings |
| `magi-controller` | Reacts to global input: hotkeys, recording |
| `magi-listen` | Low-level keyboard and mouse hooks and Raw Input (must stay fast – the OS waits on it), with a watchdog |
| `magi-housekeeping` | Stops an idle Python process; hands memory back while the window is hidden |
| `magi-gamepad` | Polls controllers |
| `magi-audio` | Owns a sound source while a macro listens to it |
| `magi-magic-test` | Runs the Magic tab's model on the screen while it is being tried |
| `magi-notify` | Keeps a notification's tray icon up for a few seconds (Windows) |
| `magi-inject` | Owns the OS input connection; serialises everything sent |
| `magi-run-N` | One per run on the native engine |
| `magi-pyrun-N` | One per run on the Python engine: performs that run's device operations |
| `magi-python-*` | Read the Python process's output |

## Extension points

| To add… | Touch |
| --- | --- |
| A block | A definition, a native function, a Python function – see [ADDING_A_NODE.md](ADDING_A_NODE.md) |
| A kind of setting | A `ParamKind` variant and one editor in `ui/inspector.rs`; mark it `bindable` if it can follow a variable |
| A rule about when input may be sent | `AutomationOptions` in `magi-core/src/options.rs`, checked in `RunCx::send` |
| Something a macro can do to the machine | `Host`, then an operation in `ops.rs` |
| Another way of seeing the screen | A function in `magi-vision`, an operation, a value block |
| A model format or engine | `magi-ml` (a `Backend`), and its options in *Settings → AI* |
| Something to download | A `Component` in `magi-app/src/components.rs` |
| An engine | Something that can start and stop runs behind `Session` |
| An operating system backend | A module in `magi-platform` |

## Planned, and how it fits

**Add-ons.** An add-on would be a folder with a manifest listing `NodeDef`s (the same JSON `macromagi nodes` prints) and a Python module registering behaviour for them with the existing `@flow_node` / `@value_node` decorators. The registry already accepts definitions at run time, the editor already draws from definitions, and unknown blocks already survive a round trip. What is missing is discovery, loading and a trust model for third-party code.

**Training in the Magic tab.** Models are made elsewhere for now. A `.magic` file can already hold training and fine-tuning pictures, sorted by class, and PyTorch can be downloaded into MacroMagi's Python. Training is meant to be a Python script run in that Python, which writes an ONNX model back into the file; the tab, the file format and the blocks need no change for it.

**Sending gamepad input.** `InputEvent` already has gamepad events, and recordings already contain them. What is missing is a backend in `magi-platform` that owns a virtual controller. On Windows that requires a driver, which a portable app cannot install; the intended approach is to use one if present and say so clearly if not.

## Portability rules

- Write only inside `Paths::data_dir` (next to the executable, or `MACROMAGI_DATA_DIR`). Background pictures are copied into it, so settings never point elsewhere.
- No registry keys, no files in the user profile, no background services. `eframe`'s own persistence is switched off. The single exception is *Options → Create desktop shortcut*, which writes a shortcut when – and only when – the user asks for one.
- No network access, except downloading a background picture from an address the user typed, and the optional parts on the AI settings page when the user installs one – each from its publisher (python.org, Microsoft, the Python Package Index).
- The Python engine sets `PYTHONDONTWRITEBYTECODE` so it leaves no caches behind.
- Anything that needs something installed (Python, a driver) must be optional, detected at run time, and explained when missing.
