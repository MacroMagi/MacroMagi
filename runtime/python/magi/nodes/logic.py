"""Logic blocks: combining and producing true/false values."""

import random

from . import value_node


@value_node("logic.gate")
def gate(run, node):
    op = node["params"]["op"]
    a = run.input_bool(node, "a")
    if op == "not":
        return {"out": not a}
    b = run.input_bool(node, "b")
    if op == "and":
        return {"out": a and b}
    if op == "or":
        return {"out": a or b}
    return {"out": a != b}  # xor


@value_node("logic.chance")
def chance(run, node):
    return {"out": random.random() < node["params"]["probability"]}


def _inputs(run, node, ports):
    """The wired inputs among `ports`, as keyword arguments for an operation."""
    wired = {}
    for port in ports:
        is_wired, value = run.input(node, port)
        if is_wired:
            wired[port] = value
    return wired


# The calculations themselves are done by the app, so both engines give the
# same answer to the same question.
@value_node("logic.math")
def math(run, node):
    return {"out": run.host.call("math", params=node["params"], **_inputs(run, node, ("a", "b")))}


@value_node("logic.compare")
def compare(run, node):
    wired = _inputs(run, node, ("a", "b"))
    wired.setdefault("a", None)
    return {"out": run.host.call("compare", params=node["params"], **wired)}


@value_node("logic.text")
def text(run, node):
    return {"out": run.host.call("text", params=node["params"], **_inputs(run, node, ("a", "b")))}
