# Adding a block

A block has three parts, each in its own place and all keyed by the same id:

1. a **definition** – what it is (`magi-core`);
2. **native behaviour** – what it does on the Rust engine (`magi-engine`);
3. **Python behaviour** – what it does on the Python engine (`runtime/python`).

There is no fourth step for the user interface. The editor draws the block from its definition and the inspector builds its settings form from its parameters.

This guide adds a **Compare** block: two numbers in, true/false out. The code is a worked example to adapt, not something already in the tree.

## 1. Define it

Pick an id. Ids are `category.name`, are written into users' files, and never change once released. Add a constant in [`crates/magi-core/src/nodes/mod.rs`](../crates/magi-core/src/nodes/mod.rs):

```rust
pub mod ids {
    // …
    pub const COMPARE: &str = "logic.compare";
}
```

Then describe the block in the module for its category – here [`nodes/logic.rs`](../crates/magi-core/src/nodes/logic.rs). Parameters get a typed struct, so the engine reads them without string lookups and the defaults exist in exactly one place:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareOp {
    #[default]
    Less,
    Equal,
    Greater,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompareParams {
    pub op: CompareOp,
}
```

Register the definition in that module's `register` function:

```rust
registry.register(
    NodeDef::new(ids::COMPARE, "Compare", Category::Logic, "True when A relates to B in the chosen way.")
        .keywords(&["greater", "less", "equal", "bigger", "smaller"])
        .data_in("a", "A", DataType::Number)
        .data_in("b", "B", DataType::Number)
        .data_out(ports::OUT, "Result", DataType::Bool)
        .param(ParamDef::new(
            "op",
            "Comparison",
            ParamKind::choice(&[("less", "A is less than B"), ("equal", "A equals B"), ("greater", "A is greater than B")]),
        ))
        .defaults(&CompareParams::default())
        .summarize(|node, _| {
            vec![match node.params_or_default::<CompareParams>().op {
                CompareOp::Less => "A < B".into(),
                CompareOp::Equal => "A = B".into(),
                CompareOp::Greater => "A > B".into(),
            }]
        }),
);
```

What the builder calls mean:

| Call | Effect |
| --- | --- |
| `.flow_in()` | Gives the block a flow input (top edge). Leave it out for value blocks. |
| `.flow_out(id, label)` | A flow output (bottom edge). Use `ports::OUT` with an empty label for the usual single output. |
| `.data_in` / `.data_out` | Value ports (left / right edge) with a type. |
| `.param(ParamDef::new(key, label, kind))` | A setting. `kind` chooses its editor – see `ParamKind`. Add `.help("…")` and `.show_if(key, value)` as needed. |
| `.defaults(&Params::default())` | Takes every setting's default from the typed struct. Panics at start-up if a setting is missing from it, so they cannot drift apart. |
| `.summarize(…)` | The lines of text on the block's face. The first is the headline. |
| `.python_only()` | Marks a block the native engine cannot run. |

Run the app. The block is already in the "add block" list, can be wired up, edited, saved and loaded. It just does nothing yet.

## 2. Native behaviour

In [`crates/magi-engine/src/native.rs`](../crates/magi-engine/src/native.rs), register a function in `NativeNodes::builtin`:

```rust
nodes.value(ids::COMPARE, compare);
```

and write it. A **value** block returns all of its outputs at once:

```rust
fn compare(interp: &mut Interp<'_>, node: &Node) -> OpResult<Map<String, Value>> {
    let p: CompareParams = params(node)?;
    // `input` returns None when nothing is wired to the port.
    let number = |value: Option<Value>| value.and_then(|v| v.as_f64()).unwrap_or(0.0);
    let a = number(interp.input(node, "a")?);
    let b = number(interp.input(node, "b")?);
    let result = match p.op {
        CompareOp::Less => a < b,
        CompareOp::Equal => a == b,
        CompareOp::Greater => a > b,
    };
    Ok(single(json!(result)))
}
```

A **flow** block is registered with `nodes.flow(…)` and returns what happens next – usually `Ok(Next::out())`, or `Ok(Next::Port("name".into()))` to pick an output:

```rust
fn delay(interp: &mut Interp<'_>, node: &Node) -> OpResult<Next> {
    let p: DelayParams = params(node)?;
    let ms = interp.cx.jittered(p.ms, p.jitter_ms);
    interp.cx.sleep_ms(ms)?;
    Ok(Next::out())
}
```

Rules that keep blocks well-behaved:

- **Anything that touches the machine goes through `interp.cx`** (`send`, `sleep`, `perform_actions`, `find_image`, …). Never call the OS from a block. If the operation you need does not exist, add it to [`ops.rs`](../crates/magi-engine/src/ops.rs) so the Python engine can use it too.
- **Wait with `interp.cx.sleep`**, never `std::thread::sleep`. It returns early when the run is stopped, and `?` then unwinds the block.
- **Return errors as messages a user can act on**: `Err(EngineError::new("no image has been chosen").into())`. The engine attaches the block.

## 3. Python behaviour

In the matching module under [`runtime/python/magi/nodes/`](../runtime/python/magi/nodes) – here `logic.py`:

```python
@value_node("logic.compare")
def compare(run, node):
    _, a = run.input(node, "a")        # (wired, value)
    _, b = run.input(node, "b")
    a, b = a or 0, b or 0
    op = node["params"]["op"]
    if op == "less":
        return {"out": a < b}
    if op == "equal":
        return {"out": a == b}
    return {"out": a > b}
```

`node["params"]` always contains every setting: MacroMagi fills in defaults before sending the macro to Python.

A flow block uses `@flow_node` and returns the name of the output to follow:

```python
@flow_node("flow.delay")
def delay(run, node):
    params = node["params"]
    run.sleep(params["ms"], params["jitter_ms"])
    return "out"
```

Device operations are requests to the app: `run.host.actions(…)`, `run.host.find_image(…)`, `run.sleep(…)`. Keep to the standard library and Python 3.8 syntax.

If you add a module, import it at the bottom of `nodes/__init__.py`.

## 4. Test it on both engines

Add a scenario to [`crates/magi-engine/tests/engines.rs`](../crates/magi-engine/tests/engines.rs). `on_all` runs it on every available engine and applies the same checks to each – this is what holds the two implementations to one behaviour:

```rust
#[test]
fn compare_orders_numbers() {
    let mut b = Builder::new();
    let image = add_target_asset(&mut b.project);
    let start = b.node(ids::START, json!({}));
    let find = b.node(ids::FIND_IMAGE, json!({ "image": image }));
    let compare = b.node(ids::COMPARE, json!({ "op": "greater" }));
    let decision = b.node(ids::DECISION, json!({}));
    let yes = b.tap(Key::KeyY);
    b.flow(start, ports::OUT, decision)
        .wire(find, "score", compare, "a") // similarity of the best match; B is unwired, so 0
        .wire(compare, "out", decision, "cond:1")
        .flow(decision, "case:1", yes);

    on_all(&b.project, start, || screen(Some(Point::new(40, 40))), |kind, outcome| {
        assert_eq!(outcome.taps(), [Key::KeyY], "{kind:?}");
    });
}
```

```bash
cargo test -p magi-engine --test engines
```

## 5. Finish

- Regenerate the block reference:

  ```bash
  cargo run -p magi-app -- nodes | python tools/node_docs.py > docs/NODES.md
  ```

- Give the block an icon in `node_icon` in [`crates/magi-app/src/ui/graph.rs`](../crates/magi-app/src/ui/graph.rs). Without one it gets a generic sparkle.
- Add a line to `CHANGELOG.md`.

## When a block needs more

**A new kind of setting.** Add a variant to `ParamKind` in `magi-core` and one arm to `edit_param` in [`ui/inspector.rs`](../crates/magi-app/src/ui/inspector.rs). Every block can then use it.

**Ports that depend on settings.** See how Decision does it: `DynamicPorts::Cases` in the definition, resolved in `NodeDef::ports`.

**Something drawn on the block itself**, like the thumbnail on Image Detection or the card on Play Recording. That is the one case with UI code: see `Layout::compute` and `GraphView::show` in `ui/graph.rs`.

**A new capability** (clipboard, sound, …). Add a method to the `Host` trait in `magi-core` (with a default that reports it as unsupported), implement it for `SystemHost` and `SimHost`, add an operation to `ops.rs` and to `perform` in `python.rs`, then write the block on top.
