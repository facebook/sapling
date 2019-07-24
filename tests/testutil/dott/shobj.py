# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import fnmatch
import os
import re
import shlex

from .. import autofix
from . import shlib


class LazyCommand(object):
    """Pseudo shell command with ways to execute it"""

    def __init__(self, command):
        self._command = command
        self._output = None
        self._stdin = None
        self._stdoutpath = None
        self._stdoutappend = False
        if _delayedexception:
            raise _delayedexception[0]

    @property
    def output(self):
        """Emulate the command. Return its output."""
        if self._output is None:
            if isinstance(self._command, str):
                args = shlex.split(self._command)
            else:
                args = self._command
            args = map(os.path.expandvars, args)
            # Work with environment variables
            backupenv = {}
            while args and "=" in args[0]:
                name, value = args[0].split("=", 1)
                backupenv[name] = os.environ.get(name)
                os.environ[name] = shlib.expandpath(value)
                args = args[1:]
            if args:
                # Lookup a function named args[0] in shlib
                func = getattr(shlib, args[0], None)
                if callable(func):
                    kwargs = {}
                    if self._stdin is not None:
                        kwargs["stdin"] = self._stdin
                    self._output = func(*args[1:], **kwargs) or ""
                    if self._stdoutpath is not None and self._output:
                        outpath = os.path.expandvars(self._stdoutpath)
                        mode = self._stdoutappend and "ab" or "wb"
                        open(outpath, mode).write(self._output)
                        self._output = ""
                else:
                    raise NotImplementedError("shell command %r is unknown" % (args,))
                for name, value in backupenv.items():
                    if value is None:
                        del os.environ[name]
                    else:
                        os.environ[name] = value
            else:
                self._output = ""
                # Do not restore environ in this case.
                # This allows "commands" like "A=B" to have side effect on environ.
        return self._output

    def __eq__(self, rhs):
        """Test output, with autofix ability"""
        # out is not mangled by "_repr"
        out = self.output
        for func in _normalizefuncs:
            out = func(out) or out
        # rhs is mangled by "_repr"
        if rhs.startswith("\n"):
            rhs = rhs[1:]
            rhs = autofix._removeindent(rhs)
            rhs = _removetrailingspacesmark(rhs)
        autofix.eq(out, rhs, nested=1, eqfunc=eqglob)

    def __del__(self):
        # Need the side-effect calculating output, if __eq__ is not called.
        if self._output is None:
            try:
                out = self.output
                # Output should be empty
                if out:
                    raise AssertionError("%r output %r is not empty" % (self, out))
            except Exception as ex:
                _delayedexception.append(ex)
                raise

    def __lshift__(self, heredoc):
        """<< str, use str as stdin content"""
        assert self._output is None
        if heredoc.startswith("\n"):
            # Strip the newline added by autofix._repr
            heredoc = heredoc[1:]
        heredoc = autofix._removeindent(heredoc)
        heredoc = _removetrailingspacesmark(heredoc)
        self._stdin = heredoc
        return self

    def __gt__(self, path):
        """> path, write output to path"""
        assert self._output is None
        self._stdoutpath = path
        self._stdoutappend = False
        return self

    def __rshift__(self, path):
        """>> path, append output to path"""
        assert self._output is None
        self._stdoutpath = path
        self._stdoutappend = True
        return self

    def __repr__(self):
        redirects = ""
        if self._stdoutpath is not None:
            if self._stdoutappend:
                redirects += " >> %s" % self._stdoutpath
            else:
                redirects += " > %s" % self._stdoutpath
        if self._stdin is not None:
            redirects += " << %r" % self._stdin
        return "<Command %r%s>" % (self._command, redirects)


class ShellSingleton(object):
    """Produce LazyCommand"""

    def __mod__(self, command):
        """%, looks like a shell prompt"""
        return LazyCommand(command)

    # Proxy other attribute accesses to shlib.
    # This enables code like: `sh.echo("foo")`
    __getattr__ = shlib.__dict__.get


# ShellSingleton and LazyCommand are merely syntax sugar to make code
# shorter. See https://github.com/python/black/issues/697.
#
# Basically, this allows:
#
#     sh % "seq 1 3" == r"""
#         1
#         2
#         3"""
#
# Instead of:
#
#     shelleq(
#         "seq 1 3",  # black puts """ in a new line.
#         r"""
#         1
#         2
#         3""",  # black puts ")" in a new line.
#     )
#
# That's 4 lines vs 7 lines.

# Delayed exceptions. Exceptions cannot be raised in `__del__`.
# Record them and raise later.
_delayedexception = []

# Functions to normalize outputs (ex. replace "$TESTTMP")
_normalizefuncs = []


# Decorator. Add an output normalizing function.
normalizeoutput = _normalizefuncs.append


_errors = {
    br"$ENOENT$": (
        # strerror()
        br"No such file or directory",
        # FormatMessage(ERROR_FILE_NOT_FOUND)
        br"The system cannot find the file specified",
    ),
    br"$ENOTDIR$": (
        # strerror()
        br"Not a directory",
        # FormatMessage(ERROR_PATH_NOT_FOUND)
        br"The system cannot find the path specified",
    ),
}


@normalizeoutput
def _normalizeerr(out, _errors=_errors):
    """Translate error messages to '$ENOENT$'"""
    for name, values in _errors.items():
        for value in values:
            out = out.replace(value, name)
    return out


def _removetrailingspacesmark(out):
    """Remove '(trailing space)'"""
    return out.replace(" (trailing space)", "")


def eqglob(a, b):
    """Compare multi-line strings, with '(glob)', '(re)' etc. support"""
    if not (isinstance(a, str) and isinstance(b, str)):
        return False
    if os.name == "nt":
        # Normalize path on Windows
        a = a.replace("\\", "/")
        b = b.replace("\\", "/")
    alines = a.splitlines()
    blines = b.splitlines()
    if len(alines) != len(blines):
        return False
    for aline, bline in zip(alines, blines):
        if bline.endswith(" (glob)"):
            # As an approximation, use fnmatch to do the job.
            # "[]" do not have special meaning in run-tests.py glob patterns.
            # Replace them with "?".
            globline = bline[:-7].replace("[", "?").replace("]", "?")
            if not fnmatch.fnmatch(aline, globline):
                return False
        elif bline.endswith(" (esc)"):
            bline = bline[:-6].decode("string-escape")
            if bline != aline:
                return False
        elif bline.endswith(" (re)"):
            if not re.match(bline[:-5] + r"\Z", aline):
                return False
        elif aline != bline:
            return False
    return True
