#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import os

from .lib import edenclient, testcase


NOT_MOUNTED_DIR_LIST = ["README_EDEN.txt"]


@testcase.eden_repo_test
class RemountTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.symlink("slink", "hello")
        self.repo.commit("Initial commit.")

    def _assert_mounted(self, mount_path: str) -> None:
        expected_entries = {".eden", "adir", "hello", "slink"}
        self.assert_checkout_root_entries(expected_entries, path=mount_path)

    def select_storage_engine(self) -> str:
        """ we need to persist data across restarts """
        return "sqlite"

    def test_remount_basic(self) -> None:
        # Mount multiple clients
        self._clone_checkouts(5)

        self.eden.shutdown()
        self.eden.start()

        # Verify that clients are remounted on startup
        for i in range(5):
            self._assert_mounted(f"{self.mount}-{i}")

        # Verify that default repo created by EdenRepoTestBase is remounted
        self._assert_mounted(self.mount)

    def test_git_and_hg(self) -> None:
        # Create git and hg repositories for mounting
        repo_names = {"git": "git_repo", "hg": "hg_repo"}
        git_repo = self.create_git_repo(repo_names["git"])
        hg_repo = self.create_hg_repo(repo_names["hg"])

        git_repo.write_file("hello", "hola\n")
        git_repo.commit("Initial commit.")

        hg_repo.write_file("hello", "hola\n")
        hg_repo.commit("Initial commit.")

        self.eden.add_repository(repo_names["git"], git_repo.path)
        self.eden.add_repository(repo_names["hg"], hg_repo.path)

        # Mount git and hg clients
        for name in repo_names.values():
            for i in range(3):
                self.eden.clone(
                    name, os.path.join(self.mounts_dir, name + "-" + str(i))
                )

        self.eden.shutdown()
        self.eden.start()

        # Verify that clients are remounted on startup
        for scm_type, name in repo_names.items():
            for i in range(3):
                mount_path = os.path.join(self.mounts_dir, f"{name}-{i}")
                self.assert_checkout_root_entries(
                    {".eden", "hello"}, mount_path, scm_type=scm_type
                )

                hello = os.path.join(mount_path, "hello")
                with open(hello, "r") as f:
                    self.assertEqual("hola\n", f.read())

    def test_partial_unmount(self) -> None:
        self._clone_checkouts(5)

        # Remove the main checkout
        self.eden.remove(self.mount)
        self.assertFalse(os.path.exists(self.mount))

        self.eden.shutdown()
        self.eden.start()

        # Verify that clients that were still mounted at shutdown are remounted
        for i in range(5):
            self._assert_mounted(f"{self.mount}-{i}")

        # Verify that unmounted client is not remounted
        self.assertFalse(os.path.exists(self.mount))

    def test_restart_twice(self) -> None:
        self.maxDiff = None
        self._clone_checkouts(5)

        self.eden.shutdown()
        self.eden.start()

        # Remove some checkouts
        self.eden.remove(self.mount)
        self.eden.remove(self.mount + "-3")

        self.eden.shutdown()
        self.eden.start()

        # Verify that clients that were still mounted at shutdown are remounted
        checkouts = self.eden.list_cmd_simple()
        expected_checkouts = {
            f"{self.mount}-{i}": "RUNNING" for i in range(5) if i != 3
        }
        self.assertEqual(expected_checkouts, checkouts)

        for i in range(5):
            if i == 3:
                continue
            self._assert_mounted(f"{self.mount}-{i}")

        # Verify that unmounted clients are not remounted
        self.assertFalse(os.path.exists(self.mount))
        self.assertFalse(os.path.exists(self.mount + "3"))

    def test_try_remount_existing_mount(self) -> None:
        """Verify trying to mount an existing mount prints a sensible error."""
        mount_destination = self.mount + "-0"
        self.eden.clone(self.repo_name, mount_destination)
        with self.assertRaises(edenclient.EdenCommandError) as context:
            self.eden.run_cmd("mount", mount_destination)
        self.assertIn(
            (
                b"ERROR: Mount point in use! %s is already mounted by Eden.\n"
                % mount_destination.encode()
            ),
            context.exception.stderr,
        )
        self.assertEqual(1, context.exception.returncode)

    def test_empty_config_json(self) -> None:
        self._clone_checkouts(5)

        # Clear the contents of config.json file
        open(os.path.join(self.eden_dir, "config.json"), "w").close()

        self.eden.shutdown()
        self.eden.start()

        self._verify_not_mounted(5)

    def test_deleted_config_json(self) -> None:
        self._clone_checkouts(5)

        # Delete the config.json file
        os.remove(os.path.join(self.eden_dir, "config.json"))

        self.eden.shutdown()
        self.eden.start()

        self._verify_not_mounted(5)

    def test_incorrect_config_json(self) -> None:
        self._clone_checkouts(5)

        # Reload config.json file with incorrect data
        config_data = {
            self.mounts_dir + "/incorrectdir1": "jskdnbailreylbflhv",
            self.mounts_dir + "/incorrectdir2": "ndjnhaleruybfialus",
        }

        with open(os.path.join(self.eden_dir, "config.json"), "w") as f:
            json.dump(config_data, f, indent=2, sort_keys=True)
            f.write("\n")

        self.eden.shutdown()
        self.eden.start()

        self._verify_not_mounted(5)

        # Incorrect mount paths from config.json should not have been created
        for incorrect_mount in config_data:
            self.assertFalse(os.path.isdir(incorrect_mount))

    def test_bad_config_json(self) -> None:
        self._clone_checkouts(5)

        # Reload config.json file with random data
        with open(os.path.join(self.eden_dir, "config.json"), "w") as f:
            f.write("njfaeriurbvailuwrawikc\n")

        self.eden.shutdown()
        self.eden.start()
        self._verify_not_mounted(5)

    def _clone_checkouts(self, num_mounts):
        for i in range(num_mounts):
            self.eden.clone(self.repo_name, self.mount + "-" + str(i))

    def _verify_not_mounted(self, num_mounts, main_mounted=False):
        # Verify that no clients are remounted. No errors should be thrown here
        for i in range(num_mounts):
            entries = sorted(os.listdir(self.mount + "-" + str(i)))
            self.assertEqual(NOT_MOUNTED_DIR_LIST, entries)

        if not main_mounted:
            entries = sorted(os.listdir(self.mount))
            self.assertEqual(NOT_MOUNTED_DIR_LIST, entries)
