#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Send the reason for a failed command to the Rust edenfsctl wrapper.

The wrapper gives the path of a report file in the `EDENFSCTL_ERROR_REPORT`
environment variable. The report is plain text. If an exception caused an
error, the error starts with the exception type, for example
"DaemonBinaryNotFound: unable to find edenfs executable".
"""

import os
from typing import Optional

# Keep this name the same as `ERROR_REPORT_ENV` in
# `eden/fs/cli_rs/edenfsctl/src/error_report.rs`.
ERROR_REPORT_ENV = "EDENFSCTL_ERROR_REPORT"


class ErrorReport:
    def __init__(self, path: Optional[str]) -> None:
        self._path = path
        self._errors: list[str] = []

    def record(self, message: str, exc: Optional[BaseException] = None) -> None:
        """Record an error of this command. Ignore a repeated error."""
        if exc is not None:
            name = type(exc).__name__
            message = f"{name}: {message}" if message else name
        if message not in self._errors:
            self._errors.append(message)

    def write(self, exit_code: int) -> None:
        """Write the recorded errors to the report file if `exit_code` is not 0.

        The last recorded error comes first. The earlier errors follow it in
        reverse order. A ": " separates each error from the next one. Do not
        create the file. Ignore errors from the file system.
        """
        if exit_code == 0 or self._path is None or not self._errors:
            return
        try:
            fd = os.open(self._path, os.O_WRONLY | os.O_TRUNC)
            with os.fdopen(fd, "w", encoding="utf-8", errors="replace") as f:
                f.write(": ".join(reversed(self._errors)))
        except OSError:
            pass


_report = ErrorReport(None)


def init() -> None:
    """Take the report path from the environment.

    Remove the variable from the environment. Then child processes cannot
    write to the report.
    """
    global _report
    _report = ErrorReport(os.environ.pop(ERROR_REPORT_ENV, None))


def record(message: str, exc: Optional[BaseException] = None) -> None:
    _report.record(message, exc)


def write(exit_code: int) -> None:
    _report.write(exit_code)
