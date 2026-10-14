"""The channel between this process and the MacroMagi app.

Messages are JSON objects, one per line, with a `t` field naming their type.

App -> engine
    {"t": "load",   "program": 1, "data": {"graph": ..., "assets": [...]}}
    {"t": "unload", "program": 1}
    {"t": "start",  "program": 1, "run": 7, "node": 3}
    {"t": "stop",   "run": 7}
    {"t": "reply",  "id": 12, "ok": true, "value": ...}
    {"t": "reply",  "id": 12, "ok": false, "stopped": true}
    {"t": "reply",  "id": 12, "ok": false, "error": "..."}
    {"t": "shutdown"}

Engine -> app
    {"t": "ready",    "python": "3.12.1", "protocol": 1}
    {"t": "call",     "id": 12, "run": 7, "op": "sleep", "args": {"ms": 500}}
    {"t": "enter",    "run": 7, "node": 3}
    {"t": "log",      "run": 7, "node": 3, "level": "info", "message": "..."}
    {"t": "stop_all", "run": 7}
    {"t": "done",     "run": 7, "error": null, "node": null}
"""

import json
import threading

from .errors import MacroError, Stopped


class _Pending:
    __slots__ = ("event", "reply")

    def __init__(self):
        self.event = threading.Event()
        self.reply = None


class Channel:
    """Thread-safe message writer plus blocking request/reply calls."""

    def __init__(self, write_line):
        # write_line(str) must write one line and flush.
        self._write_line = write_line
        self._write_lock = threading.Lock()
        self._pending = {}
        self._pending_lock = threading.Lock()
        self._next_id = 1
        self._closed = False

    def send(self, message):
        line = json.dumps(message, separators=(",", ":"), ensure_ascii=False)
        with self._write_lock:
            self._write_line(line)

    def call(self, run, op, args):
        """Ask the app to perform a device operation and wait for the result."""
        pending = _Pending()
        with self._pending_lock:
            if self._closed:
                raise Stopped()
            call_id = self._next_id
            self._next_id += 1
            self._pending[call_id] = pending
        self.send({"t": "call", "id": call_id, "run": run, "op": op, "args": args})
        pending.event.wait()
        reply = pending.reply
        if reply is None or reply.get("stopped"):
            raise Stopped()
        if not reply.get("ok"):
            raise MacroError(reply.get("error") or "the operation failed")
        return reply.get("value")

    def deliver(self, reply):
        """Hand a reply from the app to the thread waiting for it."""
        with self._pending_lock:
            pending = self._pending.pop(reply.get("id"), None)
        if pending is not None:
            pending.reply = reply
            pending.event.set()

    def close(self):
        """Wake every waiting call; they will raise Stopped."""
        with self._pending_lock:
            self._closed = True
            waiting = list(self._pending.values())
            self._pending.clear()
        for pending in waiting:
            pending.event.set()
