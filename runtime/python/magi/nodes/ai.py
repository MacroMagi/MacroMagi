"""AI blocks. The app runs the models; these only ask it to."""

from . import flow_node, value_node


@flow_node("ai.watch")
def ai_watch(run, node):
    """Each time the model acts, the blocks under "acted" run as a loop round,
    then the block is entered again and goes on watching."""
    run.waited = True
    returning = bool(run.loops) and run.loops[-1][0] == node["id"]
    result = run.host.call("ai_watch", params=node["params"], node=node["id"], returning=returning)
    run.outputs[node["id"]] = {"hits": result["hits"], "label": result["label"]}
    if result["acted"]:
        if not returning:
            run.loops.append([node["id"], None])
        return "acted"
    if returning:
        run.loops.pop()
    return "out"


@value_node("ai.classify")
def ai_classify(run, node):
    return run.host.call("ai_classify", params=node["params"])
