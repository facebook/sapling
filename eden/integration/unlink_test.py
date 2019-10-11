#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import errno
import os
import shutil

from .lib import testcase


@testcase.eden_repo_test
class UnlinkTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.symlink("slink", "hello")
        self.repo.commit("Initial commit.")

    def test_unlink(self) -> None:
        filename = os.path.join(self.mount, "hello")

        # This file is part of the git repo
        with open(filename, "r") as f:
            self.assertEqual("hola\n", f.read())

        # Removing should succeed
        os.unlink(filename)

        with self.assertRaises(OSError) as context:
            os.lstat(filename)
        self.assertEqual(
            context.exception.errno,
            errno.ENOENT,
            msg="lstat on a removed file raises ENOENT",
        )

    def test_unlink_bogus_file(self) -> None:
        with self.assertRaises(OSError) as context:
            os.unlink(os.path.join(self.mount, "this-file-does-not-exist"))
        self.assertEqual(
            context.exception.errno,
            errno.ENOENT,
            msg="unlink raises ENOENT for nonexistent file",
        )

    def test_unlink_dir(self) -> None:
        adir = os.path.join(self.mount, "adir")
        with self.assertRaises(OSError) as context:
            os.unlink(adir)
        self.assertEqual(
            context.exception.errno, errno.EISDIR, msg="unlink on a dir raises EISDIR"
        )

    def test_unlink_empty_dir(self) -> None:
        adir = os.path.join(self.mount, "an-empty-dir")
        os.mkdir(adir)
        with self.assertRaises(OSError) as context:
            os.unlink(adir)
        self.assertEqual(
            context.exception.errno,
            errno.EISDIR,
            msg="unlink on an empty dir raises EISDIR",
        )

    def test_rmdir_file(self) -> None:
        filename = os.path.join(self.mount, "hello")

        with self.assertRaises(OSError) as context:
            os.rmdir(filename)
        self.assertEqual(
            context.exception.errno, errno.ENOTDIR, msg="rmdir on a file raises ENOTDIR"
        )

    def test_rmdir(self) -> None:
        adir = os.path.join(self.mount, "adir")
        with self.assertRaises(OSError) as context:
            os.rmdir(adir)
        self.assertEqual(
            context.exception.errno,
            errno.ENOTEMPTY,
            msg="rmdir on a non-empty dir raises ENOTEMPTY",
        )

        shutil.rmtree(adir)
        with self.assertRaises(OSError) as context:
            os.lstat(adir)
        self.assertEqual(
            context.exception.errno,
            errno.ENOENT,
            msg="lstat on a removed dir raises ENOENT",
        )

    def test_rmdir_overlay(self) -> None:
        # Ensure that removing dirs works with things we make in the overlay
        deep_root = os.path.join(self.mount, "buck-out")
        deep_name = os.path.join(deep_root, "foo", "bar", "baz")
        os.makedirs(deep_name)
        with self.assertRaises(OSError) as context:
            os.rmdir(deep_root)
        self.assertEqual(
            context.exception.errno,
            errno.ENOTEMPTY,
            msg="rmdir on a non-empty dir raises ENOTEMPTY",
        )

        shutil.rmtree(deep_root)
        with self.assertRaises(OSError) as context:
            os.lstat(deep_root)
        self.assertEqual(
            context.exception.errno,
            errno.ENOENT,
            msg="lstat on a removed dir raises ENOENT",
        )
