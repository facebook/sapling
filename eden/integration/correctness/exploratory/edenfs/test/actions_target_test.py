# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import dataclasses
import unittest

from eden.integration.correctness.exploratory.edenfs.actions import ACTION_DEFS
from eden.integration.correctness.exploratory.edenfs.target import RECORDING_TARGET


class RecordingTargetActionsTest(unittest.TestCase):
    def test_recording_target_uses_the_composed_action_table(self) -> None:
        self.assertEqual(frozenset(ACTION_DEFS), frozenset(RECORDING_TARGET.actions))
        for kind, action_def in ACTION_DEFS.items():
            if kind != "fault_install":
                self.assertIs(action_def, RECORDING_TARGET.actions[kind])
        core_fault = ACTION_DEFS["fault_install"]
        target_fault = RECORDING_TARGET.actions["fault_install"]
        for field in dataclasses.fields(core_fault):
            if field.name != "sample":
                self.assertEqual(
                    getattr(core_fault, field.name), getattr(target_fault, field.name)
                )
        self.assertIsNot(core_fault.sample, target_fault.sample)
