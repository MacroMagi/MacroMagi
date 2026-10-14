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
            └───┬─────┬───┘   └────────┬──────────┘  talks to the OS
                │     │                │
     ┌──────────▼┐   ┌▼────────────────▼──┐
     │magi-vision│   │      magi-core      │  data model, no OS, no UI
     └───────────┘   └─────────────────────┘
```

| Crate | Owns | Must not |
| --- | --- | --- |
| `magi-core` | Graphs, block definitions, the input vocabulary, recordings, assets, the `.magi` and `.mp` formats, the `Host` interface | Touch the OS or draw anything |
| `magi-vision` | Template matching and colour search on pixel buffers | Know where pixels come from |
| `magi-platform` | Sending input, listening for input, gamepads, screen capture | Know about graphs or blocks |
| `magi-engine` | Running a graph: the native interpreter, the Python bridge, shared device operations | Touch the OS directly (it goes through `Host`) |
| `magi-app` | The window, the controller thread, the command line | Contain block behaviour |

Dependencies point downwards only. `magi-engine` and `magi-platform` do not know about each other: the engine asks a `Host` to do things, the platform crate provides one, and the app hands it over.

The Python runtime in `runtime/python` is a sixth component. Its sources are compiled into `magi-engine` and unpacked into the data folder on first use.

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

**Disabled blocks.** A disabled flow block is skipped: execution continues from its `out` output (or its first output if it has no `out`). A disabled value block yields `null`.

**Errors.** A failure ends the run and is reported against the block it happened in.

**Stopping.** When a run ends for any reason, every key and button it still holds is released.

**Busy loops.** A run that executes thousands of steps without ever waiting is made to yield briefly, so a loop with nothing to wait for cannot pin a CPU core.

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
}
```

Three implementations exist:

- `SystemHost` (`magi-platform`) – the real machine.
- `SimHost` (`magi-core`) – a virtual pointer, a log of what was sent, and a picture standing in for the screen. Used by tests.
- Dry-run hosts (`magi-app`) – real screen and pointer, input only logged.

Adding a capability a macro can use (clipboard, window focus, sound) means adding a method here, implementing it in `SystemHost` and `SimHost`, and exposing it through an operation in `ops.rs`.

## The application

### The controller

A macro tool spends most of its life minimised behind a game, so nothing that matters may depend on the window being drawn. [`controller.rs`](../crates/magi-app/src/controller.rs) owns hotkeys, recording and engine sessions on its own thread. The UI is one client: it *publishes* the macro being edited and the current settings, and *reads* what is running. A global hotkey is another client. They call the same functions.

```text
 OS input ──► InputHub ──► controller thread ──► HotkeyTracker ──► start / stop / record
                                   ▲                                      │
 UI thread ── publish(macro), set_config(settings) ──────────────────────►│
           ◄── running blocks, logs, finished recordings ─────────────────┘
```

### Documents and undo

Editing code never reports changes. Once per frame [`Document::track`](../crates/magi-app/src/document.rs) compares the project with what it last saw. From that one comparison come the revision number (which tells the controller to pick up the new version), the unsaved-changes marker and the undo steps. A gesture in progress – a drag, a text field with focus – is folded into a single undo step.

### UI modules

Each tab and larger widget is a module under `magi-app/src/ui`. A module edits the data it is given and returns *requests* for anything beyond it (pick a point on screen, listen for a hotkey, open a file dialog). Modules do not reach into application state, which keeps them movable and replaceable.

The node editor draws nodes by hand instead of composing widgets. Geometry is computed up front, so wires can be drawn beneath nodes in the same frame and cost stays flat with hundreds of nodes.

## Threads

| Thread | Does |
| --- | --- |
| UI | Draws; owns the document and settings |
| `magi-controller` | Reacts to global input: hotkeys, recording |
| `magi-listen` | Low-level keyboard and mouse hooks (must stay fast – the OS waits on it) |
| `magi-gamepad` | Polls controllers |
| `magi-inject` | Owns the OS input connection; serialises everything sent |
| `magi-run-N` | One per run on the native engine |
| `magi-pyrun-N` | One per run on the Python engine: performs that run's device operations |
| `magi-python-*` | Read the Python process's output |

## Extension points

| To add… | Touch |
| --- | --- |
| A block | A definition, a native function, a Python function – see [ADDING_A_NODE.md](ADDING_A_NODE.md) |
| A kind of setting | A `ParamKind` variant and one editor in `ui/inspector.rs` |
| Something a macro can do to the machine | `Host`, then an operation in `ops.rs` |
| Another way of seeing the screen | A function in `magi-vision`, an operation, a value block |
| An engine | Something that can start and stop runs behind `Session` |
| An operating system backend | A module in `magi-platform` |

## Planned, and how it fits

**Add-ons.** An add-on would be a folder with a manifest listing `NodeDef`s (the same JSON `macromagi nodes` prints) and a Python module registering behaviour for them with the existing `@flow_node` / `@value_node` decorators. The registry already accepts definitions at run time, the editor already draws from definitions, and unknown blocks already survive a round trip. What is missing is discovery, loading and a trust model for third-party code.

**A local vision model.** A function in `magi-vision` taking pixels and a reference image or prompt, an operation in `ops.rs`, and a value block with the same outputs as Image Detection. Nothing else needs to know.

**Sending gamepad input.** `InputEvent` already has gamepad events, and recordings already contain them. What is missing is a backend in `magi-platform` that owns a virtual controller. On Windows that requires a driver, which a portable app cannot install; the intended approach is to use one if present and say so clearly if not.

## Portability rules

- Write only inside `Paths::data_dir` (next to the executable, or `MACROMAGI_DATA_DIR`).
- No registry keys, no files in the user profile, no background services. `eframe`'s own persistence is switched off.
- The Python engine sets `PYTHONDONTWRITEBYTECODE` so it leaves no caches behind.
- Anything that needs something installed (Python, a driver) must be optional, detected at run time, and explained when missing.
