# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""common types used by interp and stdlib"""

from __future__ import annotations

import threading
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from enum import IntEnum
from io import BytesIO
from typing import Any, BinaryIO, Callable, Dict, List, Optional, Set, Tuple


@dataclass
class InterpResult:
    """Return value of the interp functions"""

    # output:
    # - interp string results (regardless of redirection)
    # - command stdout + stderr (if not redirected)
    out: str = ""

    # exit code
    exitcode: int = 0

    # Quoted by: ' or " or ` or $ or None.
    # If not None, prevent glob-expand or shlex.split.
    quoted: Optional[str] = None

    def chain(self, other: InterpResult) -> InterpResult:
        """combine 2 outputs into one"""
        return InterpResult(
            out=self.out + other.out,
            exitcode=other.exitcode,
            # XXX: mixed quoting is not handled correctly.
            quoted=self.quoted or other.quoted,
        )


class Scope(IntEnum):
    # single command: A=1 foo bar > baz
    COMMAND = 0

    # list of commands: { foo; bar; baz }
    COMPOUND = 1

    # inside a shell function: foo() { ... }
    FUNCTION = 2

    # top-level or sub-shells
    SHELL = 3


class OnError(IntEnum):
    # Raise as a Python exception. Discard pending shell output.
    RAISE = 0

    # Append traceback to shell output, then abort shell execution
    # with an non-zero exit code.
    WARN_ABORT = 1

    # Append traceback to shell output, then continue shell execution.
    # Shell exit code won't reflect what was wrong.
    WARN_CONTINUE = 2


@dataclass
class Env:
    """Environment for interp functions

    Env can form a nested chain (stack) for scope support.
    For example,
    - A=1 command             # A=1 scoped to command
    - f() { local A=1; ... }  # A=1 scoped to function
    - ( A=1 ... )             # A=1 scoped to subshell
    - { ... } > a             # redirect scoped to command list
    """

    # filesystem abstraction
    fs: ShellFS

    # scope, useful for setenv to decide which level
    scope: Scope = Scope.SHELL

    # redirection, only affects commands
    stdin: Optional[BinaryIO] = None
    stdout: Optional[BinaryIO] = None
    stderr: Optional[BinaryIO] = None

    # args ($0, $1, ...)
    args: List[str] = field(default_factory=list)

    # ref to $?
    reflastexitcode: List[int] = field(default_factory=lambda: [0])

    # environment variables to expand $var
    envvars: Dict[str, str] = field(default_factory=dict)

    # environment variables marked as "exported" - visible to commands
    exportedenvvars: Set[str] = field(default_factory=set)

    # environment variables marked as "local" - FUNCTION not SHELL scope
    localenvvars: Set[str] = field(default_factory=set)

    # shell commands (builtin, defined function, ...)
    cmdtable: Dict[str, Callable[[Env], InterpResult]] = field(default_factory=dict)

    # parent environment
    parent: Optional[Env] = None

    # background jobs
    jobs: List[Tuple[threading.Thread, BytesIO]] = field(default_factory=list)

    def getenv(self, name: str) -> str:
        if name == "PWD":
            return self.fs.cwd()
        value = self.envvars.get(name)
        if value is None and self.parent is not None:
            value = self.parent.getenv(name)
        return value or ""

    def setenv(self, name: str, value: str, scope: Optional[Scope] = None):
        assert isinstance(value, str), f"setenv {name}={repr(value)} is not a str"
        if scope is None:
            if name in self.localenvvars:
                scope = Scope.FUNCTION
            else:
                scope = Scope.SHELL
        parent = self.parentscope(scope)
        assert parent.scope >= scope
        parent.envvars[name] = value

    def unset(self, name: str, scope: Scope):
        parent = self.parentscope(scope)
        parent.envvars.pop(name, None)
        if name in parent.exportedenvvars:
            parent.exportedenvvars.remove(name)

    def getexportedenv(self) -> Dict[str, str]:
        """get all exported environment variables"""
        result = {}
        for name in sorted(self.exportedenvvars):
            result[name] = self.getenv(name)
        return result

    def localenv(self, name: str):
        """mark an env as local"""
        self.localenvvars.add(name)

    def exportenv(self, name: str):
        """mark an env as exported"""
        self.exportedenvvars.add(name)

    def getcmd(self, name: str) -> Callable[[Env], InterpResult]:
        """lookup a function by name

        If the function does not exist, return a dummy function that prints
        "command not found".
        """
        value = self.cmdtable.get(name)
        return value or _commandnotfound

    @property
    def lastexitcode(self) -> int:
        return self.reflastexitcode[0]

    @lastexitcode.setter
    def lastexitcode(self, value: int):
        self.reflastexitcode[0] = value

    def parentscope(self, scope: Scope) -> Env:
        """find parent Env with scope - could return 'self'"""
        cur = self
        while cur.parent is not None and cur.scope < scope:
            cur = cur.parent
        return cur

    def nested(self, scope: Scope) -> Env:
        """create a nested Env so changes won't affect parent Env

        Depends on scope, fields are handled differently:

        - shared:  ref copied, changes in nested affect parent
        - forked:  copied, but changes in nested won't affect parent
        - chained: nested gets a new, empty state but will lookup parent
        - reset:   nested gets a new empty state

                     SHELL   | FUNCTION | COMPOUND | COMMAND
                 fs: shared  | shared   | shared   | shared
             fs.cwd: forked  | shared   | shared   | shared
              stdio: shared  | shared   | shared   | shared
                 $?: shared  | shared   | shared   | shared
          exportenv: forked  | shared   | shared   | forked
           localenv: reset   | reset    | shared   | shared
            envvars: chained | chained  | shared   | chained
           cmdtable: forked  | shared   | shared   | shared
               args: reset   | forked   | shared   | shared
               jobs: reset   | shared   | shared   | shared
        """
        fs = self.fs.clone(unsharecwd=(scope >= Scope.SHELL))
        exportedenvvars = self.exportedenvvars
        localenvvars = self.localenvvars
        envvars = {}
        cmdtable = self.cmdtable
        args = self.args
        jobs = self.jobs
        if scope == Scope.SHELL:
            exportedenvvars = set(exportedenvvars)
            localenvvars = set()
            cmdtable = dict(cmdtable)
            args = []
            jobs = []
        elif scope == Scope.COMPOUND:
            envvars = self.envvars
        elif scope == Scope.FUNCTION:
            localenvvars = set()
            args = args[:]
        elif scope == Scope.COMMAND:
            exportedenvvars = set(exportedenvvars)

        return Env(
            scope=scope,
            fs=fs,
            stdin=self.stdin,
            stdout=self.stdout,
            stderr=self.stderr,
            reflastexitcode=self.reflastexitcode,
            envvars=envvars,
            exportedenvvars=exportedenvvars,
            localenvvars=localenvvars,
            cmdtable=cmdtable,
            args=args,
            jobs=jobs,
            parent=self,
        )


@dataclass
class ShellFS(ABC):
    """interface for simple shinterp fs use-cases"""

    # shared (or not shared) "cwd" among clones
    refcwd: List[str] = field(default_factory=lambda: [""])

    # shared fs state among clones
    state: Dict[str, Any] = field(default_factory=dict)

    # open, glob, cwd are required by interp for redirect, and '*' expand

    @abstractmethod
    def open(self, path: str, mode: str) -> BinaryIO:
        raise NotImplementedError

    @abstractmethod
    def glob(self, pat: str) -> List[str]:
        raise NotImplementedError

    def cwd(self) -> str:
        return self.refcwd[0]

    def _setcwd(self, path: str):
        self.refcwd[0] = path

    def clone(self, unsharecwd=False) -> ShellFS:
        """clone the FS abstraction.
        If unsharecwd is True, then changes to cwd will not be shared,
        useful for sub-shells.
        """
        kwargs = dict(self.__dict__)
        if unsharecwd:
            kwargs["refcwd"] = kwargs["refcwd"][:]
        # pyre-fixme[45]: Cannot instantiate abstract class `ShellFS`.
        fs = self.__class__(**kwargs)
        return fs

    # useful for stdlib but not interp

    def chdir(self, path: str):
        raise NotImplementedError("use _setcwd to implement chdir")

    def chmod(self, path: str, mode: int):
        raise NotImplementedError

    def stat(self, path: str):
        raise NotImplementedError

    def utime(self, path: str, time: int):
        raise NotImplementedError

    def isdir(self, path: str):
        raise NotImplementedError

    def isfile(self, path: str):
        raise NotImplementedError

    def exists(self, path: str):
        raise NotImplementedError

    def listdir(self, path: str) -> List[str]:
        raise NotImplementedError

    def mkdir(self, path: str):
        raise NotImplementedError

    def mv(self, src: str, dst: str):
        raise NotImplementedError

    def rm(self, path: str):
        raise NotImplementedError

    def cp(self, src: str, dst: str):
        raise NotImplementedError

    def link(self, src: str, dst: str):
        raise NotImplementedError

    def symlink(self, src: str, dst: str):
        raise NotImplementedError


def _commandnotfound(env: Env) -> InterpResult:
    cmd = env.args[0]
    return InterpResult(out=f"sh: command not found: {cmd}\n", exitcode=127)


# Exception for control flow


@dataclass
class ShellExit(Exception):
    # used for initial return code
    code: int

    # used for "incomplete" result
    res: Optional[InterpResult] = None

    def result(self) -> InterpResult:
        res = self.res or InterpResult()
        res.exitcode = self.code
        return res


class ShellReturn(ShellExit):
    # a superclass of ShellExit so top-level can except ShellExit
    # and ignore ShellReturn.
    pass
