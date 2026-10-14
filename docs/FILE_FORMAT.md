# File formats

MacroMagi has two file types: **`.magi`** (a macro) and **`.mp`** (a preset). Both are designed to be readable, diffable and easy to produce with other tools.

- [`.magi` – projects](#magi--projects)
- [`.mp` – presets](#mp--presets)
- [Shared vocabulary](#shared-vocabulary)
- [Versioning](#versioning)

## `.magi` – projects

A `.magi` file is a standard ZIP archive. Rename it to `.zip` to open it.

```text
project.json           required – the macro
assets/<id>.png        images
assets/<id>.json       recordings
presets/<nn>-<name>.mp bundled presets (optional)
```

`project.json` lists every other file in the archive. A reader never has to guess file names, and files not listed are ignored.

### `project.json`

```json
{
  "format": "macromagi.project",
  "version": 1,
  "app": "0.1.0",
  "meta": { "name": "Click when ready", "description": "", "author": "" },
  "engine": "auto",
  "graph": {
    "nodes": [
      { "id": 1, "type": "flow.start", "pos": [0.0, 0.0],
        "params": { "hotkey": "F7", "mode": "toggle", "run_on_start": true } },
      { "id": 2, "type": "input.action", "pos": [0.0, 120.0], "label": "Click it",
        "params": { "actions": [ { "kind": "mouse", "button": "Left", "mode": "tap", "clicks": 1 } ], "hold_ms": 30.0 } },
      { "id": 3, "type": "vision.find_image", "pos": [-260.0, 100.0],
        "params": { "image": "img-9f2c61d0a4b7e113", "region": null, "threshold": 0.8 } }
    ],
    "wires": [
      { "from": { "node": 1, "port": "out" },      "to": { "node": 2, "port": "in" } },
      { "from": { "node": 3, "port": "position" }, "to": { "node": 2, "port": "position" } }
    ]
  },
  "assets": [
    { "id": "img-9f2c61d0a4b7e113", "name": "Ready button", "kind": "image", "file": "assets/img-9f2c61d0a4b7e113.png" }
  ],
  "presets": []
}
```

| Field | Meaning |
| --- | --- |
| `format` | Always `macromagi.project`. Anything else is rejected. |
| `version` | Format version, see [Versioning](#versioning). |
| `app` | The MacroMagi version that wrote the file. Informational. |
| `meta` | Name, description and author. All optional. |
| `engine` | `auto` (follow the app setting), `python` or `native`. |
| `graph` | The nodes and wires. |
| `assets` | One entry per file under `assets/`. |
| `presets` | Paths of bundled `.mp` files inside the archive. |

### Nodes

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | yes | A positive integer, unique within the graph. |
| `type` | yes | The block type, e.g. `flow.delay`. See the [block reference](NODES.md). |
| `pos` | yes | `[x, y]` of the node's top-left corner on the canvas. |
| `params` | no | The block's settings. Missing settings take their defaults. |
| `enabled` | no | `false` to disable the block. Omitted when `true`. |
| `label` | no | A custom title. Omitted when empty. |

The meaning of `params` is defined per block type. `macromagi nodes` prints every block's parameters with their kinds and defaults.

A node whose `type` is unknown to the reading app is kept untouched and written back as it was.

### Wires

A wire goes **from an output port to an input port**.

- **Flow** wires end at the port named `in`. A flow output has at most one wire.
- **Value** wires end at a named value input. A value input has at most one wire.

Port names come from the block's definition. Ports of a Decision are derived from its cases: case `"id": "2"` has the condition input `cond:2` and the flow output `case:2`.

Wires that refer to missing nodes are dropped on load.

### Assets

| `kind` | File | Contents |
| --- | --- | --- |
| `image` | `assets/<id>.png` | A PNG image. |
| `recording` | `assets/<id>.json` | A [recording](#recordings). |

Blocks refer to assets by `id` in their parameters (`"image": "img-…"`, `"recording": "rec-1"`). Image ids are derived from the image's content, so the same picture is stored once.

### Recordings

A recording is a list of **strokes** – one per thing the user did – rather than a raw event log.

```json
{
  "duration_ms": 1620.0,
  "screen": [1920, 1080],
  "strokes": [
    { "at": 0.0,   "hold": 70.0,  "type": "key",    "key": "KeyH" },
    { "at": 610.0, "hold": 420.0, "type": "move",   "path": [[0.0, 640, 360], [14.0, 648, 369]] },
    { "at": 1080.0,"hold": 85.0,  "type": "mouse",  "button": "Left", "x": 900, "y": 360 },
    { "at": 1100.0,"hold": 300.0, "type": "mouse",  "button": "Left", "x": 10, "y": 10, "end": { "x": 200, "y": 90 } },
    { "at": 1380.0,"hold": 0.0,   "type": "scroll", "dx": 0, "dy": -2 }
  ]
}
```

Every stroke has:

- `at` – milliseconds from the start of the recording to the **press**;
- `hold` – milliseconds until the **release** (zero for strokes that have none).

| `type` | Extra fields | Notes |
| --- | --- | --- |
| `key` | `key` | |
| `mouse` | `button`, `x`, `y`, optional `end` | With `end`, a drag: pressed at `x, y`, released at `end`. |
| `move` | `path` | A list of `[ms since the stroke began, x, y]`. |
| `scroll` | `dx`, `dy` | Wheel notches. Positive `dy` is up, positive `dx` is right. |
| `pad_button` | `button` | |
| `pad_axis` | `axis`, `value` | |

Strokes need not be sorted; playback orders events by time. `screen` is the size of the primary screen at recording time, kept for future rescaling.

## `.mp` – presets

A preset is a single JSON file.

```json
{
  "format": "macromagi.preset",
  "version": 1,
  "app": "0.1.0",
  "name": "Rapid fire",
  "description": "",
  "kind": "auto_clicker",
  "data": { "…": "…" }
}
```

### `"kind": "auto_clicker"`

`data` is the Auto Clicker's settings:

```json
{
  "target":     { "kind": "mouse", "button": "Left" },
  "clicks":     1,
  "interval_ms": 100.0,
  "jitter_ms":  0.0,
  "hold_ms":    10.0,
  "limit":      { "kind": "forever" },
  "position":   { "kind": "cursor" },
  "hotkey":     "F6",
  "activation": "toggle"
}
```

| Field | Values |
| --- | --- |
| `target` | `{ "kind": "mouse", "button": … }` or `{ "kind": "key", "key": … }` |
| `clicks` | `1`, `2` or `3` (mouse only) |
| `limit` | `{ "kind": "forever" }` or `{ "kind": "count", "count": 100 }` |
| `position` | `{ "kind": "cursor" }` or `{ "kind": "fixed", "x": 300, "y": 400 }` |
| `activation` | `toggle` or `hold` |

Missing fields take their defaults.

### `"kind": "nodes"`

`data` is a piece of a graph, plus the assets it uses, embedded so the file is self-contained:

```json
{
  "nodes": [ { "id": 1, "type": "vision.find_image", "pos": [0.0, 0.0], "params": { "image": "img-…" } } ],
  "wires": [],
  "assets": [
    { "id": "img-…", "name": "Ready button", "kind": "image", "base64": "iVBORw0KGgo…" },
    { "id": "rec-1", "name": "Take 1", "kind": "recording", "recording": { "duration_ms": 0.0, "strokes": [] } }
  ]
}
```

When a nodes preset is inserted, its nodes get fresh ids and its assets are imported (images are de-duplicated by content). This is also the format MacroMagi puts on the clipboard when blocks are copied, prefixed with `macromagi-nodes:`.

## Shared vocabulary

**Keys** are named after the W3C `KeyboardEvent.code` values and identify *physical* keys: `KeyA`, `Digit1`, `F6`, `ShiftLeft`, `ArrowUp`, `Numpad5`, `NumpadEnter`… A key MacroMagi has no name for is written `Raw<code>` with its platform key code. The full list is the `keys!` table in [`crates/magi-core/src/input.rs`](../crates/magi-core/src/input.rs).

**Mouse buttons**: `Left`, `Right`, `Middle`, `Back`, `Forward`.

**Gamepad buttons** are named by position: `South`, `East`, `West`, `North`, `LeftBumper`, `RightBumper`, `LeftTrigger`, `RightTrigger`, `Select`, `Start`, `Guide`, `LeftStick`, `RightStick`, `DPadUp`, `DPadDown`, `DPadLeft`, `DPadRight`.

**Hotkeys** are the buttons to hold, joined by `+`: `F6`, `ControlLeft+ShiftLeft+KeyK`, `Mouse:Back`, `ControlLeft+Pad:South`. Modifier keys are written as their left-hand variant and match either side. An empty string means "no hotkey".

**Screen positions** are `{ "x": …, "y": … }`, **screen areas** `{ "x": …, "y": …, "w": …, "h": … }`, both in physical pixels with the origin at the top-left of the primary monitor.

**Durations** are milliseconds and may be fractional.

**Colours** are `#RRGGBB`.

## Versioning

`version` is a single integer per format, currently `1` for both.

- A reader **rejects** files with a higher version than it knows, with a message asking the user to update.
- Within a version, changes are additive: new optional fields with defaults, new block types, new stroke types. Readers ignore fields they do not know.
- A change that old readers would misread bumps the version, and the new reader migrates old files on load.
