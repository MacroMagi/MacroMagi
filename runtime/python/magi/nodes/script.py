"""The Python Script block: runs user code as a step of the macro."""

from ..api import Magi
from ..errors import MacroError, Stopped, StopRun
from . import flow_node


@flow_node("script.python")
def python(run, node):
    code = node["params"]["code"]
    _, value = run.input(node, "value")
    scope = {"magi": Magi(run, node["id"]), "value": value, "result": None}
    try:
        compiled = compile(code, "<Python Script #%s>" % node["id"], "exec")
        exec(compiled, scope)  # noqa: S102 - running the user's own script is the point
    except (Stopped, StopRun, MacroError):
        raise
    except SyntaxError as error:
        raise MacroError("line %s: %s" % (error.lineno, error.msg))
    except Exception as error:
        raise MacroError("%s%s: %s" % (_line_of(error), type(error).__name__, error))
    run.outputs[node["id"]] = {"result": scope.get("result")}
    return "out"


def _line_of(error):
    """Finds the line of the user's script an exception came from."""
    trace = error.__traceback__
    line = None
    while trace is not None:
        if trace.tb_frame.f_code.co_filename.startswith("<Python Script"):
            line = trace.tb_lineno
        trace = trace.tb_next
    return "line %s: " % line if line is not None else ""
