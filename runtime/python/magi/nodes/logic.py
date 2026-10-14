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
