#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import os
import subprocess
import sys
import typing
from pathlib import Path
from textwrap import dedent
from typing import Optional, Sequence, Set

import pexpect
from eden.integration.lib.hgrepo import HgRepository

from .lib import edenclient, testcase
from .lib.fake_edenfs import get_fake_edenfs_argv
from .lib.find_executables import FindExe
from .lib.pexpect import PexpectAssertionMixin, wait_for_pexpect_process
from .lib.service_test_case import (
    ServiceTestCaseBase,
    SystemdServiceTestCaseMarker,
    service_test,
)


# This is the name of the default repository created by EdenRepoTestBase.
repo_name = "main"


@testcase.eden_repo_test
class CloneTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def test_clone_to_non_existent_directory(self) -> None:
        tmp = self.make_temporary_directory()
        non_existent_dir = os.path.join(tmp, "foo/bar/baz")

        self.eden.run_cmd("clone", repo_name, non_existent_dir)
        self.assertTrue(
            os.path.isfile(os.path.join(non_existent_dir, "hello")),
            msg="clone should succeed in non-existent directory",
        )

    def test_clone_to_dir_under_symlink(self) -> None:
        tmp = self.make_temporary_directory()
        empty_dir = os.path.join(tmp, "foo/bar")
        os.makedirs(empty_dir)

        symlink_dir = os.path.join(tmp, "food")
        os.symlink(os.path.join(tmp, "foo"), symlink_dir)

        symlinked_target = os.path.join(symlink_dir, "bar")

        self.eden.run_cmd("clone", repo_name, symlinked_target)
        self.assertTrue(
            os.path.isfile(os.path.join(empty_dir, "hello")),
            msg="clone should succeed in empty directory",
        )

        with self.get_thrift_client() as client:
            active_mount_points: Set[Optional[str]] = {
                os.fsdecode(mount.mountPoint) for mount in client.listMounts()
            }
            self.assertIn(
                empty_dir, active_mount_points, msg="mounted using the realpath"
            )

        self.eden.run_cmd("remove", "--yes", symlinked_target)

    def test_clone_to_existing_empty_directory(self) -> None:
        tmp = self.make_temporary_directory()
        empty_dir = os.path.join(tmp, "foo/bar/baz")
        os.makedirs(empty_dir)

        self.eden.run_cmd("clone", repo_name, empty_dir)
        self.assertTrue(
            os.path.isfile(os.path.join(empty_dir, "hello")),
            msg="clone should succeed in empty directory",
        )

    def test_clone_from_repo(self) -> None:
        # Specify the source of the clone as an existing local repo rather than
        # an alias for a config.
        destination_dir = self.make_temporary_directory()
        self.eden.run_cmd("clone", self.repo.path, destination_dir)
        self.assertTrue(
            os.path.isfile(os.path.join(destination_dir, "hello")),
            msg="clone should succeed in empty directory",
        )

    def test_clone_with_arcconfig(self) -> None:
        project_id = "special_project"

        # Remember this state for later
        before_arcconfig = self.repo.get_head_hash()

        # Add an .arcconfig file to the repository
        arcconfig_data = {
            "project_id": project_id,
            "conduit_uri": "https://phabricator.example.com/api/",
            "phutil_libraries": {"foo": "foo/arcanist/"},
        }
        self.repo.write_file(".arcconfig", json.dumps(arcconfig_data) + "\n")
        self.repo.commit("Add .arcconfig")

        # Add a config alias for a repo with some bind mounts.
        edenrc = os.path.join(self.home_dir, ".edenrc")
        with open(edenrc, "w") as f:
            f.write(
                dedent(
                    f"""\
            ["repository {project_id}"]
            path = "{self.repo.get_canonical_root()}"
            type = "{self.repo.get_type()}"

            ["bindmounts {project_id}"]
            mnt1 = "foo/stuff/build_output"
            mnt2 = "node_modules"
            """
                )
            )

        # Clone the repository using its path.
        # We should find the config from the project_id field in
        # the .arcconfig file
        eden_clone = self.make_temporary_directory()
        self.eden.run_cmd("clone", self.repo.path, eden_clone)
        self.assertTrue(
            os.path.isdir(os.path.join(eden_clone, "foo/stuff/build_output")),
            msg="clone should create bind mounts",
        )
        self.assertTrue(
            os.path.isdir(os.path.join(eden_clone, "node_modules")),
            msg="clone should create bind mounts",
        )

        # Let's also check that passing in a rev is effective when
        # the repo is "bare".  We're not actually making it bare here
        # as I'm not sure if we support that concept with git also,
        # so instead I'm moving the repo back to before the arcconfig
        # exists to simulate a similar situation.  The problem that we're
        # testing here is (for mercurial at least), since the default head
        # rev is '.', if the source repo doesn't have an arcconfig we'd
        # never set up the bindmounts, even if the --rev option was passed in.

        # TODO: GitRepository doesn't yet have an update() method, so make
        # this hg specific for now.
        if self.repo.get_type() == "hg":
            assert isinstance(self.repo, HgRepository)
            head_rev = self.repo.get_head_hash()
            self.repo.update(before_arcconfig)
            alt_eden_clone = self.make_temporary_directory()
            self.eden.run_cmd("clone", "-r", head_rev, self.repo.path, alt_eden_clone)
            self.assertTrue(
                os.path.isdir(os.path.join(alt_eden_clone, "foo/stuff/build_output")),
                msg="clone should create bind mounts",
            )
            self.assertTrue(
                os.path.isdir(os.path.join(alt_eden_clone, "node_modules")),
                msg="clone should create bind mounts",
            )

    def test_clone_from_eden_repo(self) -> None:
        # Add a config alias for a repo with some bind mounts.
        edenrc = os.path.join(self.home_dir, ".edenrc")
        with open(edenrc, "w") as f:
            f.write(
                dedent(
                    f"""\
            ["repository {repo_name}"]
            path = "{self.repo.get_canonical_root()}"
            type = "{self.repo.get_type()}"

            ["bindmounts {repo_name}"]
            bm1 = "tmp/bm1"
            bm2 = "tmp/bm2"
            """
                )
            )

        # Create an Eden mount from the config alias.
        eden_clone1 = self.make_temporary_directory()
        self.eden.run_cmd("clone", repo_name, eden_clone1)
        self.assertTrue(
            os.path.isdir(os.path.join(eden_clone1, "tmp/bm1")),
            msg="clone should create bind mount",
        )

        # Clone the Eden clone! Note it should inherit its config.
        eden_clone2 = self.make_temporary_directory()
        self.eden.run_cmd(
            "clone", "--rev", self.repo.get_head_hash(), eden_clone1, eden_clone2
        )
        self.assertTrue(
            os.path.isdir(os.path.join(eden_clone2, "tmp/bm1")),
            msg="clone should inherit its config from eden_clone1, "
            "which should include the bind mounts.",
        )

    def test_clone_with_valid_revision_cmd_line_arg_works(self) -> None:
        tmp = self.make_temporary_directory()
        target = os.path.join(tmp, "foo/bar/baz")
        self.eden.run_cmd(
            "clone", "--rev", self.repo.get_head_hash(), repo_name, target
        )
        self.assertTrue(
            os.path.isfile(os.path.join(target, "hello")),
            msg="clone should succeed with --snapshop arg.",
        )

    def test_clone_with_short_revision_cmd_line_arg_works(self) -> None:
        tmp = self.make_temporary_directory()
        target = os.path.join(tmp, "foo/bar/baz")
        short = self.repo.get_head_hash()[:6]
        self.eden.run_cmd("clone", "--rev", short, repo_name, target)
        self.assertTrue(
            os.path.isfile(os.path.join(target, "hello")),
            msg="clone should succeed with short --snapshop arg.",
        )

    def test_clone_to_non_empty_directory_fails(self) -> None:
        tmp = self.make_temporary_directory()
        non_empty_dir = os.path.join(tmp, "foo/bar/baz")
        os.makedirs(non_empty_dir)
        with open(os.path.join(non_empty_dir, "example.txt"), "w") as f:
            f.write("I am not empty.\n")

        with self.assertRaises(subprocess.CalledProcessError) as context:
            self.eden.run_cmd("clone", repo_name, non_empty_dir)
        stderr = context.exception.stderr.decode("utf-8")
        self.assertRegex(
            stderr,
            "destination path .* is not empty",
            msg="clone into non-empty dir should fail",
        )

    def test_clone_with_invalid_revision_cmd_line_arg_fails(self) -> None:
        tmp = self.make_temporary_directory()
        empty_dir = os.path.join(tmp, "foo/bar/baz")
        os.makedirs(empty_dir)

        with self.assertRaises(subprocess.CalledProcessError) as context:
            self.eden.run_cmd("clone", repo_name, empty_dir, "--rev", "X")
        stderr = context.exception.stderr.decode("utf-8")
        self.assertIn(
            "unable to find hash for commit 'X': ",
            stderr,
            msg="passing invalid commit on cmd line should fail",
        )

    def test_clone_to_file_fails(self) -> None:
        tmp = self.make_temporary_directory()
        non_empty_dir = os.path.join(tmp, "foo/bar/baz")
        os.makedirs(non_empty_dir)
        file_in_directory = os.path.join(non_empty_dir, "example.txt")
        with open(file_in_directory, "w") as f:
            f.write("I am not empty.\n")

        with self.assertRaises(subprocess.CalledProcessError) as context:
            self.eden.run_cmd("clone", repo_name, file_in_directory)
        stderr = context.exception.stderr.decode("utf-8")
        self.assertIn(
            f"error: destination path {file_in_directory} is not a directory\n", stderr
        )

    def test_clone_to_non_existent_directory_that_is_under_a_file_fails(self) -> None:
        tmp = self.make_temporary_directory()
        non_existent_dir = os.path.join(tmp, "foo/bar/baz")
        with open(os.path.join(tmp, "foo"), "w") as f:
            f.write("I am not empty.\n")

        with self.assertRaises(subprocess.CalledProcessError) as context:
            self.eden.run_cmd("clone", repo_name, non_existent_dir)
        stderr = context.exception.stderr.decode("utf-8")
        self.assertIn(
            f"error: destination path {non_existent_dir} is not a directory\n", stderr
        )

    def test_attempt_clone_invalid_repo_name(self) -> None:
        tmp = self.make_temporary_directory()
        repo_name = "repo-name-that-is-not-in-the-config"

        with self.assertRaises(edenclient.EdenCommandError) as context:
            self.eden.run_cmd("clone", repo_name, tmp)
        self.assertIn(
            f"error: {repo_name!r} does not look like a valid hg or git "
            "repository or a well-known repository name\n",
            # pyre-fixme[16]: `_E` has no attribute `stderr`.
            context.exception.stderr.decode(),
        )

    def test_clone_should_start_daemon(self) -> None:
        # Shut down Eden.
        self.assertTrue(self.eden.is_healthy())
        self.eden.shutdown()
        self.assertFalse(self.eden.is_healthy())

        # Check `eden list`.
        list_output = self.eden.list_cmd_simple()
        self.assertEqual(
            {self.mount: "NOT_RUNNING"}, list_output, msg="Eden should have one mount."
        )

        extra_daemon_args = self.eden.get_extra_daemon_args()

        # Verify that clone starts the daemon.
        tmp = Path(self.make_temporary_directory())
        clone_output = self.eden.run_cmd(
            "clone",
            "--daemon-binary",
            typing.cast(str, FindExe.EDEN_DAEMON),  # T38947910
            self.repo.path,
            str(tmp),
            "--daemon-args",
            *extra_daemon_args,
        )
        self.assertIn("Starting edenfs", clone_output)
        self.assertTrue(self.eden.is_healthy(), msg="clone should start Eden.")
        mount_points = {self.mount: "RUNNING", str(tmp): "RUNNING"}
        self.assertEqual(
            mount_points,
            self.eden.list_cmd_simple(),
            msg="Eden should have two mounts.",
        )
        self.assertEqual("hola\n", (tmp / "hello").read_text())

    def test_custom_not_mounted_readme(self) -> None:
        """Test that "eden clone" creates a README file in the mount point directory
        telling users what to do if their checkout is not currently mounted.
        """
        # Write a custom readme file in our etc config directory
        custom_readme_text = "If this is broken bug joe@example.com\n"
        with open(os.path.join(self.etc_eden_dir, "NOT_MOUNTED_README.txt"), "w") as f:
            f.write(custom_readme_text)

        # Perform a clone
        new_mount = Path(self.make_temporary_directory())
        readme_path = new_mount / "README_EDEN.txt"
        self.eden.run_cmd("clone", self.repo.path, str(new_mount))
        self.assertEqual("hola\n", (new_mount / "hello").read_text())
        self.assertFalse(os.path.exists(readme_path))

        # Now unmount the checkout and make sure we see the readme
        self.eden.run_cmd("unmount", str(new_mount))
        self.assertFalse((new_mount / "hello").exists())
        self.assertEqual(custom_readme_text, readme_path.read_text())


class CloneFakeEdenFSTestBase(ServiceTestCaseBase, PexpectAssertionMixin):
    def setUp(self) -> None:
        super().setUp()
        self.eden_dir = Path(self.make_temporary_directory())

    def make_dummy_hg_repo(self) -> HgRepository:
        repo = HgRepository(path=self.make_temporary_directory())
        repo.init()
        repo.write_file("hello", "")
        repo.commit("Initial commit.")
        return repo

    def spawn_clone(
        self,
        repo_path: Path,
        mount_path: Path,
        extra_args: Optional[Sequence[str]] = None,
    ) -> "pexpect.spawn[str]":
        args = (
            ["--config-dir", str(self.eden_dir)]
            + self.get_required_eden_cli_args()
            + [
                "clone",
                "--daemon-binary",
                typing.cast(str, FindExe.FAKE_EDENFS),  # T38947910
                str(repo_path),
                str(mount_path),
            ]
        )
        if extra_args:
            args.extend(extra_args)
        return pexpect.spawn(
            FindExe.EDEN_CLI, args, encoding="utf-8", logfile=sys.stderr
        )


@service_test
class CloneFakeEdenFSTest(CloneFakeEdenFSTestBase):
    def test_daemon_command_arguments_should_forward_to_edenfs(self) -> None:
        repo = self.make_dummy_hg_repo()
        mount_path = Path(self.make_temporary_directory())

        extra_daemon_args = ["--allowExtraArgs", "hello world"]
        clone_process = self.spawn_clone(
            repo_path=Path(repo.path),
            mount_path=mount_path,
            extra_args=["--daemon-args"] + extra_daemon_args,
        )
        wait_for_pexpect_process(clone_process)

        argv = get_fake_edenfs_argv(self.eden_dir)
        self.assertEquals(
            argv[-len(extra_daemon_args) :],
            extra_daemon_args,
            f"fake_edenfs should have received arguments verbatim\nargv: {argv}",
        )


@service_test
class CloneFakeEdenFSWithSystemdTest(
    CloneFakeEdenFSTestBase, SystemdServiceTestCaseMarker
):
    def test_clone_starts_systemd_service(self) -> None:
        repo = self.make_dummy_hg_repo()
        mount_path = Path(self.make_temporary_directory())
        clone_process = self.spawn_clone(
            repo_path=Path(repo.path), mount_path=mount_path
        )
        clone_process.expect_exact(
            "edenfs daemon is not currently running.  Starting edenfs..."
        )
        clone_process.expect_exact("Started edenfs")
        self.assert_systemd_service_is_active(eden_dir=self.eden_dir)
