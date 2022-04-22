# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import os
import subprocess
from pathlib import Path
from subprocess import CompletedProcess
from typing import Any, Callable, Dict, Union

from .util import test_globals

hg_bin = Path(os.environ["HGTEST_HG"])


class CliCmd:
    cwd: Path
    env: Dict[str, str]
    EXEC: Path = Path("")

    def __init__(self, cwd: Path, env: Dict[str, str]) -> None:
        self.cwd = cwd
        self.env = env

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

        def func(*args: str, **kwargs: str):
            input = kwargs.pop("stdin", None)
            if input:
                input = input.encode("utf8")
            binary = kwargs.get("binary_output", False)

            cmd_args = list(args)
            for key, value in kwargs.items():
                key = key.replace("_", "-")
                prefix = "--" if len(key) != 1 else "-"
                option = "%s%s" % (prefix, key)
                if isinstance(value, bool):
                    if value:
                        cmd_args.append(option)
                elif isinstance(value, str):
                    cmd_args.extend([option, value])
                else:
                    raise ValueError(
                        "clicmd does not support type %s ('%s')" % (type(value), value)
                    )

            env = os.environ.copy()
            env.update(self.env)
            result = subprocess.run(
                [str(type(self).EXEC), command] + cmd_args,
                capture_output=True,
                cwd=self.cwd,
                env=env,
                input=input,
            )
            # Raise our own exception instead of using check=True because the
            # default exception doesn't have the stdout/stderr output.
            if result.returncode != 0:
                raise CommandFailure(result)

            if not binary:
                result.stdout = result.stdout.decode("utf8", errors="replace")
                result.stderr = result.stderr.decode("utf8", errors="replace")
            return result

        return func


class hg(CliCmd):
    EXEC: Path = hg_bin

    def __init__(self, root: Path) -> None:
        env = test_globals.env.copy()
        env["TESTTMP"] = str(test_globals.test_tmp)
        super().__init__(root, env)


class CommandFailure(Exception):
    def __init__(self, result) -> None:
        self.result = result

    def __str__(self) -> str:
        return "Command Failure: %s\nStdOut: %s\nStdErr: %s\n" % (
            " ".join(str(s) for s in self.result.args),
            self.result.stdout.decode("utf8", errors="replace"),
            self.result.stderr.decode("utf8", errors="replace"),
        )
