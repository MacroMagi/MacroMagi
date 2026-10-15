# Architecture

This document explains how MacroMagi is put together and, more importantly, *why* – so that changing it, removing a feature or adding one stays a local job.

## Design goals

1. **Modular.** A feature should live in one place. Removing a block, an engine or a whole capability should mean deleting code, not untangling it.
2. **Extensible by data.** What a block *is* – its ports, settings, documentation – is plain data. Code that is not about a specific block (the editor, the inspector, the file format) never names one.
3. **Portable.** One executable, one data folder next to it, no installation, no traces.
4. **Honest about the machine.** Everything that touches the operating system is behind one interface, so the rest can be tested without a desktop.

## The crates

```text
                  ┌─────────────────────────────┐
                  │           magi-app           │  egui UI, controller, CLI
                  └───┬───────────┬──────────┬───┘
                      │           │          │
            ┌─────────▼───┐   ┌───▼──────────▼───┐
            │ magi-engine │   │   magi-platform   │  the only crate that
            └┬────┬─────┬─┘   └────────┬──────────┘  talks to the OS
             │    │     │              │
 ┌───────────▼┐ ┌─▼─────────┐ ┌────────▼────────────┐
 │ magi-vision│ │ magi-audio│ │      magi-core      │  data model, no OS, no UI
 └────────────┘ └───────────┘ └─────────────────────┘
```

| Crate | Owns | Must not |
| --- | --- | --- |
| `magi-core` | Graphs, block definitions, the input vocabulary, recordings, assets, the `.magi` and `.mp` formats, the `Host` interface | Touch the OS or draw anything |
| `magi-vision` | Template matching and colour search on pixel buffers | Know where pixels come from |
| `magi-audio` | Loudness, pitch, sound fingerprints and matching; decoding uploaded sound files | Know where sound comes from |
| `magi-platform` | Sending input, listening for input, gamepads, screen capture, listening to sound, notifications, the clipboard | Know about graphs or blocks |
| `magi-engine` | Running a graph: the native interpreter, the Python bridge, shared device operations | Touch the OS directly (it goes through `Host`) |
| `magi-app` | The window, the controller thread, the command line | Contain block behaviour |

Dependencies point downwards only. `magi-engine` and `magi-platform` do not know about each other: the engine asks a `Host` to do things, the platform crate provides one, and the app hands it over.

The Python runtime in `runtime/python` is a seventh component. Its sources are compiled into `magi-engine` and unpacked into the data folder on first use.

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

**Variables.** A macro's variables live in its session and are shared by all its runs. They start from the project's values whenever a run starts while nothing else is running. A setting bound to a variable (`{"$var": name}`) is resolved just before the block runs, converted to the kind of value the setting expects ([`variables.rs`](../crates/magi-core/src/variables.rs)). Both engines keep variables on the Rust side: the Python engine reads and changes them through operations on the pipe, so there is one source of truth, and the monitor shows their current values.

**Groups.** A group that is turned off disables every block in it. The engines never see groups: the frozen program is built from a copy of the graph in which those blocks are disabled.

**Disabled blocks.** A disabled flow block is skipped: execution continues from its `out` output (or its first output if it has no `out`). A disabled value block yields `null`.

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

Both engines decide *what happens next*. Neither clicks or looks at the screen itself: they call the shared device operations in [`ops.rs`](../crates/magi-engine/src/ops.rs). The Python engine does so over its pipe.

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
    fn listen(&self, source: SoundSource, ms: f64) -> Result<SoundClip, HostError>;  // recent sound
}
```

Three implementations exist:

- `SystemHost` (`magi-platform`) – the real machine.
- `SimHost` (`magi-core`) – a virtual pointer, a log of what was sent, and a picture standing in for the screen. Used by tests.
- Dry-run hosts (`magi-app`) – real screen and pointer, input only logged.

Adding a capability a macro can use means adding a method here, implementing it in `SystemHost` and `SimHost`, and exposing it through an operation in `ops.rs`. *Switch to Window*, the clipboard, notifications and listening to sound were all added this way.

**Sound.** `listen` returns the most recent stretch of sound from what the computer plays (loopback capture) or the microphone. `SystemHost` opens a source only while a macro listens: a thread owns the audio stream, keeps the last seconds in a ring buffer and closes the stream after a quiet while. The analysis – loudness, the strongest frequency, and recognising an uploaded sound by comparing the shape of its spectrum over time, independent of volume – is pure code in `magi-audio`. A recognised sound counts once: what was heard before the match is not looked at again.

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

Every colour comes from one `Palette` in [`theme.rs`](../crates/magi-app/src/theme.rs), built from three choices: light or dark (or the system's), the theme colour (none = monochrome), and how opaque panels are over a background picture. UI code asks for colours by role – `theme::text()`, `theme::accent()`, `theme::category_color(…)` – never by value, so changing the theme recolours everything at once, block categories and icons included.

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
| An engine | Something that can start and stop runs behind `Session` |
| An operating system backend | A module in `magi-platform` |

## Planned, and how it fits

**Add-ons.** An add-on would be a folder with a manifest listing `NodeDef`s (the same JSON `macromagi nodes` prints) and a Python module registering behaviour for them with the existing `@flow_node` / `@value_node` decorators. The registry already accepts definitions at run time, the editor already draws from definitions, and unknown blocks already survive a round trip. What is missing is discovery, loading and a trust model for third-party code.

**A local vision model.** A function in `magi-vision` taking pixels and a reference image or prompt, an operation in `ops.rs`, and a value block with the same outputs as Image Detection. Nothing else needs to know.

**Sending gamepad input.** `InputEvent` already has gamepad events, and recordings already contain them. What is missing is a backend in `magi-platform` that owns a virtual controller. On Windows that requires a driver, which a portable app cannot install; the intended approach is to use one if present and say so clearly if not.

## Portability rules

- Write only inside `Paths::data_dir` (next to the executable, or `MACROMAGI_DATA_DIR`). Background pictures are copied into it, so settings never point elsewhere.
- No registry keys, no files in the user profile, no background services. `eframe`'s own persistence is switched off. The single exception is *Options → Create desktop shortcut*, which writes a shortcut when – and only when – the user asks for one.
- No network access, except downloading a background picture from an address the user typed.
- The Python engine sets `PYTHONDONTWRITEBYTECODE` so it leaves no caches behind.
- Anything that needs something installed (Python, a driver) must be optional, detected at run time, and explained when missing.
