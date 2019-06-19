#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import errno
import os
import time

from .lib import testcase


@testcase.eden_repo_test
class RenameTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.symlink("slink", "hello")
        self.repo.commit("Initial commit.")

    def test_rename_errors(self) -> None:
        """ Test some error cases """
        with self.assertRaises(OSError) as context:
            os.rename(
                os.path.join(self.mount, "not-exist"),
                os.path.join(self.mount, "also-not-exist"),
            )
        self.assertEqual(
            errno.ENOENT, context.exception.errno, msg="Renaming a bogus file -> ENOENT"
        )

    def test_rename_dir(self) -> None:
        filename = os.path.join(self.mount, "adir")
        targetname = os.path.join(self.mount, "a-new-target")
        os.rename(filename, targetname)

        with self.assertRaises(OSError) as context:
            os.lstat(filename)
        self.assertEqual(
            errno.ENOENT, context.exception.errno, msg="no longer visible as old name"
        )

        os.lstat(targetname)
        # Check that adir/file is now visible in the new location
        targetfile = os.path.join(targetname, "file")
        with open(targetfile, "r") as f:
            self.assertEqual("foo!\n", f.read())

    def test_rename_away_tree_entry(self) -> None:
        """ Rename a tree entry away and back again """
        # We should be able to rename files that are in the Tree
        hello = os.path.join(self.mount, "hello")
        targetname = os.path.join(self.mount, "a-new-target")
        os.rename(hello, targetname)

        with self.assertRaises(OSError) as context:
            os.lstat(hello)
        self.assertEqual(
            errno.ENOENT, context.exception.errno, msg="no longer visible as old name"
        )

        self.assert_checkout_root_entries({".eden", "a-new-target", "adir", "slink"})

        with open(targetname, "r") as f:
            self.assertEqual("hola\n", f.read(), msg="materialized correct data")

            # Now, while we hold this file open, check that a rename
            # leaves the handle connected to the file contents when
            # we rename it back to its old name.
            os.rename(targetname, hello)

            self.assert_checkout_root_entries({".eden", "adir", "hello", "slink"})

            with open(hello, "r+") as write_f:
                write_f.seek(0, os.SEEK_END)
                write_f.write("woot")

            f.seek(0)
            self.assertEqual("hola\nwoot", f.read())

    def test_rename_overlay_only(self) -> None:
        """ Create a local/overlay only file and rename it """
        # We should be able to rename files that are in the Tree
        from_name = os.path.join(self.mount, "overlay-a")
        to_name = os.path.join(self.mount, "overlay-b")

        with open(from_name, "w") as f:
            f.write("overlay-a\n")

        os.rename(from_name, to_name)

        with self.assertRaises(OSError) as context:
            os.lstat(from_name)
        self.assertEqual(
            errno.ENOENT, context.exception.errno, msg="no longer visible as old name"
        )

        self.assert_checkout_root_entries(
            {".eden", "adir", "hello", "overlay-b", "slink"}
        )

        with open(to_name, "r") as f:
            self.assertEqual("overlay-a\n", f.read(), msg="holds correct data")

            # Now, while we hold this file open, check that a rename
            # leaves the handle connected to the file contents when
            # we rename it back to its old name.
            os.rename(to_name, from_name)

            self.assert_checkout_root_entries(
                {".eden", "adir", "hello", "overlay-a", "slink"}
            )

            with open(from_name, "r+") as write_f:
                write_f.seek(0, os.SEEK_END)
                write_f.write("woot")

            f.seek(0)
            self.assertEqual("overlay-a\nwoot", f.read())

    def test_rename_overlay_over_tree(self) -> None:
        """ Make an overlay file and overwrite a tree entry with it """

        from_name = os.path.join(self.mount, "overlay-a")
        to_name = os.path.join(self.mount, "hello")

        with open(from_name, "w") as f:
            f.write("overlay-a\n")

        os.rename(from_name, to_name)

        with self.assertRaises(OSError) as context:
            os.lstat(from_name)
        self.assertEqual(
            errno.ENOENT, context.exception.errno, msg="no longer visible as old name"
        )

        self.assert_checkout_root_entries({".eden", "adir", "hello", "slink"})

        with open(to_name, "r") as f:
            self.assertEqual("overlay-a\n", f.read(), msg="holds correct data")

    def test_rename_between_different_dirs(self) -> None:
        adir = os.path.join(self.mount, "adir")
        bdir = os.path.join(self.mount, "bdir")
        os.mkdir(bdir)

        os.rename(os.path.join(adir, "file"), os.path.join(bdir, "FILE"))

        self.assertEqual([], sorted(os.listdir(adir)))
        self.assertEqual(["FILE"], sorted(os.listdir(bdir)))

        # Restart to ensure that our serialized state is correct
        self.eden.shutdown()
        self.eden.start()

        self.assertEqual([], sorted(os.listdir(adir)))
        self.assertEqual(["FILE"], sorted(os.listdir(bdir)))

    def test_rename_dir_over_empty_dir(self) -> None:
        subdir = os.path.join(self.mount, "sub")
        dir1 = os.path.join(subdir, "dir1")
        dir2 = os.path.join(subdir, "dir2")
        os.mkdir(subdir)
        os.mkdir(dir1)
        os.mkdir(dir2)

        os.rename(dir1, dir2)
        self.assertEqual(["dir2"], os.listdir(subdir))

    def test_rename_updates_mtime(self) -> None:
        adir_path = os.path.join(self.mount, "adir")
        mtime_root = os.lstat(self.mount).st_mtime
        mtime_adir = os.lstat(adir_path).st_mtime

        while time.time() < max(mtime_root, mtime_adir):
            time.sleep(0.01)

        os.rename(os.path.join(self.mount, "hello"), os.path.join(adir_path, "hello"))

        self.assertLess(mtime_root, os.lstat(self.mount).st_mtime)
        self.assertLess(mtime_adir, os.lstat(adir_path).st_mtime)
