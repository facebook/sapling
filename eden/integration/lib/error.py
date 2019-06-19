#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import shlex
import subprocess


class CommandError(subprocess.CalledProcessError):
    """
    A wrapper around subprocess.CalledProcessError that also includes
    includes the process's stderr when converted to a string.
    """

    def __init__(self, orig: subprocess.CalledProcessError) -> None:
        super().__init__(
            orig.returncode, orig.cmd, output=orig.output, stderr=orig.stderr
        )

    def __str__(self) -> str:
        if not self.stderr:
            return super().__str__()

        cmd_str = " ".join(shlex.quote(arg) for arg in self.cmd)

        stderr_str = self.stderr
        if isinstance(self.stderr, bytes):
            stderr_str = self.stderr.decode("utf-8", errors="replace")

        # Indent the stderr output just to help indicate where it starts
        # and ends in the test output.
        stderr_str = stderr_str.replace("\n", "\n  ")

        msg = "Command [%s] failed with status %s\nstderr:\n  %s" % (
            cmd_str,
            self.returncode,
            stderr_str,
        )
        return msg
