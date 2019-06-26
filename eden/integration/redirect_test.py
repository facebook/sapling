#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import os
import subprocess

from eden.cli.util import mkscratch_bin

from .lib import testcase


def scratch_path(repo: str, subdir: str) -> str:
    return (
        subprocess.check_output([mkscratch_bin(), "path", repo, "--subdir", subdir])
        .decode("utf-8")
        .strip()
    )


@testcase.eden_repo_test
class RedirectTest(testcase.EdenRepoTest):
    """Exercise the `eden redirect` command"""

    maxDiff = None

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.symlink("slink", "hello")
        self.repo.write_file(
            ".eden-redirections",
            """\
[redirections]
via-profile = "bind"
""",
        )
        self.repo.commit("Initial commit.")

    def test_list_no_legacy_bind_mounts(self) -> None:
        output = self.eden.run_cmd("redirect", "list", "--json", "--mount", self.mount)
        profile_path = scratch_path(self.mount, "edenfs/redirections/via-profile")
        self.assertEqual(
            json.loads(output),
            [
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    # until we hook up post-update or post-mount hooks,
                    # this won't auto-mount
                    "state": "not-mounted",
                }
            ],
        )

    def clone_with_legacy_bind_mounts(self) -> str:
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
        return tmp

    def test_disallow_bind_mount_outside_repo(self) -> None:
        dir_to_mount = os.path.join(self.tmp_dir, "to-mount")
        os.mkdir(dir_to_mount)
        mount_point = os.path.join(self.tmp_dir, "mount-point")
        os.mkdir(mount_point)

        mount_point_bytes = mount_point.encode("utf-8")
        dir_to_mount_bytes = dir_to_mount.encode("utf-8")
        with self.get_thrift_client() as client:
            with self.assertRaises(Exception) as ctx:
                client.addBindMount(
                    mount_point_bytes, mount_point_bytes, dir_to_mount_bytes
                )
            self.assertIn(
                "is not known to this eden instance",
                str(ctx.exception),
                msg="Can't specify an arbitrary mount point",
            )

            with self.assertRaises(Exception) as ctx:
                client.addBindMount(
                    self.mount.encode("utf-8"), mount_point_bytes, dir_to_mount_bytes
                )
            self.assertIn(
                f"attempt to construct a RelativePath from an absolute path string: {mount_point}",
                str(ctx.exception),
                msg="Can't mount outside the repo via absolute path",
            )

            rel_mount = os.path.relpath(mount_point, self.mount).encode("utf-8")
            with self.assertRaises(Exception) as ctx:
                client.addBindMount(
                    self.mount.encode("utf-8"), rel_mount, dir_to_mount_bytes
                )
                self.assertIn(
                    "PathComponent must not be . or ..",
                    str(ctx.exception),
                    msg="Can't mount outside the repo via relative path",
                )

    def test_list_with_legacy_bind_mount(self) -> None:
        tmp = self.clone_with_legacy_bind_mounts()
        client_dir = os.readlink(os.path.join(tmp, ".eden/client"))

        profile_path = scratch_path(tmp, "edenfs/redirections/via-profile")
        output = self.eden.run_cmd("redirect", "list", "--json", "--mount", tmp)
        self.assertEqual(
            json.loads(output),
            [
                {
                    "repo_path": "buck-out",
                    "type": "legacy",
                    "target": f"{client_dir}/bind-mounts/buck-out",
                    "source": ".eden/client/config.toml:bind-mounts",
                    "state": "ok",
                },
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    # until we hook up post-update or post-mount hooks,
                    # this won't auto-mount
                    "state": "not-mounted",
                },
            ],
            msg="We can interpret the saved bind mount configuration",
        )

        output = self.eden.run_cmd(
            "redirect", "add", "--mount", tmp, "a/new-one", "bind"
        )
        self.assertEqual(output, "", msg="we believe we set up a new bind mount")

        list_output = self.eden.run_cmd("redirect", "list", "--json", "--mount", tmp)
        target_path = scratch_path(tmp, "edenfs/redirections/a/new-one")
        self.assertEqual(
            json.loads(list_output),
            [
                {
                    "repo_path": "a/new-one",
                    "type": "bind",
                    "target": target_path,
                    "source": ".eden/client/config.toml:redirections",
                    "state": "ok",
                },
                {
                    "repo_path": "buck-out",
                    "type": "legacy",
                    "target": f"{client_dir}/bind-mounts/buck-out",
                    "source": ".eden/client/config.toml:bind-mounts",
                    "state": "ok",
                },
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    # until we hook up post-update or post-mount hooks,
                    # this won't auto-mount
                    "state": "not-mounted",
                },
            ],
            msg="saved config agrees with last output",
        )

        mount_stat = os.stat(tmp)
        bind_mount_stat = os.stat(os.path.join(tmp, "a/new-one"))
        self.assertNotEqual(
            mount_stat.st_dev,
            bind_mount_stat.st_dev,
            msg="new-one dir was created and mounted with a different device",
        )

        output = self.eden.run_cmd(
            "redirect", "add", "--mount", tmp, "a/new-one", "symlink"
        )
        self.assertEqual(output, "", msg="we believe we switched to a symlink")

        list_output = self.eden.run_cmd("redirect", "list", "--json", "--mount", tmp)
        self.assertEqual(
            json.loads(list_output),
            [
                {
                    "repo_path": "a/new-one",
                    "type": "symlink",
                    "target": target_path,
                    "source": ".eden/client/config.toml:redirections",
                    "state": "ok",
                },
                {
                    "repo_path": "buck-out",
                    "type": "legacy",
                    "target": f"{client_dir}/bind-mounts/buck-out",
                    "source": ".eden/client/config.toml:bind-mounts",
                    "state": "ok",
                },
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    # until we hook up post-update or post-mount hooks,
                    # this won't auto-mount
                    "state": "not-mounted",
                },
            ],
            msg="saved config agrees with last output",
        )

        self.assertEqual(
            os.readlink(os.path.join(tmp, "a", "new-one")),
            target_path,
            msg="symlink points to scratch space",
        )

        output = self.eden.run_cmd("redirect", "del", "--mount", tmp, "a/new-one")
        self.assertEqual(output, "", msg="we believe we removed the symlink")

        list_output = self.eden.run_cmd("redirect", "list", "--json", "--mount", tmp)
        self.assertEqual(
            json.loads(list_output),
            [
                {
                    "repo_path": "buck-out",
                    "type": "legacy",
                    "target": f"{client_dir}/bind-mounts/buck-out",
                    "source": ".eden/client/config.toml:bind-mounts",
                    "state": "ok",
                },
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    # until we hook up post-update or post-mount hooks,
                    # this won't auto-mount
                    "state": "not-mounted",
                },
            ],
            msg="saved config agrees with last output",
        )

        self.assertFalse(
            os.path.exists(os.path.join(tmp, "a", "new-one")), msg="symlink is gone"
        )

    def test_fixup_mounts_things(self) -> None:
        profile_path = scratch_path(self.mount, "edenfs/redirections/via-profile")

        output = self.eden.run_cmd("redirect", "list", "--json", "--mount", self.mount)
        self.assertEqual(
            json.loads(output),
            [
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    # until we hook up post-update or post-mount hooks,
                    # this won't auto-mount
                    "state": "not-mounted",
                }
            ],
        )
        self.eden.run_cmd("redirect", "fixup", "--mount", self.mount)
        output = self.eden.run_cmd("redirect", "list", "--json", "--mount", self.mount)
        self.assertEqual(
            json.loads(output),
            [
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    "state": "ok",
                }
            ],
        )
