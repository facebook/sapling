#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import sys

from eden.fs.cli import mtab
from eden.fs.cli.config import EdenCheckout, EdenInstance
from eden.fs.cli.doctor.problem import FixableProblem, ProblemTracker, RemediationError
from eden.fs.cli.redirect import (
    check_redirection,
    get_effective_redirections,
    Redirection,
    RedirectionState,
    RedirectionType,
)

try:
    from .facebook.internal_consts import get_doctor_link
except ImportError:

    def get_doctor_link() -> str:
        return ""


def check_redirections(
    tracker: ProblemTracker,
    instance: EdenInstance,
    checkout: EdenCheckout,
    mount_table: mtab.MountTable,
) -> None:
    redirs = get_effective_redirections(checkout, mount_table, instance)

    for redir in redirs.values():
        if redir.state == RedirectionState.MATCHES_CONFIGURATION:
            continue
        tracker.add_problem(MisconfiguredRedirection(redir, checkout))


class MisconfiguredRedirection(FixableProblem):
    def __init__(self, redir: Redirection, checkout: EdenCheckout) -> None:
        self._redir: Redirection = redir
        self._checkout: EdenCheckout = checkout

    def description(self) -> str:
        return f"Misconfigured redirection at {self._redir.repo_path}"

    def dry_run_msg(self) -> str:
        return f"Would fix redirection at {self._redir.repo_path}"

    def start_msg(self) -> str:
        return f"Fixing redirection at {self._redir.repo_path}"

    def perform_fix(self) -> None:
        if self._redir.type == RedirectionType.UNKNOWN:
            return
        try:
            self._redir.apply(self._checkout)
        except OSError as err:
            # pyre-ignore[16]: winerror only exists on windows
            if sys.platform == "win32" and err.winerror == 1314:
                # Missing permissions - this usually means that
                # the user cannot create symlinks
                msg = (
                    f"Error occured when trying to create symlink: {err}.\n"
                    "User is missing permissions to create symlinks.\n"
                    "Check that the Developer Mode has been enabled in Windows, "
                    "or that the user is allowed to create symlinks in the Local Security Policy.\n"
                    f"Running chef may fix this. See {get_doctor_link()} for more information."
                )
                raise RemediationError(msg)
            # Pass other errors
            raise

    def check_fix(self) -> bool:
        return check_redirection(self._redir, self._checkout)
