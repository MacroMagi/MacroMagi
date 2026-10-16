"""AI blocks. The app runs the models; these only ask it to."""

from . import flow_node, value_node


@flow_node("ai.watch")
def ai_watch(run, node):
    run.waited = True
    result = run.host.call("ai_watch", params=node["params"], node=node["id"])
    run.outputs[node["id"]] = {"hits": result["hits"], "label": result["label"]}
    return "out"


@value_node("ai.classify")
def ai_classify(run, node):
    return run.host.call("ai_classify", params=node["params"])
