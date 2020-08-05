#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import platform
import subprocess

from eden.fs.cli.doctor.problem import Problem, ProblemTracker


def run_kerberos_certificate_checks(tracker: ProblemTracker) -> None:
    # Skip these checks on Windows machines and on Sandcastle
    if platform.system() == "Windows" or os.environ.get("SANDCASTLE"):
        return

    result = subprocess.call(["klist", "-s"])
    if result:
        tracker.add_problem(KerberosProblem())


class KerberosProblem(Problem):
    def __init__(self) -> None:
        remediation = """\
Run `kdestroy && kinit` to obtain new Kerberos credentials.
            """
        super().__init__(
            "Kerberos ccache is empty or expired.", remediation=remediation
        )
