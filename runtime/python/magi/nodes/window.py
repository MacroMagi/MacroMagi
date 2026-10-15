"""Window blocks. The app finds and activates windows."""

from . import flow_node


@flow_node("window.focus")
def focus_window(run, node):
    run.waited = True
    found = run.host.call("focus_window", params=node["params"])
    run.outputs[node["id"]] = {"screen": found["kind"] if found else None}
    if found is None:
        run.log("No window matching “%s” was found" % node["params"]["target"], level="warning", node=node["id"])
        return "missing"
    return "out"
