"""Vision blocks. The app captures the screen and does the matching."""

from . import value_node


@value_node("vision.find_image")
def find_image(run, node):
    return run.host.find_image(node["params"])


@value_node("vision.read_text")
def read_text(run, node):
    return run.host.call("read_text", params=node["params"])


@value_node("vision.find_color")
def find_color(run, node):
    return run.host.find_color(node["params"])
