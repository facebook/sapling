#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import ctypes
import mmap
import os
import subprocess
from ctypes import c_int, c_size_t, c_ssize_t, c_void_p

from .lib import testcase


# python's mmap does not allow mapping larger than the file's size
libc = ctypes.CDLL("libc.so.6")
c_off_t = c_ssize_t

libc.mmap.argtypes = [c_void_p, c_size_t, c_int, c_int, c_off_t]
libc.mmap.restype = ctypes.POINTER(ctypes.c_byte)

libc.munmap.restype = c_void_p
libc.munmap.argtypes = [c_void_p, c_size_t]


@testcase.eden_repo_test
class MmapTest(testcase.EdenRepoTest):
    contents = "abcdef"
    filename: str

    def populate_repo(self) -> None:
        self.repo.write_file("filename", self.contents)
        self.repo.commit("Initial commit.")
        self.filename = os.path.join(self.mount, "filename")

    def test_mmap_in_backed_file_is_null_terminated(self) -> None:
        fd = os.open(self.filename, os.O_RDONLY)
        try:
            size = os.fstat(fd).st_size
            self.assertEqual(len(self.contents), size)

            map_size = (size + 4095) // 4096 * 4096
            self.assertNotEqual(size, map_size)

            m = libc.mmap(None, map_size, mmap.PROT_READ, mmap.MAP_PRIVATE, fd, 0)
            try:
                # assert the additional mapped bytes are null, per `man 2 mmap`
                for i in range(size, map_size):
                    self.assertEqual(0, m[i])
            finally:
                libc.munmap(m, map_size)
        finally:
            os.close(fd)

    def test_mmap_is_null_terminated_after_truncate_and_write_to_overlay(self) -> None:
        # WARNING: This test is very fiddly.

        # The bug is that if a file in Eden is opened with O_TRUNC followed by
        # a series of writes, then mmap of that file with a size larger than the
        # file (but still within the trailing page) does not zero the trailing
        # bytes.  Clang relies on this mmap behavior to enforce that the buffer
        # is null-terminated.  Since the buffer ends up not being null-
        # terminated, Clang segfaults.
        #
        # It seems like this is a kernel or FUSE bug more than an Eden bug,
        # but we should verify nonetheless that it does not occur.

        # If this test uses the same file committed in populate_repo, the bug
        # does not reproduce.
        filename = os.path.join(self.mount, "filename2")

        # Write to the file from another process.  if this process writes the
        # file, the bug is not reproduced.
        subprocess.check_call(
            ["dd", "if=/dev/urandom", "of=" + filename, "bs=4096", "count=6"]
        )

        # A few pages, with data slightly beyond a page boundary.
        new_contents = b"abcd" * 3072 + b"abcdef"
        new_size = len(new_contents)

        # Write to the file with another process.  if this process writes the
        # file, the bug is not reproduced.
        with subprocess.Popen(
            ["dd", "of=" + filename, "bs=512"], stdin=subprocess.PIPE
        ) as p:
            p.stdin.write(new_contents)

        fd = os.open(filename, os.O_RDONLY)
        try:
            size = os.fstat(fd).st_size
            self.assertEqual(new_size, size)

            # Map all the way up to a page boundary.
            map_size = (size + 4095) // 4096 * 4096
            self.assertNotEqual(size, map_size)

            m = libc.mmap(None, map_size, mmap.PROT_READ, mmap.MAP_PRIVATE, fd, 0)
            try:
                # Assert the additional mapped bytes are null, per `man 2 mmap`
                for i in range(size, map_size):
                    self.assertEqual(0, m[i])
            finally:
                libc.munmap(m, map_size)
        finally:
            os.close(fd)
