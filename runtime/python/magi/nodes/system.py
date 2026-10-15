"""System blocks: the clipboard and notifications, provided by the app."""

from . import flow_node, value_node


@value_node("system.clipboard")
def clipboard(run, node):
    return {"out": run.host.call("clipboard")}


@flow_node("system.notify")
def notify(run, node):
    wired, value = run.input(node, "value")
    if wired:
        run.host.call("notify", params=node["params"], value=value, node=node["id"])
    else:
        run.host.call("notify", params=node["params"], node=node["id"])
    return "out"
