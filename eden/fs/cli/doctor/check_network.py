#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import subprocess
from pathlib import Path

from typing import List

from eden.fs.cli.config import EdenCheckout
from eden.fs.cli.doctor.problem import Problem, ProblemSeverity, ProblemTracker
from eden.fs.cli.util import get_environment_suitable_for_subprocess


NETWORK_TIMEOUT = 2.0


class ConnectivityProblem(Problem):
    def __init__(self, errmsg: str) -> None:
        super().__init__(
            f"Encountered an error checking connection to Source Control Servers: {errmsg}",
            remediation="Please check your network connection. If you are connected to the VPN, please try reconnecting.",
            severity=ProblemSeverity.ERROR,
        )


class NetworkChecker:
    def run_command(
        self, args: List[str], cwd: Path
    ) -> subprocess.CompletedProcess[str]:
        env = get_environment_suitable_for_subprocess()
        env["HGPLAIN"] = "1"
        return subprocess.run(
            args,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=cwd,
            check=True,
            text=True,
            timeout=NETWORK_TIMEOUT,
        )

    def check_network(
        self,
        tracker: ProblemTracker,
        checkout: EdenCheckout,
        checked_network_backing_repos: set[str],
    ) -> None:
        if str(checkout.get_backing_repo_path()) in checked_network_backing_repos:
            return
        checked_network_backing_repos.add(str(checkout.get_backing_repo_path()))
        hg = os.environ.get("EDEN_HG_BINARY", "hg")
        cwd = checkout.path
        try:
            self.run_command([hg, "debugnetwork"], cwd)
        except subprocess.CalledProcessError as ex:
            tracker.add_problem(
                ConnectivityProblem(
                    f"command 'hg debugnetworkdoctor' reported an error:\n{ex.stdout}\n{ex.stderr}\n"
                )
            )
            return
        except subprocess.TimeoutExpired:
            tracker.add_problem(
                ConnectivityProblem("command 'hg debugnetworkdoctor' timed out.\n")
            )
            return

        try:
            self.run_command([hg, "debugnetwork", "--connection"], cwd)
        except subprocess.CalledProcessError as ex:
            # TODO: debugnetwork returns a variety of error numbers depending on the specific failure
            # but it should be covered by stdout. Noting in case we want to try to fix any of them
            # in the future.
            tracker.add_problem(
                ConnectivityProblem(
                    f"hg debugnetwork --connection reported an error: {ex.stdout}\n{ex.stderr}\n"
                )
            )
            return
        except subprocess.TimeoutExpired:
            tracker.add_problem(
                ConnectivityProblem(
                    "command 'hg debugnetwork --connection' timed out.\n"
                )
            )
            return
