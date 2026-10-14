# The Python engine

MacroMagi can run macros with Python. This page covers how to set that up, the scripting API available in **Python Script** blocks, and how the engine works.

- [Setting it up](#setting-it-up)
- [Python Script blocks](#python-script-blocks)
- [The `magi` API](#the-magi-api)
- [How it works](#how-it-works)
- [Working on the runtime](#working-on-the-runtime)

## Setting it up

Install any Python 3.8 or newer. That is all: the engine uses only the standard library, so there is nothing to `pip install`.

With the engine setting on *Automatic* (the default), MacroMagi uses Python when it finds it and the built-in native engine when it does not. It looks, in order, for:

1. the interpreter chosen under *Settings → Engine*;
2. a `python` folder next to the MacroMagi executable (`python/python.exe` on Windows, `python/bin/python3` elsewhere);
3. `py -3`, `python`, `python3` on Windows; `python3`, `python` elsewhere.

*Settings → Engine → Check* starts Python and tells you what was found.

### A fully portable setup

To carry Python along with MacroMagi, put a self-contained Python distribution in a folder named `python` next to the executable – for example the "embeddable package" from python.org on Windows. MacroMagi prefers it over anything installed on the system.

## Python Script blocks

A Python Script block runs your code when the flow reaches it. Three names are available:

| Name | What it is |
| --- | --- |
| `magi` | The [scripting API](#the-magi-api). |
| `value` | Whatever is wired into the block's **Value** input, or `None`. |
| `result` | Assign to it to send a value out of the block's **Result** output. |

```python
# Count how often this block has run, and click every third time.
magi.vars["runs"] = magi.vars.get("runs", 0) + 1
if magi.vars["runs"] % 3 == 0:
    magi.click()
result = magi.vars["runs"]
```

Things to know:

- `print()` writes to the MacroMagi console.
- An exception ends the run and is shown in the console with its line number, against the block.
- Each run of a Start block is its own thread. `magi.vars` is shared between all of them; use your own lock if several runs change the same entry.
- In long loops of your own, call `magi.sleep()` now and then or check `magi.stopped`. A script that never does cannot be asked to stop; after a few seconds MacroMagi restarts the Python engine to end it.
- You can `import` anything your Python has installed.
- Python Script blocks only run on the Python engine. On the native engine they report an error.

## The `magi` API

Positions are screen pixels. Times are milliseconds.

### Timing and control

| Call | Does |
| --- | --- |
| `magi.sleep(ms)` | Waits. Ends at once if the macro is stopped. |
| `magi.stop()` | Ends this Start block's run. |
| `magi.stop_all()` | Ends every run of the macro. |
| `magi.stopped` | `True` once a stop has been requested. |
| `magi.vars` | A dictionary shared by every script block of the macro. |
| `magi.log(*values)` | Writes a line to the console. |

### Keyboard

| Call | Does |
| --- | --- |
| `magi.press(*keys, hold_ms=30)` | Taps a key, or several together: `magi.press("ctrl", "c")`. |
| `magi.key_down(key)` / `magi.key_up(key)` | Holds / releases a key. |
| `magi.type(text)` | Types text, independent of the keyboard layout. |

Keys can be named loosely – `"a"`, `"5"`, `"f5"`, `"ctrl"`, `"shift"`, `"alt"`, `"win"`, `"enter"`, `"space"`, `"esc"`, `"tab"`, `"up"`, `"pagedown"`, `";"` – or exactly, with the ids used in files (`"KeyA"`, `"Numpad5"`, `"ControlRight"`).

Keys still held when the run ends are released automatically.

### Mouse

| Call | Does |
| --- | --- |
| `magi.click(x=None, y=None, button="left", clicks=1, hold_ms=30)` | Clicks at the pointer, or at `x, y`. |
| `magi.mouse_down(button="left")` / `magi.mouse_up(button="left")` | Holds / releases a button. |
| `magi.move(x, y, relative=False, duration_ms=0)` | Moves the pointer; with `duration_ms` it glides. |
| `magi.scroll(dy, dx=0)` | Scrolls by wheel notches. Positive `dy` is up. |
| `magi.cursor()` | Returns the pointer position as `(x, y)`. |

Buttons: `"left"`, `"right"`, `"middle"`, `"back"`, `"forward"`.

### Screen

| Call | Does |
| --- | --- |
| `magi.find_image(image, region=None, threshold=0.8)` | Looks for one of the macro's images, by name. |
| `magi.find_color(color, tolerance=16, region=None)` | Looks for a colour such as `"#FF8800"`. |

`region` is `(x, y, width, height)`; `None` searches the whole primary screen.

Both return a `Sighting` with `found`, `x`, `y` and `score`. It is truthy when found; `x` and `y` are `None` when not.

```python
seen = magi.find_image("Accept button", region=(0, 0, 800, 600), threshold=0.9)
if seen:
    magi.click(seen.x, seen.y)
magi.log("similarity:", seen.score)
```

`image` is the name shown for the image in MacroMagi (*Image name* in an Image Detection block's settings), so the image must be part of the macro.

## How it works

The Python engine is an interpreter for the same graphs the native engine runs. It decides which block runs next, evaluates conditions and executes script blocks. It does **not** touch the keyboard, mouse or screen. For those it asks MacroMagi, over a pipe, to perform an operation – the same Rust code the native engine calls directly.

```text
 MacroMagi (Rust)                              Python process
 ───────────────                               ──────────────
 load program  ───────────────────────────────►  build graph
 start run 7 at block 3  ─────────────────────►  thread: walk the graph
                         ◄────────────────────   enter block 3
                         ◄────────────────────   call #12: sleep 500 ms
 (waits 500 ms, precisely)
 reply #12: ok  ──────────────────────────────►
                         ◄────────────────────   call #13: actions [click left]
 (clicks)
 reply #13: ok  ──────────────────────────────►
                         ◄────────────────────   done run 7
```

This is why no packages are needed, why both engines click and see identically, and why timing stays accurate: waits and recording playback are timed in Rust, not by Python's scheduler.

Messages are JSON, one per line, on standard input and output. They are listed at the top of [`runtime/python/magi/protocol.py`](../runtime/python/magi/protocol.py). The operations Python can ask for are `sleep`, `actions`, `play_recording`, `find_image`, `find_color`, `send`, `move` and `cursor` – implemented in `perform` in [`crates/magi-engine/src/python.rs`](../crates/magi-engine/src/python.rs).

One Python process is started on first use and kept for as long as MacroMagi runs, so hotkey-started macros begin without a start-up delay.

The runtime's source is built into the MacroMagi executable. On first use it is unpacked to `data/runtime/python-<hash>/`; a new MacroMagi version unpacks a fresh copy and removes the old one.

## Working on the runtime

The source is in [`runtime/python`](../runtime/python):

```text
magi_host.py          entry point started by the app
magi/host.py          message loop
magi/protocol.py      the channel and request/reply plumbing
magi/interpreter.py   walks a graph; mirrors crates/magi-engine/src/native.rs
magi/nodes/           behaviour of each block type
magi/api.py           the `magi` object for script blocks
```

Rules:

- **Standard library only**, and syntax that works on Python 3.8.
- Never write to the real `stdout`: it carries the protocol. `print()` is redirected to the console for you.
- The interpreter must stay in step with the native one. The contract is the "Execution model" section of [ARCHITECTURE.md](ARCHITECTURE.md).

To try changes without rebuilding, point MacroMagi at your working copy:

```bash
MACROMAGI_PYTHON_RUNTIME=/path/to/MacroMagi/runtime/python cargo run -p magi-app
```

The tests that cover the runtime are the cross-engine scenarios:

```bash
cargo test -p magi-engine --test engines
```

Each scenario there runs on the native engine and, if Python is installed, on the Python engine, and both must produce the expected input. They are skipped for Python (with a note in the output) when no interpreter is found.
