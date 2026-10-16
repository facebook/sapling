#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""File names on EdenFS: non-ASCII and non-UTF-8 names, NUL in a path, long
working directories, hard links and readlink with bytes paths.

Ported from CPython's ``Lib/test/test_posix.py`` (``PosixTester``) and
``Lib/test/test_os.py`` (``MiscTests``, ``Pep383Tests``, ``LinkTests``,
``ReadlinkTests``, ``NonLocalSymlinkTests``), PSF license, Python 3.12: one
test method per CPython test, named after it, with the CPython class and test
in the docstring, calling the ``posix`` module directly. ``README.md`` in
this directory lists every CPython test with its method and the checks that
were left out.

CPython's ``os_helper`` names: ``TESTFN_UNICODE`` is an ASCII name plus
``-\\xe0\\xf2\\u0258\\u0141\\u011f``, ``TESTFN_NONASCII`` plus ``-\\xe6``,
``TESTFN_UNENCODABLE`` plus ``-\\udcff`` (``b"-\\xff"`` with surrogateescape,
not valid UTF-8). EdenFS by design requires UTF-8 names (``requireUtf8Path``):
a name that is not UTF-8 fails with EILSEQ over FUSE and with EINVAL over NFS
(``NFS3ERR_INVAL``).
"""

from __future__ import annotations

import errno
import posix
import stat

from eden.integration.lib import testcase

from .base import PosixTestBase


UNICODE_NAME: str = "n-\xe0\xf2\u0258\u0141\u011f"
NONASCII_NAME: str = "n-\xe6"
UNENCODABLE_NAME: bytes = b"n-\xff"


class _NamesBase(PosixTestBase):
    def not_utf8_errno(self) -> int:
        """EdenFS by design: EILSEQ over FUSE, EINVAL over NFS."""
        return errno.EINVAL if self.use_nfs() else errno.EILSEQ


@testcase.eden_nfs_repo_test
class Pep383Test(_NamesBase):
    """``test_os.Pep383Tests``: a directory of non-ASCII names.

    CPython uses ``TESTFN_UNENCODABLE`` as the directory name; EdenFS
    refuses it, so the port pins that and uses ``TESTFN_NONASCII`` for the
    directory, with the two UTF-8 names inside and the unencodable one
    pinned again.
    """

    def setup_names(self) -> set[str]:
        with self.assert_errno(self.not_utf8_errno()):
            posix.mkdir(UNENCODABLE_NAME, 0o755)
        posix.mkdir(NONASCII_NAME, 0o755)
        for name in (UNICODE_NAME, NONASCII_NAME):
            self.create_file(f"{NONASCII_NAME}/{name}")
        with self.assert_errno(self.not_utf8_errno()):
            self.create_file(NONASCII_NAME.encode() + b"/" + UNENCODABLE_NAME)
        return {UNICODE_NAME, NONASCII_NAME}

    def test_listdir(self) -> None:
        """test_os.Pep383Tests.test_listdir: listdir returns the names, str and bytes"""
        expected = self.setup_names()
        self.assertEqual(expected, set(posix.listdir(NONASCII_NAME)))
        self.assertEqual(
            {name.encode() for name in expected},
            set(posix.listdir(NONASCII_NAME.encode())),
        )
        current_directory = posix.getcwd()
        try:
            posix.chdir(NONASCII_NAME)
            self.assertEqual(expected, set(posix.listdir()))
        finally:
            posix.chdir(current_directory)

    def test_open(self) -> None:
        """test_os.Pep383Tests.test_open: open each name"""
        for name in self.setup_names():
            posix.close(posix.open(f"{NONASCII_NAME}/{name}", posix.O_RDONLY))
        with self.assert_errno(self.not_utf8_errno()):
            posix.open(NONASCII_NAME.encode() + b"/" + UNENCODABLE_NAME, posix.O_RDONLY)

    def test_statvfs(self) -> None:
        """test_os.Pep383Tests.test_statvfs: statvfs each name (issue #9645)"""
        for name in self.setup_names():
            posix.statvfs(f"{NONASCII_NAME}/{name}")

    def test_stat(self) -> None:
        """test_os.Pep383Tests.test_stat: stat each name"""
        for name in self.setup_names():
            self.assertTrue(stat.S_ISREG(posix.stat(f"{NONASCII_NAME}/{name}").st_mode))
        with self.assert_errno(self.not_utf8_errno()):
            posix.stat(NONASCII_NAME.encode() + b"/" + UNENCODABLE_NAME)


@testcase.eden_nfs_repo_test
class NamesTest(_NamesBase):
    def test_path_with_null_character(self) -> None:
        """test_posix.PosixTester.test_path_with_null_character: NUL in a str path"""
        fn_with_nul = "n0\0"
        with self.assertRaises(ValueError):
            posix.open(fn_with_nul, posix.O_WRONLY | posix.O_CREAT)
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n0")
        with self.assertRaises(ValueError):
            posix.mkdir(fn_with_nul)
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n0")
        self.create_file("n0")
        with self.assertRaises(ValueError):
            posix.stat(fn_with_nul)

    def test_path_with_null_byte(self) -> None:
        """test_posix.PosixTester.test_path_with_null_byte: NUL in a bytes path"""
        fn_with_nul = b"n0\0"
        with self.assertRaises(ValueError):
            posix.open(fn_with_nul, posix.O_WRONLY | posix.O_CREAT)
        with self.assert_errno(errno.ENOENT):
            posix.lstat(b"n0")
        with self.assertRaises(ValueError):
            posix.mkdir(fn_with_nul)
        with self.assert_errno(errno.ENOENT):
            posix.lstat(b"n0")
        self.create_file(b"n0")
        with self.assertRaises(ValueError):
            posix.stat(fn_with_nul)

    def test_getcwd_long_pathnames(self) -> None:
        """test_posix.PosixTester.test_getcwd_long_pathnames: getcwd in a deep tree

        CPython's nested helper is unreachable (it follows a ``return``);
        the port does what it describes: nest directories until the path is
        longer than 1027 bytes and call getcwd at every level.
        """
        dirname = "getcwd-test-directory-0123456789abcdef-01234567890abcdef"
        curdir = posix.getcwd()
        posix.mkdir("n0", 0o755)
        posix.chdir("n0")
        try:
            depth = 0
            while len(posix.getcwd()) < 1027:
                posix.mkdir(dirname, 0o755)
                posix.chdir(dirname)
                depth += 1
                self.assertTrue(posix.getcwd().endswith(f"/{dirname}"))
            self.assertGreater(depth, 10)
            for _ in range(depth):
                posix.chdir("..")
                posix.rmdir(dirname)
            self.assertEqual(f"{curdir}/n0", posix.getcwd())
        finally:
            posix.chdir(curdir)

    def test_getcwd_long_path(self) -> None:
        """test_os.MiscTests.test_getcwd_long_path: a working directory over 2000 bytes"""
        min_len = 2000
        dirlen = 200
        dirname = "python_test_dir_"
        dirname = dirname + ("a" * (dirlen - len(dirname)))
        curdir = posix.getcwd()
        posix.mkdir("n0", 0o755)
        posix.chdir("n0")
        try:
            expected = f"{curdir}/n0"
            while True:
                cwd = posix.getcwd()
                self.assertEqual(expected, cwd)
                need = min_len - (len(cwd) + 1)
                if need <= 0:
                    break
                if len(dirname) > need:
                    dirname = dirname[:need]
                path = f"{cwd}/{dirname}"
                posix.mkdir(path, 0o755)
                posix.chdir(path)
                expected = path
            self.assertGreaterEqual(len(posix.getcwd()), min_len)
        finally:
            posix.chdir(curdir)

    def test_getcwdb(self) -> None:
        """test_os.MiscTests.test_getcwdb: getcwdb is the encoded getcwd"""
        posix.mkdir(UNICODE_NAME, 0o755)
        curdir = posix.getcwd()
        posix.chdir(UNICODE_NAME)
        try:
            cwd = posix.getcwdb()
            self.assertIsInstance(cwd, bytes)
            self.assertEqual(posix.getcwd(), cwd.decode())
            self.assertTrue(cwd.endswith(UNICODE_NAME.encode()))
        finally:
            posix.chdir(curdir)

    def _test_link(self, file1: str | bytes, file2: str | bytes) -> None:
        """``LinkTests._test_link``

        EdenFS by design: no hard links. ``link`` fails with EPERM over FUSE
        and with errno 524 (``ENOTSUPP``) over NFS; CPython skips the test on
        PermissionError.
        """
        self.create_file(file1)
        with self.assert_errno(errno.EPERM, 524):
            posix.link(file1, file2)
        with self.assert_errno(errno.ENOENT):
            posix.lstat(file2)
        self.assertEqual(1, posix.stat(file1).st_nlink)

    def test_link(self) -> None:
        """test_os.LinkTests.test_link: a hard link with str paths"""
        self._test_link("n0", "n02")

    def test_link_bytes(self) -> None:
        """test_os.LinkTests.test_link_bytes: a hard link with bytes paths"""
        self._test_link(b"n0", b"n02")

    def test_unicode_name(self) -> None:
        """test_os.LinkTests.test_unicode_name: a hard link with a non-ASCII name"""
        self._test_link("n0\xf1", "n0\xf12")

    def test_not_symlink(self) -> None:
        """test_os.ReadlinkTests.test_not_symlink: readlink of a regular file is EINVAL"""
        self.create_file("n0")
        with self.assert_errno(errno.EINVAL):
            posix.readlink("n0")

    def test_missing_link(self) -> None:
        """test_os.ReadlinkTests.test_missing_link: readlink of a missing path is ENOENT"""
        with self.assert_errno(errno.ENOENT):
            posix.readlink("missing-link")

    def test_bytes(self) -> None:
        """test_os.ReadlinkTests.test_bytes: readlink with a bytes path gives bytes"""
        target = (f"{posix.getcwd()}/{UNICODE_NAME}").encode()
        posix.symlink(target, b"readlinktest")
        path = posix.readlink(b"readlinktest")
        self.assertEqual(target, path)
        self.assertIsInstance(path, bytes)
        self.assertEqual(target.decode(), posix.readlink("readlinktest"))

    def test_directory_link_nonlocal(self) -> None:
        """test_os.NonLocalSymlinkTests.test_directory_link_nonlocal: a relative target
        resolves relative to the link, not to the working directory"""
        posix.mkdir("base", 0o755)
        posix.mkdir("base/some_dir", 0o755)
        posix.symlink("some_dir", "base/some_link")
        self.assertTrue(stat.S_ISDIR(posix.stat("base/some_link").st_mode))
        self.assertEqual(
            posix.stat("base/some_dir").st_ino, posix.stat("base/some_link").st_ino
        )

    def test_symlink_target_not_utf8(self) -> None:
        """EdenFS addition (no CPython test): a symlink target need not be UTF-8

        Only names are checked for UTF-8; the target is stored as given.
        """
        posix.symlink(UNENCODABLE_NAME, "n0")
        self.assertEqual(UNENCODABLE_NAME, posix.readlink(b"n0"))
        with self.assert_errno(self.not_utf8_errno()):
            posix.stat("n0")
