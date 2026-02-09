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

# Commands to test. Each tuple contains:
# - List of command arguments to pass to eden
# - Description for test output
COMMANDS_TO_TEST: List[Tuple[List[str], str]] = [
    (["doctor", "--dry-run"], "eden doctor"),
    (["status"], "eden status"),
    (["list"], "eden list"),
    (["info", "--help"], "eden info"),
    (["gc", "--help"], "eden gc"),
    (["rage", "--help"], "eden rage"),
    (["debug", "--help"], "eden debug"),
    (["fsck", "--help"], "eden fsck"),
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

    def _run_command_and_check(
        self, client: edenclient.EdenFS, args: List[str], description: str
    ) -> None:
        """Run an eden command and verify it doesn't have packaging errors.
        Args:
            client: The EdenFS client
            args: Command arguments to pass to eden
            description: Human-readable description for error messages
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

        # Check both stderr and stdout since some error messages may appear in either
        combined_output = stdout_output + stderr_output
        for pattern, error_description in PACKAGING_ERROR_PATTERNS:
            self.assertNotIn(
                pattern,
                combined_output,
                f"{description} failed with {error_description}. "
                f"This may indicate a packaging/dependency issue.\n"
                f"Pattern found: '{pattern}'\n"
                f"stderr: {stderr_output}\n"
                f"stdout: {stdout_output}",
            )

    def test_cli_commands_load_without_packaging_errors(self) -> None:
        """Verify critical Eden CLI commands can be invoked without packaging errors.

        This test checks multiple CLI commands to ensure they can be loaded and
        executed without packaging errors. Commands may fail for
        legitimate reasons (e.g., EdenFS not running), but they should not crash
        due to missing modules or other packaging issues.
        """
        with edenclient.EdenFS() as client:
            for args, description in COMMANDS_TO_TEST:
                with self.subTest(command=description):
                    self._run_command_and_check(client, args, description)
