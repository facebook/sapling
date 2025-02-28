# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""shell features exclusive for .t testing"""

import io
import os
import subprocess
import sys
from contextlib import contextmanager
from typing import BinaryIO, Dict, Optional

from ..sh import Env
from ..sh.stdlib import wrap


class WrapStdIO(io.TextIOWrapper):
    """wrap a BinaryIO so it can be used as sys.stdio"""

    def close(self):
        # avoid closing the io
        self.flush()


@contextmanager
def shellenv(env: Env, stdin=None, stdout=None, stderr=None):
    """run with environ and cwd specified by the shell env"""
    origenv = os.environ.copy()
    origcwd = try_getcwd()
    origargv = sys.argv
    origstdin = sys.stdin
    origstdout = sys.stdout
    origstderr = sys.stderr
    try:
        updateosenv(env.getexportedenv())
        try:
            os.chdir(env.fs.cwd())
        except FileNotFoundError:
            pass
        sys.argv = env.args
        if stdin is not None:
            sys.stdin = WrapStdIO(stdin, newline="\n")
        if stdout is not None:
            sys.stdout = WrapStdIO(stdout, newline="\n")
        if stderr is not None:
            sys.stderr = WrapStdIO(stderr, newline="\n")
        yield
    finally:
        try:
            sys.stdout.flush()
            sys.stderr.flush()
        except Exception:
            pass
        updateosenv(origenv)
        sys.argv = origargv
        sys.stdin = origstdin
        sys.stdout = origstdout
        sys.stderr = origstderr
        cwd = try_getcwd()
        if cwd and origcwd and cwd != origcwd:
            os.chdir(origcwd)


def try_getcwd():
    try:
        cwd = os.getcwd()
    except IOError:
        # In missing-cwd tests, os.getcwd() can raise.
        cwd = None
    return cwd


def updateosenv(environ: Dict[str, str]):
    """update the Python, libc env vars to match environ"""
    # cannot use os.environ = environ - it won't update libc env
    allkeys = set(os.environ) | set(environ)
    for name in allkeys:
        oldvalue = os.environ.get(name)
        newvalue = environ.get(name)
        if oldvalue == newvalue:
            continue
        if newvalue is None:
            # calls native function unsetenv
            del os.environ[name]
        else:
            # calls native function putenv
            os.environ[name] = newvalue


def wrapexe(exepath: str, env_override: Optional[Dict[str, str]] = None):
    """wrap an external executable in a function useful for sheval"""

    def run(
        stdin: BinaryIO,
        stdout: BinaryIO,
        stderr: BinaryIO,
        env: Env,
        exepath: str = exepath,
        env_override=env_override,
    ) -> int:
        if stderr is stdout:
            pstderr = subprocess.STDOUT
        else:
            pstderr = subprocess.PIPE
        args = [exepath] + env.args[1:]
        procenv = env.getexportedenv()
        if env_override:
            procenv.update(env_override)
        p = subprocess.Popen(
            args,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=pstderr,
            env=procenv,
            cwd=env.fs.cwd(),
        )
        # assumes the program always consumes the full stdin for simplicity
        (out, err) = p.communicate(stdin.read())
        if out:
            stdout.write(out)
        if err:
            stderr.write(err)
        return p.returncode

    return wrap(run)
