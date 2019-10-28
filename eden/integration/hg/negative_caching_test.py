#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import errno
import sys

from eden.integration.hg.lib.hg_extension_test_base import EdenHgTestCase, hg_test
from eden.integration.lib import hgrepo


# This test is primarily exercising FUSE filesystem behavior.
# We don't care too much about the mercurial configuration, so only run this
# test with the TreeOnly configuration, rather than running it with
# multiple configurations.
@hg_test("TreeOnly")
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class NegativeCachingTest(EdenHgTestCase):
    commit1: str
    commit2: str
    commit3: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("src/main.c", "hello world\n")
        repo.write_file("src/hello.c", "hello2\n")
        repo.write_file("src/test/test.c", "test\n")
        repo.write_file("src/foo/bar.txt", "bar")
        self.commit1 = repo.commit("Initial commit.")

        repo.hg("rm", "src/test/test.c")
        self.commit2 = repo.commit("Remove src/hello.c")

        repo.hg("rm", "src/hello.c")
        self.commit3 = repo.commit("Remove src/hello.c")

    def assert_enoent(self, path: str) -> None:
        with self.assertRaises(EnvironmentError) as errmgr:
            self.read_file("path")
        self.assertEqual(errmgr.exception.errno, errno.ENOENT)

    def test_file(self) -> None:
        # Turn on the strace log category just so we can confirm in the logs
        # that read 2 below does not trigger another lookup() call
        # (TODO: it would be nice if we had a better programmatic way to
        # confirm this.)
        self.eden.set_log_level("eden.strace", "DBG7")
        print("=== read 1 (enoent)", file=sys.stderr)
        self.assertEqual("hello world\n", self.read_file("src/main.c"))
        self.assert_enoent("src/hello.c")

        print("=== read 2 (enoent)", file=sys.stderr)
        self.assert_enoent("src/hello.c")

        # Check out commit2, where src/hello.c exists
        self.eden.set_log_level("eden.strace", "ERR")
        print("=== checkout", file=sys.stderr)
        self.repo.update(self.commit2)
        self.eden.set_log_level("eden.strace", "DBG7")

        # Make sure we can successfully read src/hello.c after the checkout
        print("=== read 3", file=sys.stderr)
        self.assertEqual("hello2\n", self.read_file("src/hello.c"))
        print("=== read 4", file=sys.stderr)
        self.assertEqual("hello2\n", self.read_file("src/hello.c"))

        self.eden.set_log_level("eden.strace", "ERR")

    def test_directory(self) -> None:
        self.eden.set_log_level("eden.strace", "DBG7")
        print("=== read 1 (enoent)", file=sys.stderr)
        self.assertEqual("hello world\n", self.read_file("src/main.c"))
        self.assert_enoent("src/hello.c")
        self.assert_enoent("src/test/test.c")
        self.assert_enoent("src/test")

        print("=== read 2 (enoent)", file=sys.stderr)
        self.assert_enoent("src/hello.c")
        self.assert_enoent("src/test/test.c")
        self.assert_enoent("src/test")

        # Check out commit1, where src/test/test.c exists
        self.eden.set_log_level("eden.strace", "ERR")
        print("=== checkout", file=sys.stderr)
        self.repo.update(self.commit1)
        self.eden.set_log_level("eden.strace", "DBG7")

        # Make sure we can successfully read src/test/test.c and src/hello.c
        # after the checkout
        print("=== read 3", file=sys.stderr)
        self.assertEqual("test\n", self.read_file("src/test/test.c"))
        self.assertEqual("hello2\n", self.read_file("src/hello.c"))
        print("=== read 4", file=sys.stderr)
        self.assertEqual("test\n", self.read_file("src/test/test.c"))
        self.assertEqual("hello2\n", self.read_file("src/hello.c"))

        self.eden.set_log_level("eden.strace", "ERR")
