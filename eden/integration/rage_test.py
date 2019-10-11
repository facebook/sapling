#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import logging

from .lib import repobase, testcase


class RageTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("README.md", "docs\n")
        self.repo.commit("Initial commit.")

    def create_repo(self, name: str) -> repobase.Repository:
        return self.create_hg_repo(name)

    def test_rage_output(self) -> None:
        output = self.eden.run_cmd("rage", "--stdout")
        logging.info(f"Rage output:\n{output}")
        self.assert_output_includes_unconditional_checks(output)
        self.assertRegex(output, r"\nbuild_package_release\s*:")
        self.assertRegex(output, r"\nbuild_package_version\s*:")
        self.assertRegex(output, r"\nuptime\s*:")
        self.assertIn(f"\nChecking {self.mount}\n", output)
        self.assertIn("edenfs memory usage", output)

    def test_rage_output_with_stopped_daemon(self) -> None:
        self.eden.shutdown()
        output = self.eden.run_cmd("rage", "--stdout")
        logging.info(f"Rage output:\n{output}")
        self.assert_output_includes_unconditional_checks(output)

    def assert_output_includes_unconditional_checks(self, output: str) -> None:
        # Check to make sure that various important sections of information
        # are present in the rage output.
        #
        # We may need to update this in the future if we modify the rage output; the
        # main purpose it simply to make sure that the rage command did not exit early
        # or crash partway through the output.
        self.assertRegex(output, r"^User\s*:")
        self.assertRegex(output, r"\nHostname\s*:")
        # Allow the test to succeed even if the fb-eden RPM is not installed
        self.assertRegex(output, r"\n(Rpm Version\s*:|Error getting the Rpm version)")
        self.assertIn("\neden doctor --dry-run", output)
        self.assertIn("\nMost recent Eden logs:\n", output)
        self.assertIn("\nList of running Eden processes:\n", output)
        self.assertIn("\nList of mount points:\n", output)
        self.assertIn(f"\nMount point info for path {self.mount}:\n", output)
        self.assertIn(f"{self.mount}\n", output)
