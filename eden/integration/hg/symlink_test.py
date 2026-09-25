#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import asyncio
import os
from itertools import product

from eden.fs.service.eden.thrift_types import (
    CheckoutMode,
    CheckOutRevisionParams,
    ConflictType,
    FaultDefinition,
    UnblockFaultArg,
)
from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-ignore[13]: T62487924
class SymlinkTest(EdenHgTestCase):
    enable_fault_injection: bool = True
    # pyre-fixme[13]: Attribute `simple_commit` is never initialized.
    simple_commit: str
    # pyre-fixme[13]: Attribute `symlink_commit` is never initialized.
    symlink_commit: str
    # pyre-fixme[13]: Attribute `quasi_symlink_commit` is never initialized.
    quasi_symlink_commit: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("adir/hello.txt", "hola")
        self.simple_commit = repo.commit("Initial commit.")

        repo.symlink("symlink", os.path.join("adir", "hello.txt"))
        self.symlink_commit = repo.commit("Add symlink pointing to adir/hello.txt")
        repo.update(self.simple_commit)

        repo.write_file("symlink", os.path.join("adir", "hello.txt"))
        self.quasi_symlink_commit = repo.commit(
            "Add symlink lookalike 'pointing' to adir/hello.txt"
        )
        repo.update(self.simple_commit)

    def test_update_to_symlink(self) -> None:
        self.repo.update(self.quasi_symlink_commit)
        self.assertEqual(os.path.join("adir", "hello.txt"), self.read_file("symlink"))
        self.repo.update(self.symlink_commit)
        self.assertEqual("hola", self.read_file("symlink"))

    def test_update_from_symlink(self) -> None:
        self.repo.update(self.symlink_commit)
        self.assertEqual("hola", self.read_file("symlink"))
        self.repo.update(self.quasi_symlink_commit)
        self.assertEqual(os.path.join("adir", "hello.txt"), self.read_file("symlink"))

    async def test_update_symlink_over_untracked_directory_dry_run(self) -> None:
        self.backing_repo.write_file("symlink/tracked", "tracked\n")
        directory_commit = self.backing_repo.commit("Add directory")
        self.repo.update(directory_commit)
        self.write_file("symlink/untracked", "local\n")
        self.repo.update(self.simple_commit)
        self.assertEqual(["untracked"], os.listdir(self.get_path("symlink")))
        async with self.eden.get_async_thrift_client() as client:
            conflicts = await client.checkOutRevision(
                mountPoint=self.mount_path_bytes,
                snapshotHash=self.symlink_commit.encode(),
                checkoutMode=CheckoutMode.DRY_RUN,
                params=CheckOutRevisionParams(),
            )
        self.assertEqual(
            [(b"symlink", ConflictType.DIRECTORY_NOT_EMPTY)],
            [(conflict.path, conflict.type) for conflict in conflicts],
        )
        for use_rust in (False, True):
            with self.subTest(use_rust=use_rust):
                self.hg("config", "--local", "checkout.use-rust", str(use_rust))
                with self.assertRaisesRegex(hgrepo.HgError, "conflict"):
                    self.repo.update(self.symlink_commit)
        self.assertEqual(self.simple_commit, self.repo.get_head_hash())
        self.assertEqual("local\n", self.read_file("symlink/untracked"))

        # With the rejection disabled the update succeeds and leaves the
        # directory in place of the symlink.
        self.hg(
            "config",
            "--local",
            "experimental.abort-on-eden-directory-conflict",
            "False",
        )
        for use_rust in (False, True):
            with self.subTest(use_rust=use_rust, abort=False):
                self.hg("config", "--local", "checkout.use-rust", str(use_rust))
                self.repo.update(self.symlink_commit)
                self.assertEqual(self.symlink_commit, self.repo.get_head_hash())
                self.assertEqual("local\n", self.read_file("symlink/untracked"))
                self.repo.update(self.simple_commit, clean=True)

    async def test_update_symlink_over_locally_removed_child(self) -> None:
        self.backing_repo.write_file("symlink/tracked", "tracked\n")
        directory_commit = self.backing_repo.commit("Add directory")
        self.repo.update(directory_commit)
        os.unlink(self.get_path("symlink/tracked"))
        async with self.eden.get_async_thrift_client() as client:
            for mode in (CheckoutMode.DRY_RUN, CheckoutMode.NORMAL):
                conflicts = await client.checkOutRevision(
                    mountPoint=self.mount_path_bytes,
                    snapshotHash=self.symlink_commit.encode(),
                    checkoutMode=mode,
                    params=CheckOutRevisionParams(),
                )
                self.assertEqual(
                    [(b"symlink/tracked", ConflictType.MISSING_REMOVED)],
                    [(conflict.path, conflict.type) for conflict in conflicts],
                )
        self.assertEqual("hola", self.read_file("symlink"))

    async def test_update_symlink_with_file_created_during_checkout(self) -> None:
        self.backing_repo.write_file("symlink/tracked", "tracked\n")
        directory_commit = self.backing_repo.commit("Add directory")
        async with self.eden.get_async_thrift_client() as client:
            for use_rust, clean in product((False, True), repeat=2):
                with self.subTest(use_rust=use_rust, clean=clean):
                    self.hg("config", "--local", "checkout.use-rust", str(use_rust))
                    self.repo.update(directory_commit, clean=True)
                    # Materialization allows writes while checkout holds the rename lock.
                    self.write_file("symlink/tracked", "tracked\n")
                    await client.injectFault(
                        FaultDefinition(
                            keyClass="TreeInode::checkout",
                            keyValueRegex="symlink, false",
                            block=True,
                            count=1,
                        )
                    )
                    update = asyncio.create_task(
                        asyncio.to_thread(
                            self.repo.update, self.symlink_commit, clean=clean
                        )
                    )
                    try:
                        await asyncio.to_thread(
                            self.wait_on_fault_hit, key_class="TreeInode::checkout"
                        )
                        self.write_file("symlink/untracked", "local\n")
                    finally:
                        await client.unblockFault(
                            UnblockFaultArg(
                                keyClass="TreeInode::checkout",
                                keyValueRegex="symlink, false",
                            )
                        )
                    with self.assertRaisesRegex(hgrepo.HgError, "conflict"):
                        await update
                    # Eden already moved to the destination. The dirstate
                    # parent stays behind until a clean update replaces the
                    # directory.
                    self.assertEqual(directory_commit, self.repo.get_head_hash())
                    self.assertEqual(
                        {"symlink": "!", "symlink/untracked": "?"},
                        self.repo.status(),
                    )
                    self.assertEqual("local\n", self.read_file("symlink/untracked"))
                    self.repo.update(self.symlink_commit, clean=True)
                    self.assertEqual(self.symlink_commit, self.repo.get_head_hash())
                    self.assertEqual("hola", self.read_file("symlink"))

    async def test_update_symlink_after_failed_directory_removal(self) -> None:
        self.backing_repo.write_file("symlink/tracked", "tracked\n")
        directory_commit = self.backing_repo.commit("Add directory")
        self.repo.update(directory_commit)
        self.write_file("symlink/tracked", "local\n")
        results = {}
        async with self.eden.get_async_thrift_client() as client:
            for mode in (CheckoutMode.DRY_RUN, CheckoutMode.NORMAL):
                conflicts = await client.checkOutRevision(
                    mountPoint=self.mount_path_bytes,
                    snapshotHash=self.symlink_commit.encode(),
                    checkoutMode=mode,
                    params=CheckOutRevisionParams(),
                )
                results[mode] = [(c.path, c.type) for c in conflicts]
        # A modified tracked file is a conflict on its own. Only local-only
        # entries make the directory itself a conflict before checkout runs.
        self.assertEqual(
            [(b"symlink/tracked", ConflictType.MODIFIED_REMOVED)],
            results[CheckoutMode.DRY_RUN],
        )
        self.assertCountEqual(
            [
                (b"symlink/tracked", ConflictType.MODIFIED_REMOVED),
                (b"symlink", ConflictType.DIRECTORY_NOT_EMPTY),
            ],
            results[CheckoutMode.NORMAL],
        )
        self.assertEqual("local\n", self.read_file("symlink/tracked"))

    def test_update_symlink_over_untracked_descendant_clean(self) -> None:
        self.backing_repo.write_file("symlink/subdir/tracked", "tracked\n")
        directory_commit = self.backing_repo.commit("Add directory")
        self.repo.update(directory_commit)
        self.write_file("symlink/subdir/untracked", "local\n")
        self.repo.update(self.symlink_commit, clean=True)
        self.assertTrue(os.path.islink(self.get_path("symlink")))
        self.assertEqual("hola", self.read_file("symlink"))

    async def test_update_symlink_over_untracked_descendant_clean_disabled(
        self,
    ) -> None:
        self.write_configs(
            {"experimental": ["force-checkout-removes-local-only = false"]},
            self.eden.user_rc_path,
        )
        self.backing_repo.write_file("symlink/tracked", "tracked\n")
        directory_commit = self.backing_repo.commit("Add directory")
        self.repo.update(directory_commit)
        self.write_file("symlink/untracked", "local\n")
        async with self.eden.get_async_thrift_client() as client:
            await client.reloadConfig()
            conflicts = await client.checkOutRevision(
                mountPoint=self.mount_path_bytes,
                snapshotHash=self.symlink_commit.encode(),
                checkoutMode=CheckoutMode.FORCE,
                params=CheckOutRevisionParams(),
            )
        self.assertEqual(
            [(b"symlink", ConflictType.DIRECTORY_NOT_EMPTY)],
            [(conflict.path, conflict.type) for conflict in conflicts],
        )
        self.assertEqual("local\n", self.read_file("symlink/untracked"))

    def test_clean_update_keeps_untracked_in_deleted_directory(self) -> None:
        self.backing_repo.write_file("symlink/tracked", "tracked\n")
        directory_commit = self.backing_repo.commit("Add directory")
        self.repo.update(directory_commit)
        self.write_file("symlink/untracked", "local\n")
        self.repo.update(self.simple_commit, clean=True)
        self.assertEqual(self.simple_commit, self.repo.get_head_hash())
        self.assertEqual(["untracked"], os.listdir(self.get_path("symlink")))

    async def test_update_locally_replaced_symlink_to_directory(self) -> None:
        self.backing_repo.write_file("symlink/tracked", "tracked\n")
        directory_commit = self.backing_repo.commit("Add directory")
        self.repo.update(self.symlink_commit)
        os.unlink(self.get_path("symlink"))
        self.write_file("symlink/untracked", "local\n")
        async with self.eden.get_async_thrift_client() as client:
            conflicts = await client.checkOutRevision(
                mountPoint=self.mount_path_bytes,
                snapshotHash=directory_commit.encode(),
                checkoutMode=CheckoutMode.DRY_RUN,
                params=CheckOutRevisionParams(),
            )
        # FIXME: Recurse into the local directory instead of treating it as a file.
        self.assertEqual(
            [(b"symlink", ConflictType.MODIFIED_MODIFIED)],
            [(conflict.path, conflict.type) for conflict in conflicts],
        )
        with self.assertRaisesRegex(
            hgrepo.HgError, "file metadata for symlink not found at target commit"
        ):
            self.repo.update(directory_commit)
        self.assertEqual(self.symlink_commit, self.repo.get_head_hash())
        self.assertEqual("local\n", self.read_file("symlink/untracked"))

    def test_show_symlink_commit(self) -> None:
        self.repo.update(self.symlink_commit)
        self.assertEqual(
            self.repo.hg("log", "-r", ".", "--template", "{node}", "--patch"),
            """3f0b136eff77afd59a710c48d6e5f178793d08cediff --git a/symlink b/symlink
new file mode 120000
--- /dev/null
+++ b/symlink
@@ -0,0 +1,1 @@
+adir/hello.txt
\\ No newline at end of file

""",
        )

    def test_hg_mv_symlink_file(self) -> None:
        self.repo.update(self.symlink_commit)
        self.repo.hg("mv", "symlink", "symbolic_link")
        self.repo.commit("Moving symlink")
        self.assertEqual(self.read_file("symbolic_link"), "hola")
        self.assert_status_empty()
        self.assertEqual(
            self.repo.hg("log", "-r", ".", "--template", "{node}\\n", "--patch"),
            """b80dd1449ad6a2d0ae67936c905fa1e79d9ba65a
diff --git a/symlink b/symbolic_link
rename from symlink
rename to symbolic_link

""",
        )

    def test_hg_mv_symlink_dir(self) -> None:
        self.repo.symlink("symlink", "adir", target_is_directory=True)
        self.repo.commit("Created directory symlink")
        self.repo.hg("mv", "symlink", "symbolic_link")
        self.repo.commit("Moving symlink")
        self.assertEqual(
            ["hello.txt"],
            [entry.name for entry in os.scandir(self.get_path("symbolic_link"))],
        )
        self.assert_status_empty()
        self.assertEqual(
            self.repo.hg("log", "-r", ".", "--template", "{node}\\n", "--patch"),
            """08ba755d91dc22433da9e170bcb95bc87da38aab
diff --git a/symlink b/symbolic_link
rename from symlink
rename to symbolic_link

""",
        )

    def test_modified_symlink_target(self) -> None:
        self.repo.update(self.symlink_commit)
        self.assert_status_empty()
        self.repo.write_file("adir/true_hola.txt", "hola")
        os.remove(self.get_path("symlink"))
        self.repo.symlink("symlink", os.path.join("adir", "true_hola.txt"))
        self.assert_status({"adir/true_hola.txt": "?", "symlink": "M"})
        self.assertEqual(
            self.repo.hg("diff"),
            r"""diff --git a/symlink b/symlink
--- a/symlink
+++ b/symlink
@@ -1,1 +1,1 @@
-adir/hello.txt
\ No newline at end of file
+adir/true_hola.txt
\ No newline at end of file
""",
        )

    def test_symlink_diff(self) -> None:
        self.repo.update(self.symlink_commit)
        os.remove(self.get_path("symlink"))
        self.write_file("symlink", os.path.join("adir", "hello.txt"))
        self.assertEqual(
            self.repo.hg("diff"),
            (
                """diff --git a/symlink b/symlink
old mode 120000
new mode 100644
"""
                if os.name != "nt"
                else r"""diff --git a/symlink b/symlink
old mode 120000
new mode 100644
--- a/symlink
+++ b/symlink
@@ -1,1 +1,1 @@
-adir/hello.txt
\ No newline at end of file
+adir\hello.txt
\ No newline at end of file
"""
            ),
        )
        self.repo.update(self.quasi_symlink_commit, clean=True)
        os.remove(self.get_path("symlink"))
        self.repo.symlink("symlink", os.path.join("adir", "hello.txt"))
        self.assertEqual(
            self.repo.hg("diff"),
            (
                """diff --git a/symlink b/symlink
old mode 100644
new mode 120000
"""
                if os.name != "nt"
                else r"""diff --git a/symlink b/symlink
old mode 100644
new mode 120000
--- a/symlink
+++ b/symlink
@@ -1,1 +1,1 @@
-adir\hello.txt
\ No newline at end of file
+adir/hello.txt
\ No newline at end of file
"""
            ),
        )

    def test_directory_listing(self) -> None:
        self.repo.update(self.symlink_commit)
        files = os.scandir(self.mount)
        checkedSymlink = False
        for file in files:
            if file.name == "symlink":
                checkedSymlink = file.is_symlink()
        self.assertTrue(checkedSymlink)

    def test_revert(self) -> None:
        self.repo.update(self.symlink_commit)
        os.remove(self.get_path("symlink"))
        self.assert_status({"symlink": "!"})
        self.repo.hg("revert", "--all")
        self.assert_status_empty()
        self.assertEqual("hola", self.read_file("symlink"))

    def test_manually_restoring_symlink(self) -> None:
        self.repo.update(self.symlink_commit)
        os.remove(self.get_path("symlink"))
        self.assert_status({"symlink": "!"})
        self.repo.symlink("symlink", os.path.join("adir", "hello.txt"))
        self.assert_status_empty()
        self.assertEqual("hola", self.read_file("symlink"))

    def test_hg_update_works_with_symlink_feature(self) -> None:
        # Tests that what didn't work on test_failing_update works with symlinks enabled
        self.repo.update(self.symlink_commit)
        self.repo.symlink("symlink3", os.path.join("adir", "hello.txt"))
        self.repo.commit("Another commit with a symlink")
        self.repo.update(self.simple_commit)
        self.assert_status_empty()

    def test_file_symlink_chain(self) -> None:
        self.repo.symlink("f1", os.path.join("adir", "hello.txt"))
        self.repo.symlink("f2", "f1")
        self.repo.symlink("f3", "f2")
        file_symlink_chain_commit = self.repo.commit(
            "Chain of symlinks pointing to a file in a dir"
        )
        self.assert_status({})
        self.repo.update(self.simple_commit, clean=True)
        self.repo.update(file_symlink_chain_commit)
        self.assertTrue(os.path.isfile(self.get_path("f3")))
        self.assertEqual("hola", self.read_file("f3"))

    def test_dir_symlink_chain(self) -> None:
        self.repo.symlink("d1", "adir", target_is_directory=True)
        self.repo.symlink("d2", "d1", target_is_directory=True)
        self.repo.symlink("d3", "d2", target_is_directory=True)
        self.assertTrue(os.path.isdir(self.get_path("d3")))
        dir_symlink_chain_commit = self.repo.commit(
            "Chain of symlinks pointing to a directory"
        )
        self.assert_status({})
        self.repo.update(self.simple_commit, clean=True)
        self.repo.update(dir_symlink_chain_commit)
        self.assertTrue(os.path.isdir(self.get_path("d3")))
        self.assertEqual("hola", self.read_file(os.path.join("d3", "hello.txt")))

    def test_symlink_chain_directory_listing(self) -> None:
        self.repo.write_file("a/b/hello.txt", "hola")
        self.repo.write_file("a/b/bye.txt", "adios")
        self.repo.symlink("x/y/z/w", "../../../a/b", target_is_directory=True)
        self.assertEqual("adios", self.read_file("x/y/z/w/bye.txt"))
        scommit = self.repo.commit("Commit that adds symlinks")
        # We first run update to make the symlink disappear
        self.repo.update(self.simple_commit)
        # And then again to making sure it is are materialized properly
        self.repo.update(scommit)
        self.assertEqual(
            {"bye.txt", "hello.txt"},
            {p.name for p in os.scandir(os.path.join(self.mount, "x/y/z/w"))},
        )

    def test_symlink_cycle(self) -> None:
        self.repo.symlink("s0", "s2")
        self.repo.symlink("s1", "s0")
        self.repo.symlink("s2", "s1")
        cycle_symlink_commit = self.repo.commit(
            "Cycle of symlinks; type should be unresolvable"
        )
        self.assert_status({})
        self.repo.update(self.simple_commit, clean=True)
        self.repo.update(cycle_symlink_commit)
        for i in range(3):
            curpath = self.get_path(f"s{i}")
            self.assertFalse(os.path.isfile(curpath))
            self.assertFalse(os.path.isdir(curpath))
            self.assertEqual(f"s{(i + 2) % 3}", os.readlink(curpath))

    def test_status_on_dir_symlink(self) -> None:
        self.repo.symlink("dirlink", "adir", target_is_directory=True)
        self.repo.commit("Really simple commit w/ repo")
        self.repo.write_file("adir/hello.txt", "saluton")
        self.assert_status({"adir/hello.txt": "M"})

    def test_abspath_symlink(self) -> None:
        if os.name == "nt":
            targetisdir = True
        else:
            targetisdir = False
        self.repo.symlink("symlink", self.get_path("adir/hello.txt"))
        self.assertEqual("hola", self.read_file("symlink"))
        filecommit = self.repo.commit("Create a symlink to absolute path")
        self.assert_status({})
        self.repo.update(self.simple_commit)
        self.repo.symlink(
            "symlink", self.get_path("adir"), target_is_directory=targetisdir
        )
        dircommit = self.repo.commit("Create a symlink to absolute path")
        self.assert_status({})
        self.repo.update(self.simple_commit)
        self.repo.update(dircommit)
        self.assertTrue(os.path.isdir(self.get_path("symlink")))
        self.assertEqual(
            ["hello.txt"],
            [entry.name for entry in os.scandir(self.get_path("symlink"))],
        )
        self.repo.update(filecommit, clean=True)
        self.assertTrue(os.path.isfile(self.get_path("symlink")))
        self.assertEqual("hola", self.read_file("symlink"))

    def test_abspath_posixstyle_symlink(self) -> None:
        self.repo.symlink("slink", os.sep.join(["", "foo", "bar"]))
        slinkcommit = self.repo.commit("Symlink with unixpaths")
        self.assertEqual(
            self.repo.hg("log", "-r", ".", "--template", "{node}", "--patch"),
            r"""31ca60316fb55b7165c8c9257374ef4d4a09c13bdiff --git a/slink b/slink
new file mode 120000
--- /dev/null
+++ b/slink
@@ -0,0 +1,1 @@
+/foo/bar
\ No newline at end of file

""",
        )
        self.repo.update(self.simple_commit)
        self.repo.update(slinkcommit)
        self.assertEqual(
            os.readlink(self.get_path("slink")), os.sep.join(["", "foo", "bar"])
        )

    def test_non_existing_symlink_targets(self) -> None:
        self.repo.symlink("slink2", os.sep.join(["asdf", "aoeu"]))
        self.repo.symlink("slink3", os.sep.join(["..", "snth"]))
        slinkcommit = self.repo.commit("non-existing targets")
        self.repo.update(self.simple_commit)
        self.repo.update(slinkcommit)
        self.assertEqual(
            os.readlink(self.get_path("slink2")), os.sep.join(["asdf", "aoeu"])
        )
        self.assertEqual(
            os.readlink(self.get_path("slink3")), os.sep.join(["..", "snth"])
        )

    def test_path_with_symlinks(self) -> None:
        # Tests that symlinks with paths are properly classified
        # Replacement at beginning of path
        self.repo.write_file("foo/bar/baz/f", "aoeu")
        self.repo.symlink("y", "foo", target_is_directory=True)
        self.repo.symlink(
            "x", os.path.join("y", "bar", "baz"), target_is_directory=True
        )
        # Replacement at end of path
        self.repo.write_file("p/q/r/f", "snth")
        self.repo.symlink("p/y", "q", target_is_directory=True)
        self.repo.symlink("p/x", os.path.join("y", "r"), target_is_directory=True)
        # Replacement in the middle of path
        self.repo.write_file("uno/dos/tres/f", "wut")
        self.repo.symlink("uno/dos/z", "tres", target_is_directory=True)
        self.repo.symlink("uno/y", "dos", target_is_directory=True)
        self.repo.symlink("uno/x", os.path.join("y", "z"), target_is_directory=True)
        # Replacement in the middle of path (absolute)
        self.repo.write_file("one/two/three/f", "ftw")
        self.repo.symlink("one/two/z", "three", target_is_directory=True)
        self.repo.symlink("one/y", self.get_path("one/two"), target_is_directory=True)
        self.repo.symlink("one/x", os.path.join("y", "z"), target_is_directory=True)
        ## Now revert everything and check that symlinks and are correct
        slinkcommit = self.repo.commit("path stuff")
        self.repo.update(self.simple_commit, clean=True)
        self.repo.update(slinkcommit, clean=True)
        for tdir, cntt in [
            ("x", "aoeu"),
            ("p/x", "snth"),
            ("uno/x", "wut"),
            ("one/x", "ftw"),
        ]:
            self.assertEqual(
                ["f"],
                [e.name for e in os.scandir(os.path.join(self.mount, tdir))],
            )
            self.assertTrue(os.path.isdir(self.get_path(tdir)))
            self.assertEqual(cntt, self.read_file(tdir + "/f"))
