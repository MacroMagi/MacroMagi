"""The engine's message loop: receives commands from the app and runs macros."""

import io
import json
import os
import platform
import sys
import threading

from . import PROTOCOL
from .interpreter import Program, Run
from .protocol import Channel


# Which run the current thread belongs to, so print() output can be attributed.
_current = threading.local()


class _LogStream(io.TextIOBase):
    """Stands in for sys.stdout so that print() in scripts reaches the console.

    The real stdout carries the protocol; a stray print would corrupt it.
    """

    def __init__(self, channel, level):
        super().__init__()
        self._channel = channel
        self._level = level
        self._buffer = ""
        self._lock = threading.Lock()

    def writable(self):
        return True

    def write(self, text):
        with self._lock:
            self._buffer += text
            *lines, self._buffer = self._buffer.split("\n")
        run = getattr(_current, "run", None)
        for line in lines:
            if line.strip():
                self._channel.send({"t": "log", "run": run, "node": None, "level": self._level, "message": line})
        return len(text)


class Engine:
    def __init__(self, channel):
        self.channel = channel
        self.programs = {}
        self.runs = {}
        self.lock = threading.Lock()

    def handle(self, message):
        kind = message.get("t")
        if kind == "reply":
            self.channel.deliver(message)
        elif kind == "load":
            program = Program(message["data"])
            with self.lock:
                self.programs[message["program"]] = program
        elif kind == "unload":
            with self.lock:
                self.programs.pop(message["program"], None)
        elif kind == "start":
            self.start(message["program"], message["run"], message["node"])
        elif kind == "stop":
            with self.lock:
                run = self.runs.get(message["run"])
            if run is not None:
                run.stop_requested.set()
        elif kind == "shutdown":
            return False
        return True

    def start(self, program_id, run_id, node):
        with self.lock:
            program = self.programs.get(program_id)
        if program is None:
            self.channel.send({"t": "done", "run": run_id, "error": "the macro is not loaded", "node": None})
            return
        run = Run(program, run_id, self.channel)
        with self.lock:
            self.runs[run_id] = run

        def body():
            _current.run = run_id
            error, error_node = run.execute(node)
            with self.lock:
                self.runs.pop(run_id, None)
            self.channel.send({"t": "done", "run": run_id, "error": error, "node": error_node})

        threading.Thread(target=body, name="magi-run-%s" % run_id, daemon=True).start()


def main():
    # The protocol is UTF-8 on the raw byte streams, whatever the console's encoding.
    raw_in = sys.stdin.buffer
    raw_out = sys.stdout.buffer

    def write_line(line):
        raw_out.write(line.encode("utf-8") + b"\n")
        raw_out.flush()

    channel = Channel(write_line)
    sys.stdout = _LogStream(channel, "info")
    sys.stderr = _LogStream(channel, "warning")

    engine = Engine(channel)
    channel.send({"t": "ready", "python": platform.python_version(), "protocol": PROTOCOL})

    for line in raw_in:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line.decode("utf-8"))
        except ValueError:
            continue  # not for us; ignore rather than die
        try:
            if not engine.handle(message):
                break
        except Exception as error:  # never let one bad message kill the engine
            channel.send({"t": "log", "run": None, "node": None, "level": "error",
                          "message": "engine error: %s: %s" % (type(error).__name__, error)})

    # The app went away (or asked us to leave). Run threads are daemons and
    # may be blocked waiting for replies that will never come: exit outright.
    channel.close()
    os._exit(0)
