# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""runtime for python code transformed from a '.t' test"""

from __future__ import annotations

import io
import os
import re
import shutil
import sys
import tempfile
import textwrap
import unittest
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from .. import sh
from ..sh.osfs import OSFS
from ..sh.types import Env, OnError, Scope
from . import shext
from .diff import MultiLineMatcher


def hasfeature(feature: str) -> bool:
    """test if a feature is present

    >>> hasfeature("true")
    True
    >>> hasfeature("no-true")
    False
    >>> hasfeature("false")
    False
    """
    from . import hghave

    res = hghave.checkfeatures([feature])
    return all(not res.get(k) for k in ["error", "missing", "skipped"])


def require(feature: str):
    """require a feature to run a test"""
    if not hasfeature(feature):
        raise unittest.SkipTest(f"missing feature: {feature}")


@dataclass
class Mismatch:
    actual: str
    expected: str
    src: str
    # 'loc': line number starting from 0
    srcloc: int
    outloc: int
    endloc: int
    indent: int
    filename: str
    # optional associated test name
    testname: Optional[str] = None

    def __str__(self):
        return f"{repr(self.actual)} != {self(self.expected)} at {self.filename} line {self.outloc}"


def _normalizetrailingspace(text: str) -> str:
    """ensure ending with a single '\n'"""
    if text.endswith("\n\n"):
        text = text.rstrip("\n") + "\n"
    elif text and not text.endswith("\n"):
        text += "\n"
    elif text == "\n":
        text = ""
    return text


def checkoutput(
    a: str,
    b: str,
    src: str,
    srcloc: int,
    outloc: int,
    endloc: int,
    indent: int,
    filename: str,
):
    """compare output (a) with reference (b)
    report mismatch via globals()['mismatchcb']
    """
    hasfeature = sys._getframe(1).f_globals.get("hasfeature")
    matcher = MultiLineMatcher(b, hasfeature)
    if not matcher.match(a):
        a, b = matcher.normalize(a)
        # collect the output mismatch in 'mismatchmap'
        mismatch = Mismatch(
            actual=a,
            expected=b,
            src=src,
            srcloc=srcloc,
            outloc=outloc,
            endloc=endloc,
            indent=indent,
            filename=filename,
        )
        cb = sys._getframe(1).f_globals.get("mismatchcb")
        if cb:
            # callback "mismatchcb" is set - run by the testing.t.runtest()
            # report via callback, which handles rendering and autofix.
            # use the callback to report mismatch "in real time" and utilize
            # autofix features
            cb(mismatch)
        else:
            # no callback - ran as a standalone script (?)
            # report as an exception
            a = textwrap.indent(a, "  ")
            b = textwrap.indent(b, "  ")
            msg = (
                f"Mismatch in {os.path.basename(filename)} line {outloc}:\n"
                f"Expected:\n"
                f"{textwrap.indent(b, '  ')}\n"
                f"Actual:\n"
                f"{textwrap.indent(a, '  ')}\n"
            )
            raise AssertionError(msg)


class TestTmp:
    r"""TestTmp is a context manager that provides temporary test environemnts

    Example:

        with TestTmp() as t:
            ...

    t.sheval() interprets shell code:

        >>> with TestTmp() as t:
        ...     print(t.sheval('echo foo > a; cat a'), end="")
        ...     print(t.sheval('echo "`pwd`"'), end="")
        foo
        $TESTTMP

    t.path provides access to the temporary test directory:

        >>> with TestTmp() as t:
        ...     s = b'greeting() { echo "Hello, $1!"; };'
        ...     assert os.path.exists(t.path)
        ...     t.path.joinpath("a.sh").write_bytes(s) and None
        ...     t.sheval('source a.sh; greeting Alice > g.txt') or None
        ...     t.path.joinpath("g.txt").read_bytes()
        b'Hello, Alice!\n'

    t.setenv and t.getenv interact with environment variables:

        >>> with TestTmp() as t:
        ...     t.setenv("E", "100")
        ...     t.sheval('echo $E; F=101')
        ...     t.getenv("F")
        '100\n'
        '101'

    TestTmp clears out PATH by default so external executables need to be
    explicitly declared. Use t.requireexe() to declare such dependency:

        >>> with TestTmp() as t:
        ...     t.sheval('bash -c :')
        'sh: command not found: bash\n[127]\n'

        >>> with TestTmp() as t:
        ...     t.path.joinpath('a').write_bytes(b'5\n3\n') and None
        ...     s = 'for i in 2 1 4; do echo $i; done | syssort a -'
        ...     if os.path.exists("/bin/sort"):
        ...         t.requireexe("syssort", "/bin/sort")
        ...         # syssort is avalaible as a function known by sheval
        ...         t.sheval(s)
        ...     else:
        ...         '1\n2\n3\n4\n5\n'  # skip the test
        '1\n2\n3\n4\n5\n'

    t.command can be used to expose Python logic as shell commands.
    t.atexit can register cleanup logic. For example, one can implement
    server daemon lifecycle management like:

        >>> with TestTmp() as t:
        ...     @t.command
        ...     def server(args):
        ...         t.atexit(lambda: print(f"stop_server  {args=}"))
        ...         return f"start_server {args=}\n"
        ...     print(t.sheval("server 1; echo client1; server 2"), end="")
        start_server args=['1']
        client1
        start_server args=['2']
        stop_server  args=['2']
        stop_server  args=['1']

    t.pydoceval() can be used to evaluate Python code interacting with local
    variables. It works like a doctest:

        >>> with TestTmp() as t:
        ...     a = 1
        ...     # like doctest, print non-None statements
        ...     t.pydoceval('f"{a=}"; a=2; a')
        ...     a
        "'a=1'\n2\n"
        2

        >>> with TestTmp() as t:
        ...     a = 1
        ...     # use 'exec' mode to avoid printing each statement
        ...     t.pydoceval('a=5; 3; 4; a=6', 'exec')
        ...     a
        6

    By default, TestTmp updates global states (os.getcwd(), os.environ)
    in scope for convenience. This behavior can be disabled by setting
    updateglobalstate to False.

        >>> origcwd = os.getcwd()
        >>> with TestTmp() as t:
        ...     os.getcwd() == str(t.path)
        True

        >>> origcwd == os.getcwd()
        True

        >>> with TestTmp(updateglobalstate=False) as t:
        ...     os.getcwd() == t.path
        False

    """

    def __init__(
        self,
        updateglobalstate: bool = True,
        tmpprefix: str = "",
    ):
        """create a TestTmp environment (tmpdir, and a shinterp Env)
        Intended to be used in 'with' context.
        If updateglobalstate is True, also update os.environ and os.getpwd in
        the 'with' scope.
        """
        self._atexit = []
        self._updateglobalstate = updateglobalstate
        self._origpathenv = os.getenv("PATH") or os.defpath
        self._setup(tmpprefix)

    def atexit(self, func):
        # register a function to be called during tearing down
        self._atexit.append(func)

    def command(self, func):
        """decorator to register a Python function as a shell command"""
        self.shenv.cmdtable[func.__name__.lstrip("_")] = shext.wrap(func)

    def sheval(self, code: str, env: Optional[Env] = None) -> str:
        """sh.sheval in this TestTmp context"""
        if env is None:
            env = self.shenv
        try:
            out = sh.sheval(code, env, onerror=OnError.WARN_ABORT)

        except Exception as e:
            raise RuntimeError(f"cannot execute: {code.strip()}") from e
        out = self._applysubstitutions(out)
        # exit 80 means "SkipTest"
        if out == "[80]\n" or out.endswith("\n[80]\n"):
            reason = out[:-5].strip()
            if not reason:
                reason = f"{code.strip()} exited 80"
            raise unittest.SkipTest(reason)
        return out

    def pydoceval(self, code: str, mode: str = "single") -> Optional[str]:
        """evalualte python code in this TestTmp context"""
        f = sys._getframe(1)
        origout = sys.stdout
        sys.stdout = io.StringIO()
        try:
            compiled = compile(code, "<pydoceval>", mode)
            with shext.shellenv(self.shenv):
                # run code using the parent frame globals and locals
                exec(compiled, f.f_globals, f.f_locals)
            # pyre-fixme[16]: `TextIO` has no attribute `getvalue`.
            out = sys.stdout.getvalue()
        except Exception as e:
            out = str(e)
        finally:
            sys.stdout = origout
            f = None
        out = self._applysubstitutions(out)
        if out:
            return out

    def getenv(self, name: str) -> str:
        return self.shenv.getenv(name)

    def setenv(self, name: str, value, export: bool = True):
        self.shenv.setenv(name, str(value), Scope.SHELL)
        if export:
            self.shenv.exportenv(name)

    def requireexe(self, name: str, fullpath: Optional[str] = None):
        """require an external binary"""
        # find the program from PATH
        if fullpath is None:
            ext = ""
            if os.name == "nt":
                ext = ".exe"
            paths = self._origpathenv.split(os.pathsep)
            paths += os.defpath.split(os.pathsep)
            for path in paths:
                if path.startswith(str(self.path / "bin")):
                    continue
                fullpath = os.path.join(path, f"{name}{ext}")
                if os.path.isfile(fullpath):
                    break
            if not fullpath:
                raise unittest.SkipTest(f"missing exe: {name}")
        else:
            fullpath = os.path.realpath(fullpath)
        # add a function for sheval
        self.shenv.cmdtable[name] = shext.wrapexe(
            fullpath, env_override={"PATH": self._origpathenv}
        )
        # write a shim in $TESTTMP/bin for os.system
        self.path.joinpath("bin").mkdir(exist_ok=True)
        if os.name == "nt":
            script = "\n".join(
                [
                    "@echo off",
                    f"set PATH={self._origpathenv}",
                    f'"{fullpath}" %*',
                    "exit /B %errorlevel%",
                ]
            )
            destpath = self.path / "bin" / f"{name}.bat"
        else:
            script = "\n".join(
                [
                    "#!/bin/sh",
                    f"export PATH={repr(self._origpathenv)}",
                    f'exec {fullpath} "$@"',
                ]
            )
            destpath = self.path / "bin" / name
        destpath.write_text(script)
        destpath.chmod(0o555)

    def updatedglobalstate(self):
        """context manager that updates global states (pwd, environ, ...)"""
        return shext.shellenv(self.shenv)

    def __enter__(self):
        if self._updateglobalstate:
            # backup global state
            self._origenv = os.environ.copy()
            self._origcwd = os.getcwd()
            # update state
            if str(self.path) != self._origcwd:
                os.chdir(str(self.path))
            shext.updateosenv(self.shenv.getexportedenv())
        return self

    def __exit__(self, et, ev, tb):
        if self._updateglobalstate:
            # restore global state
            try:
                cwd = os.getcwd()
            except Exception:
                cwd = None
            if cwd != self._origcwd:
                os.chdir(self._origcwd)
            shext.updateosenv(self._origenv)
        self._teardown()

    def _setup(self, tmpprefix):
        # If TESTTMP is defined (ex. by run-tests.py), just create a
        # sub-directory in it and do not auto-delete. Expect whoever creating
        # TESTTMP to decide whether to delete it.
        existing_testtmp = os.getenv("TESTTMP")
        tmp = tempfile.mkdtemp(prefix=tmpprefix or "ttesttmp", dir=existing_testtmp)
        path = Path(os.path.realpath(tmp))

        fs = OSFS()
        fs.chdir(path)
        envvars = self._initialenvvars(path)
        shenv = Env(
            fs=fs,
            envvars=envvars,
            exportedenvvars=set(envvars),
            cmdtable=self._initialshellcmdtable(),
            stdin=io.BytesIO(),
        )

        pyenv = {
            "atexit": self.atexit,
            "checkoutput": self.checkoutput,
            "command": self.command,
            "getenv": self.getenv,
            "hasfeature": self.hasfeature,
            "pydoceval": self.pydoceval,
            "requireexe": self.requireexe,
            "require": self.require,
            "setenv": self.setenv,
            "sheval": self.sheval,
            "TESTTMP": path,
        }

        self.path = path
        self.should_delete_path = not existing_testtmp
        self.shenv = shenv
        self.pyenv = pyenv
        self.substitutions = [(re.escape(str(path)), "$TESTTMP")]
        if os.name == "nt":
            self.substitutions += [
                # TESTTMP using posix slash
                (re.escape(str(path).replace("\\", "/")), "$TESTTMP"),
                # strip UNC prefix
                (re.escape("\\\\?\\"), ""),
            ]

    @property
    def checkoutput(self):
        return checkoutput

    def require(self, feature: str) -> bool:
        return require(feature)

    def hasfeature(self, feature: str) -> bool:
        return hasfeature(feature)

    def _teardown(self):
        try:
            for func in reversed(self._atexit):
                func()
        finally:
            if self.should_delete_path:
                shutil.rmtree(str(self.path), ignore_errors=True)

    def _initialshellcmdtable(self):
        cmdtable = dict(sh.stdlib.cmdtable)
        return cmdtable

    def _initialenvvars(self, path: Path):
        environ = {
            "HOME": str(path),
            "PATH": str(path / "bin"),
            "TESTTMP": str(path),
        }
        if os.name == "nt":
            # Required by some application logic.
            environ.update(
                {
                    "APPDATA": str(path),
                    "USERPROFILE": str(path),
                }
            )
            # SYSTEMROOT is required on Windows for Python to initialize.
            # See https://stackoverflow.com/a/64706392
            for name in ["SYSTEMROOT"]:
                value = os.getenv(name)
                if value:
                    environ[name] = value
            # Part of PATH containing pythonXX.dll is needed to find Python
            # runtime.
            version = sys.version_info
            pythonxx = f"python{version.major}{version.minor}.dll"
            for path in sys.path:
                if os.path.exists(os.path.join(path, pythonxx)):
                    environ["PATH"] += f"{os.pathsep}{path}"
        return environ

    def _applysubstitutions(self, out: str) -> str:
        for frompat, topat in self.substitutions:
            if isinstance(frompat, bytes):
                out = re.sub(frompat, topat, out.encode()).decode()
            else:
                out = re.sub(frompat, topat, out)
        return out
