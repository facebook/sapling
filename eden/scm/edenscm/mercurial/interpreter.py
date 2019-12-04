# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Emulating the Python interpreter
#
# The emulated python interpreter:
# - Can import modules (including native ones) that can be imported here.
# - Support simple command line flags like "-m", "-c", etc.
# - Do not go through the default entry point (mercurial.dispatch).
# This is useful for testing when the main hg program is built into a single
# binary that always goes through the default entry point.

from __future__ import absolute_import

import os
import sys

from . import encoding, pycompat


if __name__ == "__main__":
    argv = sys.argv
    # PYTHONPATH is not always respected by a "python binary" wrapper.
    # Also respect HGPYTHONPATH.
    # pyre-fixme[16]: Optional type has no attribute `get`.
    sys.path.extend(encoding.environ.get("PYTHONPATH", "").split(pycompat.ospathsep))
    sys.path[0:0] = encoding.environ.get("HGPYTHONPATH", "").split(pycompat.ospathsep)

    # Silent warnings like "ImportWarning: Not importing ..."
    import warnings

    warnings.filterwarnings("ignore")

    if len(argv) >= 2 and os.path.exists(argv[1]):
        # python FILE ...
        globalvars = globals()
        globalvars.update(
            {"__file__": os.path.realpath(argv[1]), "__name__": "__main__"}
        )
        # Make it use this script as the interpreter again
        sys.executable = argv[0]
        sys.argv = argv[1:]
        exec(open(argv[1]).read(), globalvars)
    elif len(argv) == 3 and argv[1] == "-m":
        # python -m MODULE ...
        # This includes cases like "-m heredoctest" used by run-tests.py
        __import__(argv[2])
    elif len(argv) >= 3 and argv[1] == "-c":
        # python -c COMMND ...
        content = argv[2]
        sys.argv = sys.argv[0:1] + argv[3:]
        exec(content)
    elif len(argv) == 1:
        # python << EOF
        # Read from stdin
        content = sys.stdin.read()
        exec(content)
