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
    origcwd = os.getcwd()
    origargv = sys.argv
    origstdin = sys.stdin
    origstdout = sys.stdout
    origstderr = sys.stderr
    try:
        updateosenv(env.getexportedenv())
        os.chdir(env.fs.cwd())
        sys.argv = env.args
        if stdin is not None:
            sys.stdin = WrapStdIO(stdin)
        if stdout is not None:
            sys.stdout = WrapStdIO(stdout)
        if stderr is not None:
            sys.stderr = WrapStdIO(stderr)
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
        if os.getcwd() != origcwd:
            os.chdir(origcwd)


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
            # calls native funtion unsetenv
            del os.environ[name]
        else:
            # calls native funtion putenv
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
