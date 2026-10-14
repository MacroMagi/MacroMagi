"""Entry point of the MacroMagi Python engine.

The desktop app starts this script and talks to it over stdin/stdout.
It is not meant to be run by hand; see docs/PYTHON_ENGINE.md.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from magi.host import main  # noqa: E402  (the path must be set up first)

if __name__ == "__main__":
    main()
