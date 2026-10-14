"""Input blocks: sending input and replaying recordings.

The actual pressing, clicking and timing is done by the app; these functions
only decide what to ask for.
"""

from . import flow_node

OUT = "out"


@flow_node("input.action")
def action(run, node):
    wired, position = run.input(node, "position")
    if wired and not _is_point(position):
        # The position was supposed to come from somewhere (say, an image that
        # was not found). Acting at the wrong place would be worse than not acting.
        run.log("No position available – input skipped", level="warning", node=node["id"])
        return OUT
    run.waited = True
    run.host.actions(node["params"], position if wired else None)
    return OUT


@flow_node("input.recording")
def recording(run, node):
    run.waited = True
    run.host.play_recording(node["params"])
    return OUT


def _is_point(value):
    return isinstance(value, dict) and "x" in value and "y" in value
