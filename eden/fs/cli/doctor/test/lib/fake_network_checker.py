#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict
from pathlib import Path

from eden.fs.cli.config import EdenCheckout
from eden.fs.cli.doctor.check_network import NetworkChecker
from eden.fs.cli.doctor.problem import ProblemTracker


class FakeNetworkChecker(NetworkChecker):
    def check_network(
        self,
        tracker: ProblemTracker,
        checkout_backing_repo: Path,
        checked_network_backing_repos: set[str],
        run_repo_checks: bool = True,
    ) -> None:
        return None
