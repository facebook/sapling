#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import json
import os
import pathlib
import stat
import subprocess
import sys
from textwrap import dedent
from typing import Optional, Sequence, Set

import pexpect
from eden.cli import util
from eden.integration.lib.hgrepo import HgRepository

from .lib import edenclient, testcase
from .lib.fake_edenfs import read_fake_edenfs_argv_file
from .lib.find_executables import FindExe
from .lib.pexpect import PexpectAssertionMixin, wait_for_pexpect_process
from .lib.service_test_case import ServiceTestCaseBase, service_test


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

    def test_post_clone_hook(self) -> None:
        edenrc = os.path.join(self.home_dir, ".edenrc")
        hooks_dir = os.path.join(self.tmp_dir, "the_hooks")
        os.mkdir(hooks_dir)

        with open(edenrc, "w") as f:
            f.write(
                """\
["repository {repo_name}"]
path = "{repo_path}"
type = "{repo_type}"
hooks = "{hooks_dir}"
""".format(
                    repo_name=repo_name,
                    repo_path=self.repo.get_canonical_root(),
                    repo_type=self.repo.get_type(),
                    hooks_dir=hooks_dir,
                )
            )

        # Create a post-clone hook that has a visible side-effect every time it
        # is run so we can verify that it is only run once.
        hg_post_clone_hook = os.path.join(hooks_dir, "post-clone")
        scratch_file = os.path.join(self.tmp_dir, "scratch_file")
        with open(scratch_file, "w") as f:
            f.write("ok")
        with open(hg_post_clone_hook, "w") as f:
            f.write(
                """\
#!/bin/bash
CONTENTS=`cat "{scratch_file}"`
echo -n "$1" >> "{scratch_file}"
""".format(
                    scratch_file=scratch_file
                )
            )
        os.chmod(hg_post_clone_hook, stat.S_IRWXU)

        # Verify that the hook gets run as part of `eden clone`.
        self.assertEqual("ok", util.read_all(bytes(scratch_file, "utf-8")))
        tmp = self.make_temporary_directory()
        self.eden.clone(repo_name, tmp)
        new_contents = "ok" + self.repo.get_type()
        self.assertEqual(new_contents, util.read_all(bytes(scratch_file, "utf-8")))

        # Restart Eden and verify that post-clone is NOT run again.
        self.eden.shutdown()
        self.eden.start()
        self.assertEqual(new_contents, util.read_all(bytes(scratch_file, "utf-8")))

    def test_attempt_clone_invalid_repo_name(self) -> None:
        tmp = self.make_temporary_directory()
        repo_name = "repo-name-that-is-not-in-the-config"

        with self.assertRaises(edenclient.EdenCommandError) as context:
            self.eden.run_cmd("clone", repo_name, tmp)
        self.assertIn(
            f"error: {repo_name!r} does not look like a valid hg or git "
            "repository or a well-known repository name\n",
            context.exception.stderr.decode(),
        )

    def test_clone_should_start_daemon(self) -> None:
        # Shut down Eden.
        self.assertTrue(self.eden.is_healthy())
        self.eden.shutdown()
        self.assertFalse(self.eden.is_healthy())

        # Check `eden list`.
        list_output = self.eden.list_cmd()
        self.assertEqual(
            {self.mount: self.eden.CLIENT_INACTIVE},
            list_output,
            msg="Eden should have one mount.",
        )

        extra_daemon_args = self.eden.get_extra_daemon_args()

        # Verify that clone starts the daemon.
        tmp = self.make_temporary_directory()
        clone_output = self.eden.run_cmd(
            "clone",
            "--daemon-binary",
            FindExe.EDEN_DAEMON,
            self.repo.path,
            tmp,
            "--daemon-args",
            *extra_daemon_args,
        )
        self.assertIn("Starting edenfs", clone_output)
        self.assertTrue(self.eden.is_healthy(), msg="clone should start Eden.")
        mount_points = {
            self.mount: self.eden.CLIENT_ACTIVE,
            tmp: self.eden.CLIENT_ACTIVE,
        }
        self.assertEqual(
            mount_points, self.eden.list_cmd(), msg="Eden should have two mounts."
        )
        self.assertEqual(
            "hola\n", util.read_all(bytes(os.path.join(tmp, "hello"), "utf-8"))
        )

    def test_custom_not_mounted_readme(self) -> None:
        """Test that "eden clone" creates a README file in the mount point directory
        telling users what to do if their checkout is not currently mounted.
        """
        # Write a custom readme file in our etc config directory
        custom_readme_text = "If this is broken bug joe@example.com\n"
        with open(os.path.join(self.etc_eden_dir, "NOT_MOUNTED_README.txt"), "w") as f:
            f.write(custom_readme_text)

        # Perform a clone
        new_mount = self.make_temporary_directory()
        readme_path = os.path.join(new_mount, "README_EDEN.txt")
        self.eden.run_cmd("clone", self.repo.path, new_mount)
        self.assertEqual(
            "hola\n", util.read_all(bytes(os.path.join(new_mount, "hello"), "utf-8"))
        )
        self.assertFalse(os.path.exists(readme_path))

        # Now unmount the checkout and make sure we see the readme
        self.eden.run_cmd("unmount", new_mount)
        self.assertFalse(os.path.exists(os.path.join(new_mount, "hello")))
        self.assertEqual(custom_readme_text, util.read_all(bytes(readme_path, "utf-8")))


# TODO(T33122320): Integrate systemd and 'eden clone'.
@service_test(skip_systemd=True)
class CloneFakeEdenFSTest(ServiceTestCaseBase, PexpectAssertionMixin):
    def setUp(self) -> None:
        super().setUp()
        self.eden_dir = pathlib.Path(self.make_temporary_directory())

    def test_daemon_command_arguments_should_forward_to_edenfs(self) -> None:
        argv_file = self.eden_dir / "argv"
        assert not argv_file.exists()
        repo = self.make_dummy_hg_repo()
        mount_path = pathlib.Path(self.make_temporary_directory())

        extra_daemon_args = ["--commandArgumentsLogFile", str(argv_file), "hello world"]
        clone_process = self.spawn_clone(
            repo_path=pathlib.Path(repo.path),
            mount_path=mount_path,
            extra_args=["--daemon-args"] + extra_daemon_args,
        )
        wait_for_pexpect_process(clone_process)

        argv = read_fake_edenfs_argv_file(argv_file)
        self.assertEquals(
            argv[-len(extra_daemon_args) :],
            extra_daemon_args,
            f"fake_edenfs should have received arguments verbatim\nargv: {argv}",
        )

    def make_dummy_hg_repo(self) -> HgRepository:
        repo = HgRepository(path=self.make_temporary_directory())
        repo.init()
        repo.write_file("hello", "")
        repo.commit("Initial commit.")
        return repo

    def spawn_clone(
        self,
        repo_path: pathlib.Path,
        mount_path: pathlib.Path,
        extra_args: Sequence[str] = (),
    ) -> "pexpect.spawn[str]":
        args = (
            ["--config-dir", str(self.eden_dir)]
            + self.get_required_eden_cli_args()
            + [
                "clone",
                "--daemon-binary",
                FindExe.FAKE_EDENFS,
                str(repo_path),
                str(mount_path),
            ]
        )
        args.extend(extra_args)
        return pexpect.spawn(
            FindExe.EDEN_CLI, args, encoding="utf-8", logfile=sys.stderr
        )
