# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import atexit
import errno
import fnmatch
import os
import re
import shlex
import sys

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
        # Raising from an "atexit" function might have weird behavior, for
        # example:
        #
        #   Exception KeyError: KeyError(139797274294080,) in <module
        #   'threading' from '/usr/lib64/python2.7/threading.pyc'> ignored
        #
        # So we try to check delayed exception if we got a chance and raise
        # it early.
        _checkdelayedexception()

    @property
    def output(self):
        """Emulate the command. Return its output.

        Currently the output is a string. i.e. infinite stream (ex. `yes` from
        coreutils) cannot be expressed using this API.
        """
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
                autofix.eq(out, "", nested=1, fixfunc=_fixoutput)
            except (SystemExit, Exception):
                # Cannot raise in __del__. Put it in _delayedexception
                if not _delayedexception:
                    _delayedexception.append(sys.exc_info())

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

    def __or__(self, cmd):
        """| cmd, pipe through another command"""
        if isinstance(cmd, str):
            cmd = LazyCommand(cmd)
        assert isinstance(cmd, LazyCommand)
        out = self.output
        return cmd << out

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


@atexit.register
def _checkdelayedexception(_delayedexception=_delayedexception):
    if _delayedexception:
        exctype, excvalue, traceback = _delayedexception[0]
        # Only raise the first "delayed exception"
        _delayedexception[:] = [(None, None, None)]
        if excvalue is not None:
            raise exctype, excvalue, traceback


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


def _fixoutput(actual, expected, path, lineno, parsed):
    """Fix `sh % "foo"` to `sh % "foo" == actual`"""
    # XXX: This function does not do any real AST parsing and can in theory
    # produce wrong autofixes. For example, if `line` ends with a comment
    # like `# foo`. This is merely because the current implementation is easier
    # to write and it should work for most cases. If that becomes an issue we
    # need to change it to do more precise parsing.
    lines = parsed.get_code().splitlines(True)
    # - 1: convert 1-based index to 0-based
    line = lines[lineno - 1]
    linewidth = len(lines[lineno - 1]) - 1
    code = "%s == %s" % (line.rstrip(), autofix._repr(actual, indent=4))
    return ((lineno, 0), (lineno, linewidth)), code


def eqglob(a, b):
    """Compare multi-line strings, with '(glob)', '(re)' etc. support"""
    if not (isinstance(a, str) and isinstance(b, str)):
        return False
    alines = a.splitlines()
    blines = b.splitlines()
    if len(alines) != len(blines):
        return False
    for aline, bline in zip(alines, blines):
        if bline.endswith(" (esc)"):
            bline = bline[:-6].decode("string-escape")
        if os.name == "nt":
            # Normalize path on Windows
            aline = aline.replace("\\", "/")
            bline = bline.replace("\\", "/")
        if bline.endswith(" (glob)"):
            # As an approximation, use fnmatch to do the job.
            # "[]" do not have special meaning in run-tests.py glob patterns.
            # Replace them with "?".
            globline = bline[:-7].replace("[", "?").replace("]", "?")
            if not fnmatch.fnmatch(aline, globline):
                return False
        elif bline.endswith(" (re)"):
            if not re.match(bline[:-5] + r"\Z", aline):
                return False
        elif aline != bline:
            return False
    return True
