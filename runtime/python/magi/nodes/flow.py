"""Flow blocks: start, wait, repeat, decide, stop, log."""

import time

from ..errors import StopRun
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
    # Arriving while our own frame is on top means the looped branch came back.
    returning = bool(run.loops) and run.loops[-1][0] == node["id"]
    if not returning:
        remaining = None if params["mode"] == "forever" else int(params["count"])
        run.loops.append([node["id"], remaining])
    frame = run.loops[-1]
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
    raise StopRun(everything=node["params"]["scope"] == "all")


@flow_node("flow.log")
def log(run, node):
    message = node["params"]["message"]
    wired, value = run.input(node, "value")
    if wired:
        message = "%s %s" % (message, value if isinstance(value, str) else _json(value))
    run.log(message.strip(), node=node["id"])
    return OUT


def _json(value):
    import json

    return json.dumps(value, separators=(",", ":"))
