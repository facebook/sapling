# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import io
import os
import subprocess
from pathlib import Path
from subprocess import CompletedProcess
from typing import Any, Dict, List

from .commit import Commit
from .file import File
from .util import override_environ, test_globals, trace

hg_bin = Path(os.environ["HGTEST_HG"])


class CliCmd:
    cwd: Path
    _env: Dict[str, str]
    EXEC: Path = Path("")

    def __init__(self, cwd: Path, env: Dict[str, str]) -> None:
        self.cwd = cwd
        self._env = env

    def __getattr__(self, command: str):
        """
        This magic allows a cli invocation like:

          hg commit file1 --message "blah" --amend

        to be invoked from Python as:

          hg.commit(file1, message="blah", amend=True)

        The return value is a subprocess.CompletedProcess. If the command fails,
        a CommandFailure exception is raised, containing the CompletedProcess.

        There are a few special arguments, which are not passed to the
        underlying cli.

        - `stdin="..."` takes a string which is passed to the cli on stdin.
        - `binary_output=True` causes the stdout and stderr variables on the
          output to be bytes instead of utf8 strings.

        Note that for now this shells out to a new Mercurial process. In the
        future we can make this invoke the commands inside the test process.
        """

        def func(*args: Any, **kwargs: Any):
            input = kwargs.pop("stdin", "").encode("utf8")
            binary = kwargs.get("binary_output", False)

            env = os.environ.copy()
            env.update(self._env)
            env.update(kwargs.pop("env", {}))

            cmd_args = []

            def process_arg(value: Any):
                if isinstance(value, str):
                    cmd_args.append(value)
                elif isinstance(value, Commit):
                    cmd_args.append(value.hash)
                elif isinstance(value, File):
                    cmd_args.append(value.path)
                elif isinstance(value, Path):
                    cmd_args.append(str(value))
                else:
                    raise ValueError(
                        "clicmd does not support type %s ('%s')" % (type(arg), arg)
                    )

            for arg in args:
                process_arg(arg)

            def process_kwarg(key: str, value: Any):
                key = key.replace("_", "-")
                prefix = "--" if len(key) != 1 else "-"
                option = "%s%s" % (prefix, key)
                if isinstance(value, bool):
                    if value:
                        cmd_args.append(option)
                elif isinstance(value, str):
                    cmd_args.extend([option, value])
                elif isinstance(value, Commit):
                    cmd_args.extend([option, value.hash])
                elif isinstance(value, File):
                    cmd_args.extend([option, value.path])
                elif isinstance(value, list):
                    for v in value:
                        process_kwarg(key, v)
                elif value is None:
                    # This allows code to pass Optional[]'s more easily, and we
                    # can just ignore them.
                    return
                else:
                    raise ValueError(
                        "clicmd does not support type %s ('%s')" % (type(value), value)
                    )

            for key, value in kwargs.items():
                process_kwarg(key, value)

            trace_output = f"$ hg {command}"
            for arg in cmd_args:
                if " " in arg:
                    arg = f'"{arg}"'
                trace_output += f" {arg}"
            trace(trace_output)

            if os.environ.get("HGTEST_SHELLOUT", False):
                result = self._shellout(command, cmd_args, env, input)
            else:
                result = self._inproc(command, cmd_args, env, input)

            if not binary:
                result.stdout = result.stdout.decode("utf8", errors="replace")
                result.stderr = result.stderr.decode("utf8", errors="replace")

            if result.stdout:
                trace(result.stdout)
            if result.stderr:
                trace(result.stderr)
            if not result.stdout and not result.stderr:
                trace("(no output)")

            # Raise our own exception instead of using check=True because the
            # default exception doesn't have the stdout/stderr output.
            if result.returncode != 0:
                trace(f"(exit code: {result.returncode})")
                raise CommandFailure(result)

            # Newline to space out the commands.
            trace("")

            return result

        return func

    def _shellout(
        self, command: str, args: List[str], env: Dict[str, str], stdin: bytes
    ):
        return subprocess.run(
            [str(type(self).EXEC), command] + args,
            capture_output=True,
            cwd=self.cwd,
            env=env,
            input=stdin,
        )

    def _inproc(self, command: str, args: List[str], env: Dict[str, str], stdin: bytes):
        with override_environ(env):
            old_cwd = os.getcwd()
            os.chdir(self.cwd)
            try:
                args = ["hg", command] + args
                fout = io.BytesIO()
                ferr = io.BytesIO()
                fin = io.BytesIO(stdin or b"")
                import bindings

                returncode = bindings.commands.run(args, fin, fout, ferr)
                return subprocess.CompletedProcess(
                    args,
                    returncode,
                    stdout=fout.getvalue(),
                    stderr=ferr.getvalue(),
                )
            finally:
                os.chdir(old_cwd)


class hg(CliCmd):
    EXEC: Path = hg_bin

    def __init__(self, root: Path) -> None:
        env = test_globals.env.copy()
        super().__init__(root, env)


class CommandFailure(Exception):
    def __init__(self, result) -> None:
        self.result = result

    def __str__(self) -> str:
        return "Command Failure: %s\nStdOut: %s\nStdErr: %s\n" % (
            " ".join(str(s) for s in self.result.args),
            self.result.stdout,
            self.result.stderr,
        )
