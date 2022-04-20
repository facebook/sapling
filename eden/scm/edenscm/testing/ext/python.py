# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""python extension for TestTmp

- provides a "python" command
"""

import traceback
from typing import BinaryIO, List

from ..sh import Env
from ..t.runtime import TestTmp
from ..t.shext import shellenv


def testsetup(t: TestTmp):
    t.command(python)
    t.setenv("PYTHON", "python")


def python(
    args: List[str], stdin: BinaryIO, stdout: BinaryIO, stderr: BinaryIO, env: Env
) -> int:
    # cannot use 'hg debugpython - ...' - it is one-time (Py_Main -> Py_Finalize)
    # emulate Py_Main instead
    if not args:
        # python << EOF ... EOF
        code = stdin.read()
    else:
        # python a.py arg1 arg2 ...
        with env.fs.open(args[0], "rb") as f:
            code = f.read()
    env.args = args
    with shellenv(env, stdin=stdin, stdout=stdout, stderr=stderr):
        code = compile(code, args and args[0] or "<stdin>", "exec")
        try:
            exec(code, {"__name__": "__main__"})
        except SystemExit as e:
            return e.code
        except Exception:
            tb = traceback.format_exc(limit=-3)
            stderr.write(f"{tb}\n".encode())
            return 1
