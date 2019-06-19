#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import os

from .lib import testcase


@testcase.eden_repo_test
class InfoTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def test_info_with_bind_mounts(self) -> None:
        edenrc = os.path.join(os.environ["HOME"], ".edenrc")
        with open(edenrc, "w") as f:
            f.write(
                """\
["repository {repo_name}"]
path = "{repo_path}"
type = "{repo_type}"

["bindmounts {repo_name}"]
buck-out = "buck-out"
""".format(
                    repo_name=self.repo_name,
                    repo_path=self.repo.get_canonical_root(),
                    repo_type=self.repo.get_type(),
                )
            )

        basename = "eden_mount"
        tmp = os.path.join(self.tmp_dir, basename)

        self.eden.run_cmd("clone", self.repo_name, tmp)
        info = self.eden.run_cmd("info", tmp)

        client_info = json.loads(info)
        client_dir = os.path.join(self.eden_dir, "clients", basename)
        self.assertEqual(
            {
                "bind-mounts": {"buck-out": "buck-out"},
                "client-dir": client_dir,
                "scm_type": self.repo.get_type(),
                "mount": tmp,
                "snapshot": self.repo.get_head_hash(),
            },
            client_info,
        )

    def test_relative_path(self) -> None:
        """
        Test calling "eden info <relative_path>" and make sure it gives
        the expected results.
        """
        info = self.eden.run_cmd("info", os.path.relpath(self.mount))

        client_info = json.loads(info)
        client_dir = os.path.join(
            self.eden_dir, "clients", os.path.basename(self.mount)
        )
        self.assertEqual(
            {
                "bind-mounts": {},
                "client-dir": client_dir,
                "scm_type": self.repo.get_type(),
                "mount": self.mount,
                "snapshot": self.repo.get_head_hash(),
            },
            client_info,
        )

    def test_through_symlink(self) -> None:
        """
        Test calling "eden info" through a symlink and make sure it gives
        the expected results.  This makes sure "eden info" resolves the path
        correctly before looking it up in the configuration.
        """
        link1 = os.path.join(self.tmp_dir, "link1")
        os.symlink(self.mount, link1)

        info1 = json.loads(self.eden.run_cmd("info", link1))
        self.assertEqual(self.mount, info1["mount"])

        # Create a non-normalized symlink pointing to the parent directory
        # of the mount
        link2 = os.path.join(self.tmp_dir, "mounts_link")
        os.symlink(self.mount + "//..", link2)
        mount_through_link2 = os.path.join(link2, self.repo_name)

        info2 = json.loads(self.eden.run_cmd("info", mount_through_link2))
        self.assertEqual(self.mount, info2["mount"])
