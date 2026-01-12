#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import json
import os
import subprocess
import sys
from pathlib import Path

from eden.fs.cli.util import mkscratch_bin
from eden.fs.service.eden.thrift_types import (
    ListRedirectionsRequest,
    MountId,
    Redirection,
    RedirectionState,
    RedirectionType,
)

from .lib import testcase

if sys.platform == "win32":
    from eden.fs.cli.util import remove_unc_prefix


def scratch_path(repo: str, subdir: str) -> str:
    return (
        subprocess.check_output(
            [
                os.fsdecode(mkscratch_bin()),
                "path",
                repo,
                "--subdir",
                subdir,
            ]
        )
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
        profile_path = scratch_path(
            self.mount,
            os.path.join("edenfs", "redirections", "via-profile"),
        )
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

    async def test_disallow_bind_mount_outside_repo(self) -> None:
        dir_to_mount = os.path.join(self.tmp_dir, "to-mount")
        os.mkdir(dir_to_mount)
        mount_point = os.path.join(self.tmp_dir, "mount-point")
        os.mkdir(mount_point)

        mount_point_bytes = mount_point.encode("utf-8")
        dir_to_mount_bytes = dir_to_mount.encode("utf-8")
        async with self.get_thrift_client() as client:
            with self.assertRaises(Exception) as ctx:
                await client.addBindMount(
                    mount_point_bytes, mount_point_bytes, dir_to_mount_bytes
                )
            self.assertIn(
                "is not known to this eden instance",
                str(ctx.exception),
                msg="Can't specify an arbitrary mount point",
            )

            with self.assertRaises(Exception) as ctx:
                await client.addBindMount(
                    self.mount.encode("utf-8"), mount_point_bytes, dir_to_mount_bytes
                )
            self.assertIn(
                f"attempt to construct a RelativePath from an absolute path string: {mount_point}",
                str(ctx.exception),
                msg="Can't mount outside the repo via absolute path",
            )

            rel_mount = os.path.relpath(mount_point, self.mount).encode("utf-8")
            with self.assertRaises(Exception) as ctx:
                await client.addBindMount(
                    self.mount.encode("utf-8"), rel_mount, dir_to_mount_bytes
                )
                self.assertIn(
                    "PathComponent must not be . or ..",
                    str(ctx.exception),
                    msg="Can't mount outside the repo via relative path",
                )

    def test_list_with_cli(self) -> None:
        # Redirection via profile
        profile_repo_path = "via-profile"
        profile_target_path = scratch_path(
            self.mount, os.path.join("edenfs", "redirections", profile_repo_path)
        )
        profile_source = ".eden-redirections"
        # New redirection to be added later
        repo_path = os.path.join("a", "new-one")
        source = ".eden/client/config.toml:redirections"
        target_path = scratch_path(
            self.mount,
            os.path.join("edenfs", "redirections", "a", "new-one"),
        )

        output = self.eden.run_cmd("redirect", "list", "--json", "--mount", self.mount)
        self.assertEqual(
            json.loads(output),
            [
                {
                    "repo_path": profile_repo_path,
                    "type": "bind",
                    "target": profile_target_path,
                    "source": profile_source,
                    "state": "ok",
                }
            ],
            msg="We can interpret the saved bind mount configuration",
        )

        output = self.eden.run_cmd(
            "redirect", "add", "--mount", self.mount, repo_path, "bind"
        )
        self.assertEqual(output, "", msg="we believe we set up a new bind mount")

        list_output = self.eden.run_cmd(
            "redirect", "list", "--json", "--mount", self.mount
        )
        self.assertEqual(
            json.loads(list_output),
            [
                {
                    "repo_path": repo_path,
                    "type": "bind",
                    "target": target_path,
                    "source": source,
                    "state": "ok",
                },
                {
                    "repo_path": profile_repo_path,
                    "type": "bind",
                    "target": profile_target_path,
                    "source": profile_source,
                    "state": "ok",
                },
            ],
            msg="saved config agrees with last output",
        )

        if sys.platform != "win32":
            mount_stat = os.stat(self.mount)
            bind_mount_stat = os.stat(os.path.join(self.mount, repo_path))
            self.assertNotEqual(
                mount_stat.st_dev,
                bind_mount_stat.st_dev,
                msg="new-one dir was created and mounted with a different device",
            )
        else:
            # On Windows we use symlink to implement bind mount type
            # redirection. As a result `st_dev` check will fail so we check if
            # the symlink is pointing actually outside of the repository.
            redirection = os.path.join(self.mount, repo_path)
            link_target = os.readlink(redirection)

            # This checks if the common parent of redirection target and the
            # repository is still in the mount (i.e. if redirection target is a
            # subdirectory of the mount).
            self.assertNotEqual(
                os.path.commonprefix([self.mount, link_target]),
                self.mount,
                msg="Redirection target is still inside the repository.",
            )

        output = self.eden.run_cmd(
            "redirect",
            "del",
            "--mount",
            self.mount,
            repo_path,
        )
        self.assertEqual(output, "", msg="we believe we removed the bind mount")
        output = self.eden.run_cmd(
            "redirect", "add", "--mount", self.mount, repo_path, "symlink"
        )
        self.assertEqual(output, "", msg="we believe we switched to a symlink")

        list_output = self.eden.run_cmd(
            "redirect", "list", "--json", "--mount", self.mount
        )
        self.assertEqual(
            json.loads(list_output),
            [
                {
                    "repo_path": repo_path,
                    "type": "symlink",
                    "target": target_path,
                    "source": source,
                    "state": "ok",
                },
                {
                    "repo_path": profile_repo_path,
                    "type": "bind",
                    "target": profile_target_path,
                    "source": profile_source,
                    "state": "ok",
                },
            ],
            msg="saved config agrees with last output",
        )

        expected_target = Path(
            os.readlink(os.path.join(self.mount, "a", "new-one"))
        ).resolve()
        if sys.platform == "win32":
            expected_target = remove_unc_prefix(expected_target)

        self.assertEqual(
            expected_target,
            Path(target_path).resolve(),
            msg="symlink points to scratch space",
        )

        output = self.eden.run_cmd("redirect", "del", "--mount", self.mount, repo_path)
        self.assertEqual(output, "", msg="we believe we removed the symlink")

        list_output = self.eden.run_cmd(
            "redirect", "list", "--json", "--mount", self.mount
        )
        self.assertEqual(
            json.loads(list_output),
            [
                {
                    "repo_path": profile_repo_path,
                    "type": "bind",
                    "target": profile_target_path,
                    "source": profile_source,
                    "state": "ok",
                }
            ],
            msg="saved config agrees with last output",
        )

        self.assertFalse(
            os.path.exists(os.path.join(self.mount, "a", "new-one")),
            msg="symlink is gone",
        )

    async def test_list_with_thrift(self) -> None:
        # Redirection via profile
        profile_repo_path = "via-profile"
        profile_target_path = scratch_path(
            self.mount, os.path.join("edenfs", "redirections", profile_repo_path)
        )
        profile_source = ".eden-redirections"
        # New redirection to be added later
        repo_path = os.path.join("a", "new-one")
        source = ".eden/client/config.toml:redirections"
        target_path = scratch_path(
            self.mount,
            os.path.join("edenfs", "redirections", "a", "new-one"),
        )
        # Thrift request
        thrift_mount = MountId(mountPoint=self.mount.encode("utf-8"))
        thrift_req = ListRedirectionsRequest(mount=thrift_mount)

        # List via thrift
        async with self.get_thrift_client() as client:
            thrift_output = await client.listRedirections(thrift_req)
            self.assertListEqual(
                list(thrift_output.redirections),
                [
                    Redirection(
                        repoPath=profile_repo_path.encode("utf-8"),
                        redirType=RedirectionType.BIND,
                        source=profile_source.encode("utf-8"),
                        state=RedirectionState.MATCHES_CONFIGURATION,
                        target=profile_target_path.encode("utf-8"),
                    )
                ],
            )

        # Add a new bind redirection
        output = self.eden.run_cmd(
            "redirect", "add", "--mount", self.mount, repo_path, "bind"
        )
        self.assertEqual(output, "", msg="we believe we set up a new bind mount")

        # List via thrift
        async with self.get_thrift_client() as client:
            thrift_output = await client.listRedirections(thrift_req)
            self.assertListEqual(
                list(thrift_output.redirections),
                [
                    Redirection(
                        repoPath=repo_path.encode("utf-8"),
                        redirType=RedirectionType.BIND,
                        source=source.encode("utf-8"),
                        state=RedirectionState.MATCHES_CONFIGURATION,
                        target=target_path.encode("utf-8"),
                    ),
                    Redirection(
                        repoPath=profile_repo_path.encode("utf-8"),
                        redirType=RedirectionType.BIND,
                        source=profile_source.encode("utf-8"),
                        state=RedirectionState.MATCHES_CONFIGURATION,
                        target=profile_target_path.encode("utf-8"),
                    ),
                ],
                msg="saved config agrees with last output via thrift endpoint",
            )

        # Remove the bind redirection
        output = self.eden.run_cmd(
            "redirect",
            "del",
            "--mount",
            self.mount,
            repo_path,
        )
        self.assertEqual(output, "", msg="we believe we removed the bind mount")
        # Add a new symlink redirection
        output = self.eden.run_cmd(
            "redirect", "add", "--mount", self.mount, repo_path, "symlink"
        )
        self.assertEqual(output, "", msg="we believe we switched to a symlink")

        # List via thrift
        async with self.get_thrift_client() as client:
            thrift_output = await client.listRedirections(thrift_req)
            self.assertListEqual(
                list(thrift_output.redirections),
                [
                    Redirection(
                        repoPath=repo_path.encode("utf-8"),
                        redirType=RedirectionType.SYMLINK,
                        source=source.encode("utf-8"),
                        state=RedirectionState.MATCHES_CONFIGURATION,
                        target=target_path.encode("utf-8"),
                    ),
                    Redirection(
                        repoPath=profile_repo_path.encode("utf-8"),
                        redirType=RedirectionType.BIND,
                        source=profile_source.encode("utf-8"),
                        state=RedirectionState.MATCHES_CONFIGURATION,
                        target=profile_target_path.encode("utf-8"),
                    ),
                ],
                msg="saved config agrees with last output via thrift endpoint",
            )

        # Remove the symlink redirection
        output = self.eden.run_cmd("redirect", "del", "--mount", self.mount, repo_path)
        self.assertEqual(output, "", msg="we believe we removed the symlink")

        # List via thrift
        async with self.get_thrift_client() as client:
            thrift_output = await client.listRedirections(thrift_req)
            self.assertListEqual(
                list(thrift_output.redirections),
                [
                    Redirection(
                        repoPath=profile_repo_path.encode("utf-8"),
                        redirType=RedirectionType.BIND,
                        source=profile_source.encode("utf-8"),
                        state=RedirectionState.MATCHES_CONFIGURATION,
                        target=profile_target_path.encode("utf-8"),
                    ),
                ],
                msg="saved config agrees with last output via thrift endpoint",
            )

    def test_fixup_mounts_things(self) -> None:
        profile_path = scratch_path(
            self.mount,
            os.path.join("edenfs", "redirections", "via-profile"),
        )
        repo_path = os.path.join("a", "new-one")

        # add a symlink redirection manually
        output = self.eden.run_cmd(
            "redirect", "add", "--mount", self.mount, repo_path, "symlink"
        )
        self.assertEqual(output, "", msg="we believe we set up a new symlink mount")
        target_path = scratch_path(
            self.mount,
            os.path.join("edenfs", "redirections", repo_path),
        )

        # unmount all redirections to ensure `fixup` must fix them all
        output = self.eden.run_cmd("redirect", "unmount", "--mount", self.mount)
        self.assertEqual(output, "", msg="we believe we unmounted all redirections")
        output = self.eden.run_cmd("redirect", "list", "--json", "--mount", self.mount)
        self.assertEqual(
            json.loads(output),
            [
                {
                    "repo_path": repo_path,
                    "type": "symlink",
                    "target": target_path,
                    "source": ".eden/client/config.toml:redirections",
                    "state": "symlink-missing",
                },
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    "state": (
                        "not-mounted" if sys.platform != "win32" else "symlink-missing"
                    ),
                },
            ],
        )

        # ensure all redirections from .eden-redirections source are fixed correctly
        self.eden.run_cmd(
            "redirect", "fixup", "--mount", self.mount, "--only-repo-source"
        )
        output = self.eden.run_cmd("redirect", "list", "--json", "--mount", self.mount)
        self.assertEqual(
            json.loads(output),
            [
                {
                    "repo_path": repo_path,
                    "type": "symlink",
                    "target": target_path,
                    "source": ".eden/client/config.toml:redirections",
                    "state": "symlink-missing",
                },
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    "state": "ok",
                },
            ],
        )

        # unmount all redirections (again) to ensure `fixup` must fix them all
        output = self.eden.run_cmd("redirect", "unmount", "--mount", self.mount)
        self.assertEqual(output, "", msg="we believe we unmounted all redirections")
        output = self.eden.run_cmd("redirect", "list", "--json", "--mount", self.mount)
        self.assertEqual(
            json.loads(output),
            [
                {
                    "repo_path": repo_path,
                    "type": "symlink",
                    "target": target_path,
                    "source": ".eden/client/config.toml:redirections",
                    "state": "symlink-missing",
                },
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    "state": (
                        "not-mounted" if sys.platform != "win32" else "symlink-missing"
                    ),
                },
            ],
        )

        # ensure all redirections (including non .eden-redirections sourced) are fixed
        self.eden.run_cmd("redirect", "fixup", "--mount", self.mount)
        output = self.eden.run_cmd("redirect", "list", "--json", "--mount", self.mount)
        self.assertEqual(
            json.loads(output),
            [
                {
                    "repo_path": repo_path,
                    "type": "symlink",
                    "target": target_path,
                    "source": ".eden/client/config.toml:redirections",
                    "state": "ok",
                },
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    "state": "ok",
                },
            ],
        )

    def test_unmount_unmounts_things(self) -> None:
        profile_path = scratch_path(
            self.mount,
            os.path.join("edenfs", "redirections", "via-profile"),
        )

        # setup new symlink redirection
        repo_path = os.path.join("a", "new-one")
        output = self.eden.run_cmd(
            "redirect", "add", "--mount", self.mount, repo_path, "symlink"
        )
        self.assertEqual(
            output, "", msg="we believe we set up a new symlink redirection"
        )
        target_path = scratch_path(
            self.mount,
            os.path.join("edenfs", "redirections", "a", "new-one"),
        )

        # assert both redirections exist and are mounted
        output = self.eden.run_cmd("redirect", "list", "--json", "--mount", self.mount)
        self.assertEqual(
            json.loads(output),
            [
                {
                    "repo_path": repo_path,
                    "type": "symlink",
                    "target": target_path,
                    "source": ".eden/client/config.toml:redirections",
                    "state": "ok",
                },
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    "state": "ok",
                },
            ],
        )

        self.eden.run_cmd("redirect", "unmount", "--mount", self.mount)
        output = self.eden.run_cmd("redirect", "list", "--json", "--mount", self.mount)
        self.assertEqual(
            json.loads(output),
            [
                {
                    "repo_path": repo_path,
                    "type": "symlink",
                    "target": target_path,
                    "source": ".eden/client/config.toml:redirections",
                    "state": "symlink-missing",
                },
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    "state": (
                        "not-mounted" if sys.platform != "win32" else "symlink-missing"
                    ),
                },
            ],
        )

    def test_redirect_no_config_dir(self) -> None:
        profile_path = scratch_path(
            self.mount,
            os.path.join("edenfs", "redirections", "via-profile"),
        )

        output = self.eden.run_cmd(
            "redirect",
            "list",
            "--json",
            "--mount",
            self.mount,
            config_dir=False,
        )
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

    def test_add_absolute_target(self) -> None:
        profile_path = scratch_path(
            self.mount,
            os.path.join("edenfs", "redirections", "via-profile"),
        )

        # providing an absolute path should behave in the exact same way as providing a rel path
        repo_path = os.path.join("a", "new-one")
        abs_path = os.path.join(self.mount, repo_path)
        output = self.eden.run_cmd(
            "redirect", "add", "--mount", self.mount, abs_path, "bind"
        )
        self.assertEqual(output, "", msg="we believe we set up a new bind mount")

        target_path = scratch_path(
            self.mount,
            os.path.join("edenfs", "redirections", "a", "new-one"),
        )
        output = self.eden.run_cmd(
            "redirect",
            "list",
            "--json",
            "--mount",
            self.mount,
        )
        self.assertEqual(
            json.loads(output),
            [
                {
                    "repo_path": repo_path,
                    "type": "bind",
                    "target": target_path,
                    "source": ".eden/client/config.toml:redirections",
                    "state": "ok",
                },
                {
                    "repo_path": "via-profile",
                    "type": "bind",
                    "target": profile_path,
                    "source": ".eden-redirections",
                    "state": "ok",
                },
            ],
        )
