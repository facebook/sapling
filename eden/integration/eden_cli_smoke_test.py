#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

"""
Smoke test for edenFS CLI commands

This test verifies that critical edenFS CLI commands can be
loaded and executed smoothly

The goal of this test is to catch packaging/dependency issues
early by verifying that CLI commands can be invoked (regardless
of EdenFS not running or gracefully fail due to other errors)
"""

import subprocess

from .lib import edenclient, testcase


@testcase.eden_test
class EdenCLISmokeTest(testcase.IntegrationTestCase):
    """
    Smoke test to verify EdenFS CLI commands can be loaded without
    import errors.
    These tests do not require a running EdenFS daemon - they just verify that
    the CLI code can be imported and executed. This catches dependency issues
    like missing Python modules that would prevent the CLI from running at all.
    """

    def test_eden_doctor_loads_without_error(self) -> None:
        """
        Verify 'eden doctor' can at least load its Python code without
        import/module errors.
        """
        with edenclient.EdenFS() as client:
            # Run doctor with --dry-run to avoid any actual fixes
            # We expect this to either succeed or fail gracefully (non-zero exit)
            # but NOT crash with an import error
            cmd_result = client.run_unchecked(
                "doctor", "--dry-run", stdout=subprocess.PIPE, stderr=subprocess.PIPE
            )
            # The command might fail (e.g., no mounts configured), but it should
            # not crash with a Python import error. Import errors typically show
            # "ModuleNotFoundError" or "ImportError" in stderr.
            stderr_output = (
                cmd_result.stderr.decode("utf-8", errors="replace")
                if cmd_result.stderr
                else ""
            )
            self.assertNotIn(
                "ModuleNotFoundError",
                stderr_output,
                f"eden doctor failed with missing module error: {stderr_output}",
            )
            self.assertNotIn(
                "ImportError",
                stderr_output,
                f"eden doctor failed with import error: {stderr_output}",
            )

    def test_eden_status_loads_without_error(self) -> None:
        """Verify 'eden status' can be invoked without import/module errors."""
        with edenclient.EdenFS() as client:
            cmd_result = client.run_unchecked(
                "status", stdout=subprocess.PIPE, stderr=subprocess.PIPE
            )
            stderr_output = (
                cmd_result.stderr.decode("utf-8", errors="replace")
                if cmd_result.stderr
                else ""
            )
            self.assertNotIn(
                "ModuleNotFoundError",
                stderr_output,
                f"eden status failed with missing module error: {stderr_output}",
            )
            self.assertNotIn(
                "ImportError",
                stderr_output,
                f"eden status failed with import error: {stderr_output}",
            )

    def test_eden_list_loads_without_error(self) -> None:
        """Verify 'eden list' can be invoked without import/module errors."""
        with edenclient.EdenFS() as client:
            cmd_result = client.run_unchecked(
                "list", stdout=subprocess.PIPE, stderr=subprocess.PIPE
            )
            stderr_output = (
                cmd_result.stderr.decode("utf-8", errors="replace")
                if cmd_result.stderr
                else ""
            )
            self.assertNotIn(
                "ModuleNotFoundError",
                stderr_output,
                f"eden list failed with missing module error: {stderr_output}",
            )
            self.assertNotIn(
                "ImportError",
                stderr_output,
                f"eden list failed with import error: {stderr_output}",
            )
