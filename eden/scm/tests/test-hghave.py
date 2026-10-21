# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import os
import shlex
import subprocess
import sys
import unittest
from unittest import mock

import hghave
import silenttestrunner
from sapling.testing.t import hghave as modernhghave


class TimeoutPopen(subprocess.Popen):
    """Bound the regression's real process waits without changing pipe behavior."""

    def wait(self, timeout=None):
        return super().wait(timeout=30 if timeout is None else timeout)

    def communicate(self, input=None, timeout=None):
        return super().communicate(input, timeout=30 if timeout is None else timeout)


@unittest.skipIf(os.name == "nt", "requires POSIX shell exec for child ownership")
class MatchOutputTest(unittest.TestCase):
    feature_probe = hghave

    def matchoutput(self, code, regexp=rb"marker$", ignorestatus=False):
        python = os.environ.get("PYTHON", sys.executable)
        command = "exec " + " ".join(shlex.quote(arg) for arg in [python, "-c", code])
        child = None

        def popen(*args, **kwargs):
            nonlocal child
            child = TimeoutPopen(*args, **kwargs)
            return child

        with mock.patch.object(self.feature_probe.subprocess, "Popen", popen):
            try:
                return self.feature_probe.matchoutput(
                    command, regexp, ignorestatus=ignorestatus
                )
            except subprocess.TimeoutExpired:
                self.fail(
                    "feature probe did not drain subprocess output before waiting"
                )
            finally:
                if child is not None:
                    if child.poll() is None:
                        child.kill()
                    child.communicate()

    def test_large_output_and_merged_stderr(self):
        match = self.matchoutput(
            "import sys; sys.stdout.write('x' * (1024 * 1024)); "
            "sys.stdout.flush(); sys.stderr.write('marker')"
        )
        self.assertIsNotNone(match)
        self.assertEqual(match.group(), b"marker")
        self.assertEqual(match.start(), 1024 * 1024)


class ModernMatchOutputTest(MatchOutputTest):
    """Apply the same probe contract to the modern .t runner."""

    feature_probe = modernhghave


if __name__ == "__main__":
    silenttestrunner.main(__name__)
