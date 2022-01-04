#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import subprocess

from eden.fs.cli.config import EdenInstance
from eden.fs.cli.doctor.problem import Problem, ProblemTracker


class KerberosChecker:
    def run_kerberos_certificate_checks(
        self, instance: EdenInstance, tracker: ProblemTracker
    ) -> None:
        if not instance.get_config_bool("doctor.enable-kerberos-check", default=False):
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
