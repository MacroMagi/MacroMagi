"""Sound blocks. The app listens and does the analysis."""

from . import value_node


@value_node("sound.level")
def sound_level(run, node):
    return run.host.call("sound_level", params=node["params"])


@value_node("sound.match")
def sound_match(run, node):
    return run.host.call("sound_match", params=node["params"], node=node["id"])
