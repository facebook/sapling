#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import binascii
import unittest
from typing import Tuple

import eden.cli.doctor as doctor
import eden.dirstate
from eden.cli.config import EdenCheckout
from eden.cli.test.lib.output import TestOutput
from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from .fake_process_finder import FakeProcessFinder


class DoctorTestBase(unittest.TestCase, TemporaryDirectoryMixin):
    def create_fixer(self, dry_run: bool) -> Tuple[doctor.ProblemFixer, TestOutput]:
        out = TestOutput()
        if not dry_run:
            fixer = doctor.ProblemFixer(out)
        else:
            fixer = doctor.DryRunFixer(out)
        return fixer, out

    def assert_results(
        self,
        fixer: doctor.ProblemFixer,
        num_problems: int = 0,
        num_fixed_problems: int = 0,
        num_failed_fixes: int = 0,
        num_manual_fixes: int = 0,
    ) -> None:
        self.assertEqual(num_problems, fixer.num_problems)
        self.assertEqual(num_fixed_problems, fixer.num_fixed_problems)
        self.assertEqual(num_failed_fixes, fixer.num_failed_fixes)
        self.assertEqual(num_manual_fixes, fixer.num_manual_fixes)

    def assert_dirstate_p0(self, checkout: EdenCheckout, commit: str) -> None:
        dirstate_path = checkout.path / ".hg" / "dirstate"
        with dirstate_path.open("rb") as f:
            parents, _tuples_dict, _copymap = eden.dirstate.read(f, str(dirstate_path))
        self.assertEqual(binascii.hexlify(parents[0]).decode("utf-8"), commit)

    def make_process_finder(self) -> FakeProcessFinder:
        return FakeProcessFinder(self.make_temporary_directory())
