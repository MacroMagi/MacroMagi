"""The `magi` object available inside Python Script blocks.

Everything here ends up as a request to the app, so scripts send input and
read the screen exactly like the built-in blocks do. See docs/PYTHON_ENGINE.md
for the user-facing reference.
"""

import collections

from .errors import MacroError, StopRun

Sighting = collections.namedtuple("Sighting", ["found", "x", "y", "score"])
Sighting.__doc__ = "Result of looking for something on screen. `x`/`y` are None when not found."
Sighting.__bool__ = lambda self: bool(self.found)

_KEY_ALIASES = {
    "ctrl": "ControlLeft", "control": "ControlLeft", "lctrl": "ControlLeft", "rctrl": "ControlRight",
    "shift": "ShiftLeft", "lshift": "ShiftLeft", "rshift": "ShiftRight",
    "alt": "AltLeft", "lalt": "AltLeft", "ralt": "AltRight", "altgr": "AltRight",
    "win": "MetaLeft", "cmd": "MetaLeft", "command": "MetaLeft", "super": "MetaLeft", "meta": "MetaLeft",
    "enter": "Enter", "return": "Enter", "space": "Space", " ": "Space", "tab": "Tab",
    "esc": "Escape", "escape": "Escape", "backspace": "Backspace", "delete": "Delete", "del": "Delete",
    "insert": "Insert", "ins": "Insert", "home": "Home", "end": "End",
    "pageup": "PageUp", "pgup": "PageUp", "pagedown": "PageDown", "pgdn": "PageDown",
    "up": "ArrowUp", "down": "ArrowDown", "left": "ArrowLeft", "right": "ArrowRight",
    "capslock": "CapsLock", "numlock": "NumLock", "scrolllock": "ScrollLock",
    "printscreen": "PrintScreen", "pause": "Pause", "menu": "ContextMenu",
    "-": "Minus", "=": "Equal", "[": "BracketLeft", "]": "BracketRight", "\\": "Backslash",
    ";": "Semicolon", "'": "Quote", "`": "Backquote", ",": "Comma", ".": "Period", "/": "Slash",
}

_BUTTONS = {"left": "Left", "right": "Right", "middle": "Middle", "back": "Back", "forward": "Forward"}


def key_id(name):
    """Turns a friendly key name ("a", "ctrl", "F5", "enter") into a MacroMagi key id."""
    text = str(name)
    lowered = text.lower()
    if lowered in _KEY_ALIASES:
        return _KEY_ALIASES[lowered]
    if len(text) == 1 and text.isascii():
        if text.isalpha():
            return "Key" + text.upper()
        if text.isdigit():
            return "Digit" + text
    if lowered.startswith("f") and lowered[1:].isdigit():
        return "F" + lowered[1:]
    return text  # assume it already is an id such as "KeyA" or "Numpad5"


def button_id(name):
    try:
        return _BUTTONS[str(name).lower()]
    except KeyError:
        raise MacroError("unknown mouse button %r (use left, right, middle, back or forward)" % (name,))


def _region(region):
    if region is None:
        return None
    x, y, w, h = region
    return {"x": int(x), "y": int(y), "w": int(w), "h": int(h)}


class Magi:
    """Scripting API bound to the run that reached the script block."""

    def __init__(self, run, node_id):
        self._run = run
        self._node = node_id

    # -- timing and control -------------------------------------------------

    def sleep(self, ms):
        """Wait for a number of milliseconds. Ends immediately if the macro is stopped."""
        self._run.sleep(float(ms))

    def stop(self):
        """End this Start block's run."""
        raise StopRun(everything=False)

    def stop_all(self):
        """End every running Start block of the macro."""
        raise StopRun(everything=True)

    @property
    def stopped(self):
        """True once the macro has been asked to stop. Check it in long loops."""
        return self._run.stop_requested.is_set()

    @property
    def vars(self):
        """The macro's variables: magi.vars["delay"] reads one, magi.vars["delay"] = 50
        changes it. Every block bound to a variable follows its new value."""
        return Variables(self._run)

    def log(self, *values):
        """Write a line to the MacroMagi console."""
        self._run.log(" ".join(str(v) for v in values), node=self._node)

    # -- keyboard -----------------------------------------------------------

    def press(self, *keys, hold_ms=30):
        """Tap one key, or several together: magi.press("ctrl", "c")."""
        actions = [{"kind": "key", "key": key_id(k), "mode": "tap"} for k in keys]
        self._actions(actions, hold_ms)

    def key_down(self, key):
        self._run.host.send({"type": "key_down", "key": key_id(key)})

    def key_up(self, key):
        self._run.host.send({"type": "key_up", "key": key_id(key)})

    def type(self, text):
        """Type text, independent of the keyboard layout."""
        self._run.host.send({"type": "text", "text": str(text)})

    # -- mouse --------------------------------------------------------------

    def click(self, x=None, y=None, button="left", clicks=1, hold_ms=30):
        """Click where the pointer is, or at (x, y)."""
        action = {"kind": "mouse", "button": button_id(button), "mode": "tap", "clicks": int(clicks)}
        if x is not None and y is not None:
            action["at"] = {"x": int(x), "y": int(y)}
        self._actions([action], hold_ms)

    def mouse_down(self, button="left"):
        self._run.host.send({"type": "button_down", "button": button_id(button)})

    def mouse_up(self, button="left"):
        self._run.host.send({"type": "button_up", "button": button_id(button)})

    def move(self, x, y, relative=False, duration_ms=0):
        """Move the pointer. With duration_ms it glides instead of jumping."""
        self._run.waited = self._run.waited or duration_ms > 0
        self._run.host.move(int(x), int(y), bool(relative), int(duration_ms))

    def scroll(self, dy, dx=0):
        """Scroll by wheel notches. Positive dy scrolls up."""
        self._run.host.send({"type": "scroll", "dx": int(dx), "dy": int(dy)})

    def cursor(self):
        """The pointer position as (x, y)."""
        position = self._run.host.cursor()
        return position["x"], position["y"]

    # -- screen -------------------------------------------------------------

    def find_image(self, image, region=None, threshold=0.8):
        """Look for one of the macro's images (by name) on screen.

        `region` is (x, y, width, height); None searches the whole screen.
        Returns a Sighting, which is truthy when the image was found.
        """
        params = {
            "image": self._run.program.asset_id(image, "image"),
            "region": _region(region),
            "threshold": float(threshold),
        }
        return _sighting(self._run.host.find_image(params))

    def find_color(self, color, tolerance=16, region=None):
        """Look for a colour such as "#FF8800" on screen."""
        params = {"color": str(color), "tolerance": int(tolerance), "region": _region(region)}
        return _sighting(self._run.host.find_color(params))

    # -- helpers ------------------------------------------------------------

    def _actions(self, actions, hold_ms):
        self._run.waited = True
        self._run.host.actions({"actions": actions, "hold_ms": float(hold_ms)}, None)


def _sighting(result):
    position = result.get("position") or {}
    return Sighting(bool(result.get("found")), position.get("x"), position.get("y"), result.get("score", 0.0))


class Variables:
    """The macro's variables, kept by the app and shared by every run."""

    def __init__(self, run):
        self._run = run

    def __getitem__(self, name):
        return self._run.host.call("var_get", name=name)

    def __setitem__(self, name, value):
        self._run.host.call("var_set", name=name, value=value)

    def get(self, name, default=None):
        value = self[name]
        return default if value is None else value

    def __contains__(self, name):
        return name in self._run.host.call("vars")

    def keys(self):
        return list(self._run.host.call("vars").keys())

    def items(self):
        return list(self._run.host.call("vars").items())

    def __repr__(self):
        return repr(self._run.host.call("vars"))
