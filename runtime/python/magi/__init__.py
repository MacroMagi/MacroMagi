"""The MacroMagi Python engine.

This package interprets macro graphs: it decides which block runs next,
evaluates conditions and runs Python Script blocks. It never touches the
keyboard, mouse or screen itself. Every such operation is a request to the
host application, which performs it with the same Rust code the native engine
uses. That keeps this package free of third-party dependencies (standard
library only) and guarantees both engines behave identically at the device
level.

Layout:
    host.py         message loop: receives commands from the app
    protocol.py     the JSON-lines channel and request/reply plumbing
    interpreter.py  walks a graph, one run per Start block
    nodes/          behaviour of each block type
    api.py          the `magi` object available inside Python Script blocks
"""

__version__ = "0.1.0"

# Bumped whenever a message changes shape. The app refuses to talk to a
# runtime that speaks a different protocol version.
PROTOCOL = 1
