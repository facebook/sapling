#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from eden.fs.cli.config import EdenInstance
from eden.fs.cli.doctor.check_kerberos import KerberosChecker
from eden.fs.cli.doctor.problem import ProblemTracker


class FakeKerberosChecker(KerberosChecker):
    def run_kerberos_certificate_checks(
        self, instance: EdenInstance, tracker: ProblemTracker
    ) -> None:
        return None
