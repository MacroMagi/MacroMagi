"""Walks a macro graph.

This mirrors the native Rust interpreter (crates/magi-engine/src/native.rs)
step for step; when you change behaviour in one, change the other. The
contract both follow is described in docs/ARCHITECTURE.md ("Execution model").
"""

import threading

from . import nodes
from .errors import MacroError, Stopped, StopRun

FLOW_IN = "in"
OUT = "out"

# Value blocks may feed value blocks; this bounds the chain.
MAX_VALUE_DEPTH = 64
# After this many steps without waiting, yield so a tight loop cannot pin a core.
BUSY_STEPS = 2000


class Program:
    """An immutable macro: nodes, wires and the names of its assets."""

    def __init__(self, data):
        graph = data["graph"]
        self.nodes = {node["id"]: node for node in graph["nodes"]}
        # A flow output leads to one node; a data input has one source.
        self.flow_targets = {}
        self.data_sources = {}
        for wire in graph.get("wires", []):
            source, target = wire["from"], wire["to"]
            if target["port"] == FLOW_IN:
                self.flow_targets[(source["node"], source["port"])] = target["node"]
            else:
                self.data_sources[(target["node"], target["port"])] = (source["node"], source["port"])
        self.assets = data.get("assets", [])
        # State shared by all Python Script blocks of this program (`magi.vars`).
        self.variables = {}
        self.variables_lock = threading.RLock()

    def asset_id(self, name_or_id, kind):
        """Resolves an asset given by id or by display name."""
        for asset in self.assets:
            if asset["id"] == name_or_id and asset["kind"] == kind:
                return asset["id"]
        for asset in self.assets:
            if asset["name"] == name_or_id and asset["kind"] == kind:
                return asset["id"]
        raise MacroError("there is no %s named %r in this macro" % (kind, name_or_id))


class Host:
    """Device operations, performed by the app on this run's behalf."""

    def __init__(self, channel, run_id):
        self._channel = channel
        self._run = run_id

    def call(self, op, **args):
        return self._channel.call(self._run, op, args)

    def sleep(self, ms, jitter_ms=0.0):
        self.call("sleep", ms=ms, jitter_ms=jitter_ms)

    def actions(self, params, position=None):
        self.call("actions", params=params, position=position)

    def play_recording(self, params):
        self.call("play_recording", params=params)

    def find_image(self, params):
        return self.call("find_image", params=params)

    def find_color(self, params):
        return self.call("find_color", params=params)

    def send(self, event):
        self.call("send", event=event)

    def move(self, x, y, relative=False, duration_ms=0):
        self.call("move", x=x, y=y, relative=relative, duration_ms=duration_ms)

    def cursor(self):
        return self.call("cursor")


class Run:
    """One execution of the flow that begins at a Start block."""

    def __init__(self, program, run_id, channel):
        self.program = program
        self.id = run_id
        self.channel = channel
        self.host = Host(channel, run_id)
        self.stop_requested = threading.Event()
        # Repeat blocks being executed: [node id, iterations left or None].
        self.loops = []
        # Outputs of value blocks evaluated during the current step.
        self.memo = {}
        # Outputs of flow blocks (e.g. a script's result), kept between steps.
        self.outputs = {}
        self.depth = 0
        # Set by anything that waits; lets the loop below spot busy spinning.
        self.waited = False

    # -- reporting ---------------------------------------------------------

    def log(self, message, level="info", node=None):
        self.channel.send({"t": "log", "run": self.id, "node": node, "level": level, "message": str(message)})

    def check_stop(self):
        if self.stop_requested.is_set():
            raise Stopped()

    def sleep(self, ms, jitter_ms=0.0):
        self.waited = True
        self.host.sleep(ms, jitter_ms)

    # -- execution ---------------------------------------------------------

    def execute(self, start):
        """Runs to completion. Returns (error message or None, node id or None)."""
        try:
            self._run_from(start)
        except Stopped:
            pass
        except StopRun as stop:
            if stop.everything:
                self.channel.send({"t": "stop_all", "run": self.id})
        except MacroError as error:
            return str(error), error.node
        except Exception as error:  # a bug in a block or in user code
            return "%s: %s" % (type(error).__name__, error), getattr(error, "node", None)
        return None, None

    def _run_from(self, start):
        program = self.program
        current = start
        busy_steps = 0
        while True:
            self.check_stop()
            if current is None:
                # The branch ran out. Inside a Repeat that means "next iteration".
                if not self.loops:
                    return
                current = self.loops[-1][0]
                continue

            if self.waited:
                self.waited = False
                busy_steps = 0
            else:
                busy_steps += 1
                if busy_steps >= BUSY_STEPS:
                    busy_steps = 0
                    self.sleep(1)

            node = program.nodes.get(current)
            if node is None:
                raise MacroError("block #%s no longer exists" % current)
            kind = node["type"]

            if not node.get("enabled", True):
                # A disabled block is skipped: execution passes straight through it.
                current = program.flow_targets.get((current, node.get("pass_through", OUT)))
                continue

            self.memo.clear()
            self.channel.send({"t": "enter", "run": self.id, "node": current})
            behaviour = nodes.FLOW.get(kind)
            if behaviour is None:
                raise MacroError("“%s” cannot be run by the Python engine" % kind, current)
            try:
                port = behaviour(self, node)
            except MacroError as error:
                if error.node is None:
                    error.node = current
                raise
            except (Stopped, StopRun):
                raise
            except Exception as error:
                raise MacroError("%s: %s" % (type(error).__name__, error), current)
            current = program.flow_targets.get((current, port)) if port is not None else None

    # -- data --------------------------------------------------------------

    def input(self, node, port):
        """Reads a data input. Returns (wired, value)."""
        source = self.program.data_sources.get((node["id"], port))
        if source is None:
            return False, None
        source_id, source_port = source
        source_node = self.program.nodes.get(source_id)
        if source_node is None:
            raise MacroError("block #%s no longer exists" % source_id)
        if not source_node.get("enabled", True):
            return True, None
        if source_id in self.outputs:
            return True, self.outputs[source_id].get(source_port)
        if source_id not in self.memo:
            evaluate = nodes.VALUE.get(source_node["type"])
            if evaluate is None:
                # A flow block that has not run yet, or an unknown block.
                return True, None
            if self.depth >= MAX_VALUE_DEPTH:
                raise MacroError("values are wired in a loop", source_id)
            self.channel.send({"t": "enter", "run": self.id, "node": source_id})
            self.depth += 1
            try:
                self.memo[source_id] = evaluate(self, source_node)
            except MacroError as error:
                if error.node is None:
                    error.node = source_id
                raise
            finally:
                self.depth -= 1
        return True, self.memo[source_id].get(source_port)

    def input_bool(self, node, port):
        """Reads a data input as true/false. Unwired inputs are false."""
        wired, value = self.input(node, port)
        return wired and truthy(value)


def truthy(value):
    """How values are read as true/false. Matches the native engine."""
    if value is None:
        return False
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return value != 0
    if isinstance(value, str):
        return value != ""
    return True
