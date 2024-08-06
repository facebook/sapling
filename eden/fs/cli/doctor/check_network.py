#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from eden.fs.cli.config import EdenCheckout
from eden.fs.cli.doctor.problem import Problem, ProblemTracker


class NetworkDoctorProblem(Problem):
    def __init__(self, doctor_output: str) -> None:
        super().__init__(
            f"hg debugnetworkdoctor reported an error: {doctor_output}",
            remediation="Check the network report in hg debugnetworkdoctor",
        )


def check_network(tracker: ProblemTracker, checkout: EdenCheckout) -> None:
    pass
