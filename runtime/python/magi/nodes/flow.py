"""Flow blocks: start, wait, repeat, decide, stop, log, rate, definitions and functions."""

import time

from ..errors import EndBranch, ExitLoop, GoTo, MacroError, StopRun
from . import flow_node

OUT = "out"


@flow_node("flow.start")
def start(run, node):
    return OUT


@flow_node("flow.delay")
def delay(run, node):
    params = node["params"]
    run.sleep(params["ms"], params["jitter_ms"])
    return OUT


@flow_node("flow.repeat")
def repeat(run, node):
    params = node["params"]
    mode = params["mode"]
    # Arriving while our own frame is on top means the looped branch came back.
    returning = bool(run.loops) and run.loops[-1][0] == node["id"]
    if not returning:
        remaining = int(params["count"]) if mode == "count" else None
        run.loops.append([node["id"], remaining])
    # A condition is checked before every round, the first included.
    if mode == "until":
        ends = run.input_bool(node, "condition")
    elif mode == "while":
        ends = not run.input_bool(node, "condition")
    else:
        ends = False
    frame = run.loops[-1]
    if ends:
        run.loops.pop()
        return "done"
    if frame[1] is None:
        return "loop"
    if frame[1] <= 0:
        run.loops.pop()
        return "done"
    frame[1] -= 1
    return "loop"


@flow_node("flow.decision")
def decision(run, node):
    for case in node["params"]["cases"]:
        if run.input_bool(node, "cond:" + case["id"]):
            return "case:" + case["id"]
    return "else"


@flow_node("flow.wait_until")
def wait_until(run, node):
    params = node["params"]
    started = time.monotonic()
    while True:
        run.memo.clear()  # look again, don't reuse the last answer
        if run.input_bool(node, "condition"):
            return OUT
        timeout = params["timeout_ms"]
        if timeout > 0 and (time.monotonic() - started) * 1000.0 >= timeout:
            return "timeout"
        run.sleep(max(params["poll_ms"], 1.0))


@flow_node("flow.stop")
def stop(run, node):
    params = node["params"]
    scope = params["scope"]
    if scope == "next":
        raise EndBranch()
    if scope == "exit_loop":
        raise ExitLoop()
    if scope == "go_to":
        if params.get("target") is None:
            raise MacroError("no block to continue from has been chosen")
        raise GoTo(params["target"])
    raise StopRun(everything=scope == "all")


@flow_node("flow.log")
def log(run, node):
    message = node["params"]["message"]
    wired, value = run.input(node, "value")
    if wired:
        message = "%s %s" % (message, as_text(value))
    run.log(message.strip(), node=node["id"])
    return OUT


def as_text(value):
    """How a value reads as text. Matches the app (magi_core::variables::as_text)."""
    import json

    if value is None:
        return ""
    if isinstance(value, str):
        return value
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        return ("%.6f" % value).rstrip("0").rstrip(".")
    return json.dumps(value, separators=(",", ":"))


@flow_node("flow.define")
def define(run, node):
    return OUT


@flow_node("flow.rate")
def rate(run, node):
    wired, value = run.input(node, "rate")
    run.waited = True
    if wired:
        run.host.call("rate", params=node["params"], rate=value)
    else:
        run.host.call("rate", params=node["params"])
    return OUT


@flow_node("flow.call")
def call(run, node):
    wired, value = run.input(node, "input")
    run.call_definition(node["params"]["definition"], wired, value)
    return OUT
