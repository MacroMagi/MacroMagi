"""Behaviour of each block type, keyed by the same ids as the app's node definitions.

A flow block is a function `(run, node) -> name of the flow output to follow`
(or None to end the branch). A value block is a function
`(run, node) -> dict of output values`.

To add a block, write the function in the fitting module and decorate it:

    @flow_node("flow.delay")
    def delay(run, node):
        run.sleep(node["params"]["ms"])
        return "out"
"""

FLOW = {}
VALUE = {}


# The decorators are deliberately not called `flow` and `value`: a package
# attribute with the name of a submodule would stop that submodule from loading.
def flow_node(node_type):
    def register(function):
        FLOW[node_type] = function
        return function

    return register


def value_node(node_type):
    def register(function):
        VALUE[node_type] = function
        return function

    return register


# Importing the modules runs their decorators, which fills the tables above.
from . import flow as _flow  # noqa: E402,F401
from . import input as _input  # noqa: E402,F401
from . import logic as _logic  # noqa: E402,F401
from . import script as _script  # noqa: E402,F401
from . import vision as _vision  # noqa: E402,F401
from . import window as _window  # noqa: E402,F401
