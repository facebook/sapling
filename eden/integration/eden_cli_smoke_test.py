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
from typing import List, Tuple

from .lib import edenclient, testcase

# Error patterns that indicate packaging/dependency issues.
# These are checked in stderr output to detect problems like missing modules,
# incompatible Python versions, or API mismatches between bundled modules.
PACKAGING_ERROR_PATTERNS: List[Tuple[str, str]] = [
    # Module completely missing from package (Python 3.6+)
    ("ModuleNotFoundError", "missing module"),
    # General import failure (parent of ModuleNotFoundError)
    ("ImportError", "import error"),
    # Incompatible Python version or syntax issues in bundled code
    ("SyntaxError", "syntax error"),
    # Module exists but missing expected attributes (partial/incompatible module)
    ("AttributeError: module", "module attribute error"),
    # Type/API incompatibility between modules
    ("TypeError: ", "type error indicating API incompatibility"),
    # General catch-all for any unhandled Python exception
    ("Traceback (most recent call last):", "unhandled Python exception"),
]


@testcase.eden_test
class EdenCLISmokeTest(testcase.IntegrationTestCase):
    """
    Smoke test to verify EdenFS CLI commands can be loaded without
    import errors.
    These tests do not require a running EdenFS daemon - they just verify that
    the CLI code can be imported and executed. This catches dependency issues
    like missing Python modules that would prevent the CLI from running at all.
    """

    def _assert_no_packaging_errors(
        self, command: str, stdout_output: str, stderr_output: str
    ) -> None:
        """Check that command output doesn't contain packaging error patterns.

        Args:
            command: The eden command that was run (for error messages)
            stdout_output: The stdout output from the command (some errors go here)
            stderr_output: The stderr output from the command
        """
        # Check both stderr and stdout since some error messages may appear in either
        combined_output = stdout_output + stderr_output
        for pattern, description in PACKAGING_ERROR_PATTERNS:
            self.assertNotIn(
                pattern,
                combined_output,
                f"eden {command} failed with {description}. "
                f"This may indicate a packaging/dependency issue.\n"
                f"Pattern found: '{pattern}'\n"
                f"stderr: {stderr_output}\n"
                f"stdout: {stdout_output}",
            )

    def _get_command_output(
        self, client: edenclient.EdenFS, *args: str
    ) -> Tuple[str, str]:
        """Run an eden command and return its output.

        Args:
            client: The EdenFS client
            *args: Command arguments to pass to eden

        Returns:
            Tuple of (stdout_output, stderr_output)
        """
        cmd_result = client.run_unchecked(
            *args, stdout=subprocess.PIPE, stderr=subprocess.PIPE
        )
        stdout_output = (
            cmd_result.stdout.decode("utf-8", errors="replace")
            if cmd_result.stdout
            else ""
        )
        stderr_output = (
            cmd_result.stderr.decode("utf-8", errors="replace")
            if cmd_result.stderr
            else ""
        )
        # This is a Smoke Test, and we don't want to check the
        # cmd_result.returncode in this test

        return stdout_output, stderr_output

    def test_eden_doctor_loads_without_error(self) -> None:
        """
        Verify 'eden doctor' can at least load its Python code without
        packaging errors.
        """
        with edenclient.EdenFS() as client:
            # Run doctor with --dry-run to avoid any actual fixes
            # We expect this to either succeed or fail gracefully (non-zero exit)
            # but NOT crash with an packaging error
            stdout, stderr = self._get_command_output(client, "doctor", "--dry-run")
            self._assert_no_packaging_errors("doctor --dry-run", stdout, stderr)

    def test_eden_status_loads_without_error(self) -> None:
        """Verify 'eden status' can be invoked without packaging errors."""
        with edenclient.EdenFS() as client:
            stdout, stderr = self._get_command_output(client, "status")
            self._assert_no_packaging_errors("status", stdout, stderr)

    def test_eden_list_loads_without_error(self) -> None:
        """Verify 'eden list' can be invoked without packaging errors."""
        with edenclient.EdenFS() as client:
            stdout, stderr = self._get_command_output(client, "list")
            self._assert_no_packaging_errors("list", stdout, stderr)
