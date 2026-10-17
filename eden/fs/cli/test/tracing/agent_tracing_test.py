# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import unittest
from unittest.mock import MagicMock, patch

from eden.fs.cli import agent_tracing


class ExitInterruptedTest(unittest.TestCase):
    def exit_code(self, binding: object) -> int:
        with (
            patch.object(agent_tracing, "art_cli_lite", binding),
            patch.object(os, "_exit", side_effect=SystemExit) as exited,
            self.assertRaises(SystemExit),
        ):
            agent_tracing.exit_interrupted()
        exited.assert_called_once()
        return exited.call_args.args[0]

    def test_finishes_within_one_second_then_exits_130(self) -> None:
        binding = MagicMock()
        self.assertEqual(self.exit_code(binding), 130)
        binding.finish.assert_called_once_with(timeout_s=1)

    def test_a_second_interrupt_during_finish_still_exits_130(self) -> None:
        binding = MagicMock()
        binding.finish.side_effect = KeyboardInterrupt
        self.assertEqual(self.exit_code(binding), 130)

    def test_a_failing_finish_still_exits_130(self) -> None:
        binding = MagicMock()
        binding.finish.side_effect = RuntimeError("can't start new thread")
        self.assertEqual(self.exit_code(binding), 130)

    def test_without_the_binding_it_exits_130(self) -> None:
        self.assertEqual(self.exit_code(None), 130)
