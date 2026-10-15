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
    result = scope.get("result")
    run.outputs[node["id"]] = {"result": result}
    variable = (node["params"].get("result_variable") or "").strip()
    if variable:
        run.host.call("var_set", name=variable, value=_plain(result))
    return "out"


def _plain(value):
    """A value that can be stored in a variable: JSON-like data, or its text."""
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    if isinstance(value, (list, tuple)):
        return [_plain(v) for v in value]
    if isinstance(value, dict):
        return {str(k): _plain(v) for k, v in value.items()}
    return str(value)


def _line_of(error):
    """Finds the line of the user's script an exception came from."""
    trace = error.__traceback__
    line = None
    while trace is not None:
        if trace.tb_frame.f_code.co_filename.startswith("<Python Script"):
            line = trace.tb_lineno
        trace = trace.tb_next
    return "line %s: " % line if line is not None else ""
