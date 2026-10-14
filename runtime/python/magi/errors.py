"""Exceptions used to unwind a run."""


class Stopped(Exception):
    """The run was asked to stop. Not an error: it just ends the run."""


class MacroError(Exception):
    """Something went wrong in a block. Ends the run and is shown to the user."""

    def __init__(self, message, node=None):
        super().__init__(message)
        self.node = node


class StopRun(Exception):
    """Raised by a Stop block to end the current run (or every run)."""

    def __init__(self, everything=False):
        super().__init__("stop")
        self.everything = everything
