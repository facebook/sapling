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
from itertools import chain
from pathlib import Path
from typing import Callable, Iterable, Optional, Union

from .. import sh
from ..sh.bufio import BufIO
from ..sh.osfs import OSFS
from ..sh.types import Env, OnError, Scope
from . import hghave, shext
from .diff import MultiLineMatcher


def hasfeature(feature: str) -> bool:
    """test if a feature is present

    >>> hasfeature("true")
    True
    >>> hasfeature("no-true")
    False
    >>> hasfeature("false")
    False
    >>> hasfeature("banana")
    False
    >>> hasfeature("no-banana")
    True
    """
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
    fallback_line_match: Optional[Callable[[str, str], bool]] = None,
):
    """compare output (a) with reference (b)
    report mismatch via globals()['mismatchcb']
    """
    hasfeature = sys._getframe(2).f_globals.get("hasfeature")
    matcher = MultiLineMatcher(b, hasfeature, fallback_line_match)
    # AssertionError usually means the test is already broken.
    # Report it as a mismatch to fail the test. Note: we don't raise here
    # to provide better error messages (ex. show line number of the assertion)
    force_mismatch = a.startswith("AssertionError")
    if force_mismatch or not matcher.match(a):
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
        cb = sys._getframe(2).f_globals.get("mismatchcb")
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
    r"""TestTmp is a context manager that provides temporary test environments

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
        ...         # syssort is available as a function known by sheval
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
        testcase: Union[Iterable[Union[str, None, bool]], str, None, bool] = None,
    ):
        """create a TestTmp environment (tmpdir, and a shinterp Env)
        Intended to be used in 'with' context.
        If updateglobalstate is True, also update os.environ and os.getpwd in
        the 'with' scope.
        """
        self._atexit = []
        self._updateglobalstate = updateglobalstate
        self._origpathenv = os.getenv("PATH") or os.defpath
        self._fallbackmatch = None
        self._setup(tmpprefix)
        self._lastout = ""
        # feature names that can be tested by `hasfeature` for this single test case
        self._testcase_names = list(_expand_testcase_names(testcase))

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
        self._lastout = out
        return out

    def pydoceval(self, code: str, mode: str = "single") -> Optional[str]:
        """evaluate python code in this TestTmp context"""
        f = sys._getframe(1)
        origout = sys.stdout
        sys.stdout = io.StringIO()
        try:
            compiled = compile(code, "<pydoceval>", mode)
            # Mutating globals() works, but not locals() (it works at the module
            # level because locals() is globals()). We aren't at the module
            # scope because we are inside the _run_once function.
            #
            # Work around by passing in globals() as exec's locals so newly
            # defined variables will become globals. To make the actual locals
            # available, we mix them into globals_env.

            # Provide "_" as the output from the last command.
            globals_env = {**f.f_globals, "_": self._lastout, **f.f_locals}
            with shext.shellenv(self.shenv):
                # run code using the parent frame globals and locals
                exec(compiled, globals_env, f.f_globals)
            # pyre-fixme[16]: `TextIO` has no attribute `getvalue`.
            out = sys.stdout.getvalue()
        except AssertionError as e:
            msg = str(e)
            if msg:
                out = f"AssertionError: {msg}\n"
            else:
                out = "AssertionError!\n"
        except Exception as e:
            out = str(e)
        finally:
            sys.stdout = origout
            f = None
        out = self._applysubstitutions(out)
        self._lastout = out
        if out:
            return out

    def getenv(self, name: str, alt: str = "") -> str:
        return self.shenv.getenv(name, alt)

    def setenv(self, name: str, value, export: bool = True):
        self.shenv.setenv(name, str(value), Scope.SHELL)
        if export:
            self.shenv.exportenv(name)

    def requireexe(self, name: str, fullpath: Optional[str] = None, symlink=False):
        """require an external binary"""
        ext = ".exe" if os.name == "nt" else ""
        # find the program from PATH
        if fullpath is None:
            paths = self._origpathenv.split(os.pathsep)
            paths += os.defpath.split(os.pathsep)
            for path in paths:
                if path.startswith(str(self.path / "bin")):
                    continue
                exts = [".exe", ".bat"] if os.name == "nt" else [""]
                found = False
                for x in exts:
                    fullpath = os.path.join(path, f"{name}{x}")
                    if os.path.isfile(fullpath):
                        found = True
                        break
                if found:
                    break
            if not fullpath:
                raise unittest.SkipTest(f"missing exe: {name}")
        else:
            fullpath = os.path.realpath(fullpath)
        # add a function for sheval
        orig_path = os.pathsep.join([str(self.path / "bin"), self._origpathenv])
        for allowed in (name, fullpath):
            # Allow shell to run the short name (e.g. "hg") or the fullpath (e.g.
            # "/some/long/path/build-dir/hg").
            self.shenv.cmdtable[allowed] = shext.wrapexe(
                fullpath, env_override={"PATH": orig_path}
            )
        # write a shim in $TESTTMP/bin for os.system
        self.path.joinpath("bin").mkdir(exist_ok=True)
        if symlink:
            os.symlink(fullpath, self.path / "bin" / (name + ext))
        else:
            if os.name == "nt":
                script = "\n".join(
                    [
                        "@echo off",
                        f"set PATH={orig_path}",
                        f'"{fullpath}" %*',
                        "exit /B %errorlevel%",
                    ]
                )
                destpath = self.path / "bin" / f"{name}.bat"
            else:
                script = "\n".join(
                    [
                        "#!/bin/sh",
                        f"export PATH={repr(orig_path)}",
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

        for name in self._testcase_names:
            if name in hghave.checks:
                raise RuntimeError(
                    f"test case {name} conflicts with an existing feature"
                )
            hghave.checks[name] = (True, f"test case {name}")

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

        for name in self._testcase_names:
            del hghave.checks[name]

        self._teardown()

    def _setup(self, tmpprefix):
        # If TESTTMP is defined (ex. by run-tests.py), just create a
        # sub-directory in it and do not auto-delete. Expect whoever creating
        # TESTTMP to decide whether to delete it.
        existing_testtmp = os.getenv("TESTTMP")
        tmp = tempfile.mkdtemp(prefix=tmpprefix or "ttesttmp", dir=existing_testtmp)
        path = Path(os.path.realpath(tmp))

        if existing_testtmp:
            # Write a breadcrumb leading from original $TESTTMP to current one.
            # This is useful for (persistent) EdenFS to discover the current $TESTTMP.
            (Path(existing_testtmp) / ".testtmp").write_text(str(path))

        fs = OSFS()
        fs.chdir(path)
        envvars = self._initialenvvars(path)
        shenv = Env(
            fs=fs,
            envvars=envvars,
            exportedenvvars=set(envvars),
            cmdtable=self._initialshellcmdtable(),
            stdin=BufIO(),
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

    def checkoutput(
        self,
        a: str,
        b: str,
        src: str,
        srcloc: int,
        outloc: int,
        endloc: int,
        indent: int,
        filename: str,
    ):
        try:
            return checkoutput(
                a,
                b,
                src,
                srcloc,
                outloc,
                endloc,
                indent,
                filename,
                fallback_line_match=self._fallbackmatch,
            )
        finally:
            self.post_checkoutput(a, b, src, srcloc, outloc, endloc, indent, filename)

    def post_checkoutput(
        self,
        a: str,
        b: str,
        src: str,
        srcloc: int,
        outloc: int,
        endloc: int,
        indent: int,
        filename: str,
    ):
        # Can be patched by extensions, like "record".
        pass

    def require(self, feature: str) -> bool:
        return require(feature)

    def registerfallbackmatch(self, fallback: Callable[[str, str], bool]):
        self._fallbackmatch = fallback

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


def _expand_testcase_names(
    value: Union[Iterable[str], str, bool, None],
) -> Iterable[str]:
    """Flatten and filter out None test case names.

    >>> list(_expand_testcase_names(None))
    []
    >>> list(_expand_testcase_names('a'))
    ['a']
    >>> list(_expand_testcase_names(('a', None, False, 'b')))
    ['a', 'b']
    >>> list(_expand_testcase_names(('a', ('b', 'c'))))
    ['a', 'b', 'c']
    """
    if value is False or value is None:
        return
    elif isinstance(value, str):
        yield value
    elif value is True:
        raise RuntimeError("_expand_testcase_names: invalid value: True")
    else:
        yield from chain(*map(_expand_testcase_names, value))
