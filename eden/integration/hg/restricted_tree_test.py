#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import abc
import configparser
import errno
import os
import stat
from typing import TYPE_CHECKING

from eden.fs.service.eden.thrift_types import (
    DirListAttributeDataOrError,
    GetScmStatusParams,
    GlobParams,
    ReaddirParams,
    SyncBehavior,
)
from eden.integration.hg.lib.hg_extension_test_base import EdenHgTestCase, hg_test
from eden.integration.lib import hgrepo
from eden.integration.lib.eagerrepo import EagerRepo


class _RestrictedTreeTestBase(EdenHgTestCase, metaclass=abc.ABCMeta):
    """Base class for restricted tree tests using standard eagerepo setup."""

    initial_commit: str = ""
    swapped_commit: str = ""
    # Subclasses flip this to False to disable client-side enforcement.
    enable_restricted_tree_mode: bool = True
    # Subclasses flip this to True to enable server-side PermissionDenied.
    enable_server_acl_enforcement: bool = False

    def apply_hg_config_variant(self, hgrc: configparser.ConfigParser) -> None:
        super().apply_hg_config_variant(hgrc)
        # scmstore reads these from the backing repo's .hg/hgrc. EdenHgTestCase
        # writes this hgrc before eden.clone(), so the config is in place by
        # the time the backing store reads it.
        if self.enable_restricted_tree_mode:
            if not hgrc.has_section("experimental"):
                hgrc.add_section("experimental")
            hgrc["experimental"]["restricted-tree-mode"] = "enforced"
            # Companion: ACL data rides on tree child metadata, so without
            # tree-metadata-mode=always the acl_checker has nothing to enforce on.
            if not hgrc.has_section("scmstore"):
                hgrc.add_section("scmstore")
            hgrc["scmstore"]["tree-metadata-mode"] = "always"
        if not hgrc.has_section("slacl"):
            hgrc.add_section("slacl")
        # These settings apply only to the backing repo hgrc used by Sapling
        # commands in test setup, not the checked-out Eden client.
        hgrc["slacl"]["on-permission-denied"] = "warn"
        hgrc["slacl"]["server-acl-enforcement"] = (
            "true" if self.enable_server_acl_enforcement else "false"
        )

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        # Populate the eager backing repo directly so pulled trees carry the
        # ACL child metadata these tests are exercising. Committing in the
        # backing repo would go through a local-only path that bypasses it.
        eagerepo_path = repo.eagerepo
        assert eagerepo_path is not None, (
            "backing HgRepository.init() must populate self.eagerepo before "
            "populate_backing_repo runs"
        )
        eager = EagerRepo(
            eagerepo_path,
            hg_environment=repo.hg_environment,
            system_hgrc=None,
        )
        eager.init()

        # Initial commit: restricted/ has .slacl, regular/ does not.
        eager.write_file("regular/file.txt", "regular content")
        eager.write_file("restricted/.slacl", "acl config")
        eager.write_file("restricted/secret.txt", "secret content")
        eager.write_file("parent/normal_file.txt", "normal")
        eager.write_file("parent/nested_restricted/.slacl", "acl config")
        eager.write_file("parent/nested_restricted/deep.txt", "deep secret")
        eager.write_file("hello.txt", "hello")
        self.initial_commit = eager.commit("Initial commit.")

        # Swapped commit: restricted/ loses .slacl, regular/ gains it.
        eager.remove_file("restricted/.slacl")
        eager.write_file("regular/.slacl", "regular acl config")
        self.swapped_commit = eager.commit("Swap ACL state.")

        # Pull both commits into the backing repo via SLAPI. ``fetch_edenapi``
        # populates ``indexedlog_cache`` with entries that include
        # ``acl_children_indices`` derived from ``has_acl`` on children.
        # Pulling by hash avoids needing a server bookmark (``master`` is on
        # the disallowed list anyway).
        repo.hg("pull", "-r", self.initial_commit)
        repo.hg("pull", "-r", self.swapped_commit)
        repo.hg("update", self.initial_commit)


if TYPE_CHECKING:
    # At type-check time, pretend the mixin inherits from the test base so
    # Pyre can resolve self.assertRaises, self.mount, etc. without a flood
    # of type-ignore annotations. At runtime, the mixin stays a plain object
    # so the concrete subclasses' MRO (mixin + real base) is unchanged.
    _MethodsBase = _RestrictedTreeTestBase
else:
    _MethodsBase = object


class _RestrictedTreeTestMethods(_MethodsBase, metaclass=abc.ABCMeta):
    """Mixin with test methods parameterized by expect_restricted."""

    # Subclasses set this to False for config-off variants
    expect_restricted: bool = True

    def _assert_dir_blocked(self, path: str) -> None:
        """Assert directory is blocked (EACCES) or accessible, based on expect_restricted."""
        if self.expect_restricted:
            with self.assertRaises(OSError) as ctx:
                os.listdir(path)
            self.assertEqual(ctx.exception.errno, errno.EACCES)
        else:
            os.listdir(path)

    def _assert_file_blocked(self, path: str) -> None:
        """Assert file access is blocked or accessible."""
        if self.expect_restricted:
            with self.assertRaises(OSError) as ctx:
                with open(path, "r") as f:
                    f.read()
            self.assertEqual(ctx.exception.errno, errno.EACCES)
        else:
            with open(path, "r") as f:
                f.read()  # should not raise

    def test_regular_dir_is_accessible(self) -> None:
        """Regular directories should always be fully accessible."""
        entries = sorted(os.listdir(os.path.join(self.mount, "regular")))
        self.assertEqual(["file.txt"], entries)

        with open(os.path.join(self.mount, "regular", "file.txt"), "r") as f:
            self.assertEqual("regular content", f.read())

    def test_root_listing_includes_restricted_dir(self) -> None:
        """The root listing should include restricted directories."""
        entries = os.listdir(self.mount)
        self.assertIn("regular", entries)
        self.assertIn("restricted", entries)

    def test_regular_dir_stat_has_normal_permissions(self) -> None:
        st = os.lstat(os.path.join(self.mount, "regular"))
        self.assertTrue(stat.S_ISDIR(st.st_mode))
        self.assertNotEqual(st.st_mode & 0o7777, 0)

    def test_restricted_dir_stat_permissions(self) -> None:
        restricted_path = os.path.join(self.mount, "restricted")
        st = os.lstat(restricted_path)
        self.assertTrue(stat.S_ISDIR(st.st_mode))
        if self.expect_restricted:
            self.assertEqual(st.st_mode & 0o7777, 0)
        else:
            self.assertNotEqual(st.st_mode & 0o7777, 0)

    def test_restricted_dir_access(self) -> None:
        restricted_path = os.path.join(self.mount, "restricted")
        if self.expect_restricted:
            self.assertFalse(os.access(restricted_path, os.R_OK))
            self.assertFalse(os.access(restricted_path, os.W_OK))
            self.assertFalse(os.access(restricted_path, os.X_OK))
        else:
            self.assertTrue(os.access(restricted_path, os.R_OK))

    def test_restricted_dir_listdir(self) -> None:
        restricted_path = os.path.join(self.mount, "restricted")
        if self.expect_restricted:
            with self.assertRaises(OSError) as ctx:
                os.listdir(restricted_path)
            self.assertEqual(ctx.exception.errno, errno.EACCES)
        else:
            entries = os.listdir(restricted_path)
            self.assertIn("secret.txt", entries)

    def test_restricted_dir_file_access(self) -> None:
        secret_path = os.path.join(self.mount, "restricted", "secret.txt")
        if self.expect_restricted:
            with self.assertRaises(OSError) as ctx:
                with open(secret_path, "r") as f:
                    f.read()
            self.assertEqual(ctx.exception.errno, errno.EACCES)
        else:
            with open(secret_path, "r") as f:
                self.assertEqual("secret content", f.read())

    def test_nested_restricted_parent_is_accessible(self) -> None:
        parent_entries = sorted(os.listdir(os.path.join(self.mount, "parent")))
        self.assertIn("normal_file.txt", parent_entries)
        self.assertIn("nested_restricted", parent_entries)

    def test_nested_restricted_dir(self) -> None:
        nested_path = os.path.join(self.mount, "parent", "nested_restricted")
        if self.expect_restricted:
            st = os.lstat(nested_path)
            self.assertEqual(st.st_mode & 0o7777, 0)
            with self.assertRaises(OSError) as ctx:
                os.listdir(nested_path)
            self.assertEqual(ctx.exception.errno, errno.EACCES)
        else:
            entries = os.listdir(nested_path)
            self.assertIn("deep.txt", entries)

    def test_create_file_in_restricted_dir(self) -> None:
        new_file_path = os.path.join(self.mount, "restricted", "new_file.txt")
        if self.expect_restricted:
            with self.assertRaises(OSError) as ctx:
                with open(new_file_path, "w") as f:
                    f.write("should not be allowed")
            self.assertEqual(ctx.exception.errno, errno.EACCES)
        else:
            with open(new_file_path, "w") as f:
                f.write("should be allowed")

    async def test_glob_restricted_dirs(self) -> None:
        params = GlobParams(
            mountPoint=self.mount_path_bytes,
            globs=["**/*.txt"],
            includeDotfiles=False,
        )
        async with self.get_async_thrift_client() as client:
            result = await client.globFiles(params)

        matching = sorted(result.matchingFiles)

        self.assertIn(b"hello.txt", matching)
        self.assertIn(b"regular/file.txt", matching)
        self.assertIn(b"parent/normal_file.txt", matching)

        restricted_files = [f for f in matching if f.startswith(b"restricted/")]
        nested_restricted_files = [f for f in matching if b"nested_restricted/" in f]
        if self.expect_restricted:
            self.assertEqual(restricted_files, [])
            self.assertEqual(nested_restricted_files, [])
        else:
            self.assertGreater(len(restricted_files), 0)

    async def test_status_restricted_dirs(self) -> None:
        async with self.get_async_thrift_client() as client:
            status = await client.getScmStatusV2(
                GetScmStatusParams(
                    mountPoint=self.mount_path_bytes,
                    commit=self.initial_commit.encode(),
                    listIgnored=False,
                    rootIdOptions=None,
                )
            )

        if self.expect_restricted:
            restricted_entries = [
                path
                for path in status.status.entries
                if path.startswith(b"restricted/") or b"nested_restricted/" in path
            ]
            self.assertEqual(restricted_entries, [])

    async def test_readdir_thrift_on_restricted_dir(self) -> None:
        async with self.get_async_thrift_client() as client:
            result = await client.readdir(
                ReaddirParams(
                    mountPoint=self.mount_path_bytes,
                    directoryPaths=[b"restricted"],
                    sync=SyncBehavior(),
                )
            )
            dir_data = result.dirLists[0]
            if self.expect_restricted:
                # Restricted directories should return the union's error arm.
                self.assertEqual(DirListAttributeDataOrError.Type.error, dir_data.type)
                err = dir_data.error
                self.assertIsNotNone(err)
                if err.errorCode is not None:
                    self.assertIn(err.errorCode, [errno.EACCES, errno.EPERM])
            else:
                self.assertEqual(
                    DirListAttributeDataOrError.Type.dirListAttributeData,
                    dir_data.type,
                )
                self.assertIsNotNone(dir_data.dirListAttributeData)

    def test_checkout_restricted_to_unrestricted(self) -> None:
        self.repo.hg("update", self.initial_commit)

        if self.expect_restricted:
            with self.assertRaises(OSError) as ctx:
                os.listdir(os.path.join(self.mount, "restricted"))
            self.assertEqual(ctx.exception.errno, errno.EACCES)
        else:
            entries = os.listdir(os.path.join(self.mount, "restricted"))
            self.assertIn("secret.txt", entries)

        # After checkout to swapped, restricted/ loses .slacl -- always accessible
        self.repo.hg("update", self.swapped_commit)
        entries = sorted(os.listdir(os.path.join(self.mount, "restricted")))
        self.assertEqual(["secret.txt"], entries)

    def test_checkout_unrestricted_to_restricted(self) -> None:
        self.repo.hg("update", self.initial_commit)
        entries = sorted(os.listdir(os.path.join(self.mount, "regular")))
        self.assertEqual(["file.txt"], entries)

        # After checkout to swapped, regular/ gains .slacl
        self.repo.hg("update", self.swapped_commit)
        if self.expect_restricted:
            with self.assertRaises(OSError) as ctx:
                os.listdir(os.path.join(self.mount, "regular"))
            self.assertEqual(ctx.exception.errno, errno.EACCES)
        else:
            entries = os.listdir(os.path.join(self.mount, "regular"))
            self.assertIn("file.txt", entries)

    def test_checkout_unrestricted_to_restricted_with_local_changes(
        self,
    ) -> None:
        """Non-force checkout of unrestricted -> restricted with dirty subtree
        should surface a conflict and preserve the unrestricted directory."""
        # Start at initial_commit where regular/ is unrestricted.
        self.repo.hg("update", self.initial_commit)

        # Materialize regular/ by writing to a file inside it via the mount.
        regular_file = os.path.join(self.mount, "regular", "file.txt")
        with open(regular_file, "w") as f:
            f.write("local modification")

        if self.expect_restricted:
            # Non-force update into a restriction transition over a dirty
            # subtree should fail rather than burying the local modification
            # behind an EACCES wall. The checkout pre-check in
            # TreeInode::checkoutUpdateEntry surfaces a MODIFIED_MODIFIED
            # conflict; hg's checkout code reports this as an abort.
            with self.assertRaises(hgrepo.HgError):
                self.repo.hg("update", self.swapped_commit)

            # Local modification must still be accessible and intact.
            entries = os.listdir(os.path.join(self.mount, "regular"))
            self.assertIn("file.txt", entries)
            with open(regular_file, "r") as f:
                self.assertEqual("local modification", f.read())
        else:
            self.repo.hg("update", self.swapped_commit)
            entries = os.listdir(os.path.join(self.mount, "regular"))
            self.assertIn("file.txt", entries)

    def test_checkout_unrestricted_to_restricted_force(self) -> None:
        """Force checkout (-C) of unrestricted -> restricted with dirty
        subtree should drive the transition through and restrict the dir."""
        self.repo.hg("update", self.initial_commit)

        regular_file = os.path.join(self.mount, "regular", "file.txt")
        with open(regular_file, "w") as f:
            f.write("local modification")

        # Force update: discards local state and applies the transition.
        self.repo.hg("update", "-C", self.swapped_commit)

        # Verify the subtree is now restricted (under expect_restricted) or
        # still accessible (under config-off).
        self._assert_dir_blocked(os.path.join(self.mount, "regular"))

    def test_checkout_roundtrip_restricted(self) -> None:
        self.repo.hg("update", self.initial_commit)

        self._assert_dir_blocked(os.path.join(self.mount, "restricted"))

        self.repo.hg("update", self.swapped_commit)
        entries = sorted(os.listdir(os.path.join(self.mount, "restricted")))
        self.assertEqual(["secret.txt"], entries)

        self.repo.hg("update", self.initial_commit)
        self._assert_dir_blocked(os.path.join(self.mount, "restricted"))

    def test_checkout_unloaded_inode_becomes_restricted(self) -> None:
        self.repo.hg("update", self.initial_commit)

        # Do NOT access regular/ before checkout -- leave the inode unloaded.
        self.repo.hg("update", self.swapped_commit)

        # regular/ gains .slacl after checkout
        if self.expect_restricted:
            with self.assertRaises(OSError) as ctx:
                os.listdir(os.path.join(self.mount, "regular"))
            self.assertEqual(ctx.exception.errno, errno.EACCES)
        else:
            entries = os.listdir(os.path.join(self.mount, "regular"))
            self.assertIn("file.txt", entries)

    def test_checkout_nested_restricted_transition(self) -> None:
        self.repo.hg("update", self.initial_commit)

        # parent/nested_restricted/ has .slacl in initial commit
        if self.expect_restricted:
            with self.assertRaises(OSError) as ctx:
                os.listdir(os.path.join(self.mount, "parent", "nested_restricted"))
            self.assertEqual(ctx.exception.errno, errno.EACCES)
        else:
            entries = os.listdir(
                os.path.join(self.mount, "parent", "nested_restricted")
            )
            self.assertIn("deep.txt", entries)

        # parent/normal_file.txt is always accessible
        with open(os.path.join(self.mount, "parent", "normal_file.txt"), "r") as f:
            self.assertEqual("normal", f.read())

    def test_checkout_read_file_after_unrestrict(self) -> None:
        self.repo.hg("update", self.initial_commit)

        # restricted/secret.txt blocked or readable depending on config
        self._assert_file_blocked(os.path.join(self.mount, "restricted", "secret.txt"))

        # After checkout to swapped, restricted/ becomes accessible
        self.repo.hg("update", self.swapped_commit)
        with open(os.path.join(self.mount, "restricted", "secret.txt"), "r") as f:
            self.assertEqual("secret content", f.read())

    def test_checkout_stat_permissions_change(self) -> None:
        self.repo.hg("update", self.initial_commit)
        st = os.lstat(os.path.join(self.mount, "regular"))
        self.assertNotEqual(st.st_mode & 0o777, 0)

        # After checkout to swapped, regular/ gains .slacl
        self.repo.hg("update", self.swapped_commit)
        st = os.lstat(os.path.join(self.mount, "regular"))
        if self.expect_restricted:
            self.assertEqual(st.st_mode & 0o777, 0)
        else:
            self.assertNotEqual(st.st_mode & 0o777, 0)


class _RestrictedTreeConfigOffBase(_RestrictedTreeTestBase, metaclass=abc.ABCMeta):
    """Base for tests with restricted tree mode disabled."""

    enable_restricted_tree_mode: bool = False


class _RestrictedTreeServerOnlyBase(
    _RestrictedTreeConfigOffBase, metaclass=abc.ABCMeta
):
    """Base for tests that isolate server-side PermissionDenied enforcement."""

    enable_server_acl_enforcement: bool = True


@hg_test
# pyre-ignore[13]: T62487924
class RestrictedTreeTest(_RestrictedTreeTestMethods, _RestrictedTreeTestBase):
    """Client-side enforcement via has_acl metadata."""

    pass


@hg_test
# pyre-ignore[13]: T62487924
class RestrictedTreeEnforcementTest(
    _RestrictedTreeTestMethods, _RestrictedTreeServerOnlyBase
):
    """Server-side enforcement via PermissionDenied only."""


@hg_test
# pyre-ignore[13]: T62487924
class RestrictedTreeCombinedEnforcementTest(
    _RestrictedTreeTestMethods, _RestrictedTreeTestBase
):
    """Client-side and server-side enforcement enabled together."""

    enable_server_acl_enforcement: bool = True


@hg_test
# pyre-ignore[13]: T62487924
class RestrictedTreeConfigOffTest(
    _RestrictedTreeTestMethods, _RestrictedTreeConfigOffBase
):
    """Feature disabled — all directories accessible."""

    expect_restricted: bool = False


@hg_test
# pyre-ignore[13]: T62487924
class RestrictedTreeRebaseCombinedEnforcementTest(_RestrictedTreeTestBase):
    """Rebase over a destination that removes a loaded restricted child."""

    enable_server_acl_enforcement: bool = True

    base_commit: str = ""
    dest_commit: str = ""
    conflict_path: str = "project/notes/conflict.txt"
    restricted_path: str = "project/notes/restricted_child"

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        eagerepo_path = repo.eagerepo
        assert eagerepo_path is not None, (
            "backing HgRepository.init() must populate self.eagerepo before "
            "populate_backing_repo runs"
        )
        eager = EagerRepo(
            eagerepo_path,
            hg_environment=repo.hg_environment,
            system_hgrc=None,
        )
        eager.init()

        eager.write_file("project/notes/README.md", "base\n")
        eager.write_file(f"{self.restricted_path}/.slacl", "acl config\n")
        eager.write_file(f"{self.restricted_path}/secret.txt", "secret\n")
        self.base_commit = eager.commit("Initial restricted tree.")

        eager.remove_file(f"{self.restricted_path}/.slacl")
        eager.remove_file(f"{self.restricted_path}/secret.txt")
        eager.write_file(self.conflict_path, "destination\n")
        self.dest_commit = eager.commit("Remove restricted tree and add conflict file.")

        repo.hg("pull", "-r", self.base_commit)
        repo.hg("pull", "-r", self.dest_commit)
        repo.hg("update", self.base_commit)

    def test_rebase_removes_loaded_restricted_tree(self) -> None:
        self.repo.hg("update", self.base_commit)

        restricted_abspath = os.path.join(self.mount, self.restricted_path)
        restricted_stat = os.lstat(restricted_abspath)
        self.assertTrue(stat.S_ISDIR(restricted_stat.st_mode))
        self.assertEqual(restricted_stat.st_mode & 0o7777, 0)
        with self.assertRaises(OSError) as ctx:
            os.listdir(restricted_abspath)
        self.assertEqual(ctx.exception.errno, errno.EACCES)

        self.write_file(self.conflict_path, "local\n")
        self.repo.add_file(self.conflict_path)
        local_commit = self.repo.commit("Add local conflict file.")

        try:
            with self.assertRaises(hgrepo.HgError) as context:
                self.hg(
                    "rebase",
                    "--config",
                    "rebase.experimental.inmemory=False",
                    "-r",
                    local_commit,
                    "-d",
                    self.dest_commit,
                )

            # FIXME(T272514471): this should fail with the normal file conflict
            # for conflict_path and should not report EdenError/path ACL
            # restriction for the removed restricted subtree.
            self.assertIn(b"EdenError", context.exception.stderr)
            self.assertIn(b"path ACL restriction", context.exception.stderr)
            self.assertIn(self.restricted_path.encode(), context.exception.stderr)
        finally:
            self.hg("rebase", "--abort", check=False)
