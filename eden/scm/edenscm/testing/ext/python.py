# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""python extension for TestTmp

- provides a "python" command
- provides a "assertCovered" function to check function coverage
"""

import collections
import dis
import inspect
import sys
import traceback
from typing import BinaryIO, List

from ..sh import Env
from ..t.runtime import TestTmp
from ..t.shext import shellenv


def testsetup(t: TestTmp):
    t.command(python)
    t.setenv("PYTHON", "python")
    t.pyenv["assertCovered"] = CoverageChecker


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
    # pyre-fixme[7]: Expected `int` but got implicit return value of `None`.
    with shellenv(env, stdin=stdin, stdout=stdout, stderr=stderr):
        # pyre-fixme[6]: For 2nd param expected `Union[_PathLike[typing.Any], bytes,
        #  str]` but got `Union[List[str], str]`.
        code = compile(code, args and args[0] or "<stdin>", "exec")
        try:
            exec(code, {"__name__": "__main__"})
        except SystemExit as e:
            # pyre-fixme[7]: Expected `int` but got `Union[None, int, str]`.
            return e.code
        except Exception:
            tb = traceback.format_exc(limit=-3)
            stderr.write(f"{tb}\n".encode())
            return 1


class CoverageChecker:
    """Check that lines in the given functions are covered in the 'with' scope.

    Example:

        >>> def mydiv(a, b):
        ...     '''
        ...     This is a test function
        ...     '''
        ...     try:
        ...         "useless line"
        ...         result = a / b
        ...     except ZeroDivisionError:
        ...         # Silent "/ 0" error.
        ...         result = 0
        ...     return result

    mydiv(3, 2) does not cover result = 0 case:

        >>> try:
        ...     with CoverageChecker(mydiv):
        ...         v = mydiv(3, 2)
        ... except MissingCoverage as e:
        ...     print(e, end="")
        Missing coverage:
               mydiv+7   |     except ZeroDivisionError:
               mydiv+9   |         result = 0

    mydiv(3, 0) covers all lines in the function:

        >>> with CoverageChecker(mydiv):
        ...     v = mydiv(3, 0)
    """

    def __init__(self, *funcs_to_track):
        """Track given functions"""
        self._tracked_funcs = funcs_to_track
        self._tracked_codes = {f.__code__ for f in funcs_to_track}
        self._covered_code_lines = collections.defaultdict(set)

    def __enter__(self):
        sys.settrace(self._trace)
        return self

    def __exit__(self, _exc_type, _exc_value, _traceback):
        sys.settrace(None)
        missing_coverage = self._calculate_missing_coverage().rstrip()
        if missing_coverage:
            raise MissingCoverage(f"Missing coverage:\n{missing_coverage}")
        return None

    def _trace(self, frame, event, arg):
        # See Python stdlib sys.settrace for the specification of this
        # function.

        # Mark the line as covered.
        self._covered_code_lines[frame.f_code].add(frame.f_lineno)

        # Tell Python to report line coverage for selected functions.
        if event == "call" and frame.f_code in self._tracked_codes:
            return self._trace

    def _calculate_missing_coverage(self) -> str:
        """Print summary of coverage information for tracked functions"""
        missing_coverage = []
        for func in self._tracked_funcs:
            code = func.__code__
            # Lines like comments or docstrings don't need coverage.
            # Only check lines referred by bytecode instructions.
            expect_linenos = {
                inst.starts_line
                for inst in dis.Bytecode(code)
                if inst.starts_line is not None
            }
            # Line numbers that are actually covered.
            actual_linenos = self._covered_code_lines[code]
            # Source code for representation.
            lines, start_lineno = inspect.getsourcelines(code)
            for missing_lineno in expect_linenos - actual_linenos:
                relative_lineno = missing_lineno - start_lineno
                if relative_lineno < len(lines) and relative_lineno >= 0:
                    line = lines[relative_lineno]
                else:
                    line = "# unknown line"
                missing_coverage.append(
                    "%12s+%-4d| %s\n" % (func.__name__, relative_lineno, line.rstrip())
                )
        return "".join(missing_coverage)


class MissingCoverage(ValueError):
    pass
