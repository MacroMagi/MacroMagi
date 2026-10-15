"""Variable blocks. The variables themselves live in the app, shared by every
run of the macro and by both engines; these functions only ask for changes."""

from . import flow_node, value_node

OUT = "out"


@value_node("var.get")
def variable(run, node):
    name = (node["params"].get("variable") or "").strip()
    return {OUT: run.host.call("var_get", name=name)}


@flow_node("var.set")
def set_variable(run, node):
    wired, value = run.input(node, "value")
    if wired:
        run.host.call("set_variable", params=node["params"], value=value)
    else:
        run.host.call("set_variable", params=node["params"])
    return OUT


@flow_node("var.counter")
def counter(run, node):
    answer = run.host.call("counter", params=node["params"])
    run.outputs[node["id"]] = {"count": answer["count"]}
    return "reached" if answer["reached"] else OUT
