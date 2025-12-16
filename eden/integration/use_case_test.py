#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

from .lib import edenclient, testcase  # noqa


@testcase.eden_repo_test
class UseCaseTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def test_eden_cli_use_case(self) -> None:
        cmd_result = self.eden.run_cmd("list", cwd=self.mount)
        self.assertNotIn("Error reading use case", cmd_result)

        cmd_result = self.eden.run_cmd(
            "list",
            "--use-case=eden-fs-tests",
            "--debug",
            capture_stderr=True,
            cwd=self.mount,
            env={"EDENFS_LOG": "TRACE"},
        )
        self.assertNotIn("Error reading use case", cmd_result)
        self.assertIn("Creating EdenFsInstance with use case: EdenFsTests", cmd_result)

    def test_eden_cli_use_case_returns_without_error_bad_use_case(self) -> None:
        cmd_result = self.eden.run_cmd("list", cwd=self.mount)
        self.assertNotIn("Error reading use case", cmd_result)

        cmd_result = self.eden.run_cmd(
            "list",
            "--use-case=bad-use-case",
            "--debug",
            cwd=self.mount,
            env={"EDENFS_LOG": "TRACE"},
        )
        self.assertNotIn("Error reading use case", cmd_result)
        self.assertIn("Creating EdenFsInstance with use case: Unknown", cmd_result)
