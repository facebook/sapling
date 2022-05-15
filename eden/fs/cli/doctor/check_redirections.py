#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


from eden.fs.cli import mtab
from eden.fs.cli.config import EdenCheckout, EdenInstance
from eden.fs.cli.doctor.problem import FixableProblem, ProblemTracker
from eden.fs.cli.redirect import (
    get_effective_redirections,
    Redirection,
    RedirectionState,
    RedirectionType,
)


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
        self._redir.remove_existing(self._checkout)
        if self._redir.type == RedirectionType.UNKNOWN:
            return
        self._redir.apply(self._checkout)
