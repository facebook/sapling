#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Extended attributes on EdenFS: setxattr, getxattr, listxattr and
removexattr through a path, a symlink and a descriptor.

Ported from CPython's ``Lib/test/test_os.py`` (``ExtendedAttributeTests``),
PSF license, Python 3.12: one test method per CPython test, named after it,
with the CPython class and test in the docstring, calling the ``posix``
module directly. CPython skips the whole class when ``setxattr`` fails
("no non-broken extended attribute support"); the port pins what EdenFS
does instead. ``README.md`` in this directory lists every CPython test with
its method and the checks that were left out.

EdenFS by design: no user xattrs can be stored. Over FUSE, ``setxattr`` and
``removexattr`` fail with EOPNOTSUPP, ``listxattr`` is empty, and
``getxattr`` answers only ``user.sha1`` and ``user.blake3`` on regular files
(the content hashes EdenFS keeps); anything else is ENODATA. Over NFS (v3,
no xattr protocol) ``getxattr``, ``setxattr`` and ``removexattr`` fail with
EOPNOTSUPP in the client and ``listxattr`` is empty.
"""

from __future__ import annotations

import errno
import hashlib
import posix
from typing import Any, Callable

from eden.integration.lib import testcase

from .base import PosixTestBase


def sha1_hex(data: bytes) -> bytes:
    """What ``user.sha1`` is defined to be: the hex SHA-1 of the content."""
    # patternlint-disable-next-line poor-choice-of-hash-function
    return hashlib.sha1(data).hexdigest().encode()


@testcase.eden_nfs_repo_test
class XattrTest(PosixTestBase):
    def _check_xattrs_str(
        self,
        s: Callable[[str], str | bytes],
        getxattr: Callable[..., bytes],
        setxattr: Callable[..., None],
        removexattr: Callable[..., None],
        listxattr: Callable[..., list[str]],
        **kwargs: bool,
    ) -> None:
        """``ExtendedAttributeTests._check_xattrs_str``, pinned to EdenFS.

        CPython expects ENODATA for an unset attribute, then sets, replaces,
        lists and removes ``user.test*``; EdenFS refuses every store.
        """
        fn = "n0"
        self.create_file(fn)
        if self.use_nfs():
            with self.assert_errno(errno.EOPNOTSUPP):
                getxattr(fn, s("user.test"), **kwargs)
            self.assertEqual([], listxattr(fn, **kwargs))
            with self.assert_errno(errno.EOPNOTSUPP):
                setxattr(fn, s("user.test"), b"", **kwargs)
            with self.assert_errno(errno.EOPNOTSUPP):
                removexattr(fn, s("user.test"), **kwargs)
            return
        with self.assert_errno(errno.ENODATA):
            getxattr(fn, s("user.test"), **kwargs)
        init_xattr = listxattr(fn, **kwargs)
        self.assertIsInstance(init_xattr, list)
        self.assertEqual([], init_xattr)
        with self.assert_errno(errno.EOPNOTSUPP):
            setxattr(fn, s("user.test"), b"", **kwargs)
        with self.assert_errno(errno.EOPNOTSUPP):
            setxattr(fn, s("user.test"), b"hello", posix.XATTR_REPLACE, **kwargs)
        with self.assert_errno(errno.EOPNOTSUPP):
            setxattr(fn, s("user.test2"), b"foo", posix.XATTR_CREATE, **kwargs)
        self.assertEqual([], listxattr(fn, **kwargs))
        with self.assert_errno(errno.ENODATA):
            getxattr(fn, s("user.test"), **kwargs)
        with self.assert_errno(errno.EOPNOTSUPP):
            removexattr(fn, s("user.test"), **kwargs)

    def _check_xattrs(
        self,
        getxattr: Callable[..., bytes],
        setxattr: Callable[..., None],
        removexattr: Callable[..., None],
        listxattr: Callable[..., list[str]],
        **kwargs: bool,
    ) -> None:
        for s in (str, str.encode):
            self._check_xattrs_str(
                s, getxattr, setxattr, removexattr, listxattr, **kwargs
            )
            posix.unlink("n0")

    def test_simple(self) -> None:
        """test_os.ExtendedAttributeTests.test_simple: xattrs through a path"""
        self._check_xattrs(
            posix.getxattr, posix.setxattr, posix.removexattr, posix.listxattr
        )

    def test_lpath(self) -> None:
        """test_os.ExtendedAttributeTests.test_lpath: xattrs with follow_symlinks=False"""
        self._check_xattrs(
            posix.getxattr,
            posix.setxattr,
            posix.removexattr,
            posix.listxattr,
            follow_symlinks=False,
        )

    def test_fds(self) -> None:
        """test_os.ExtendedAttributeTests.test_fds: xattrs through a descriptor"""

        def getxattr(path: str, *args: Any) -> bytes:
            with self.open_fd(path, posix.O_RDONLY) as fd:
                return posix.getxattr(fd, *args)

        def setxattr(path: str, *args: Any) -> None:
            with self.open_fd(path, posix.O_WRONLY) as fd:
                posix.setxattr(fd, *args)

        def removexattr(path: str, *args: Any) -> None:
            with self.open_fd(path, posix.O_WRONLY) as fd:
                posix.removexattr(fd, *args)

        def listxattr(path: str, *args: Any) -> list[str]:
            with self.open_fd(path, posix.O_RDONLY) as fd:
                return posix.listxattr(fd, *args)

        self._check_xattrs(getxattr, setxattr, removexattr, listxattr)

    def test_getxattr_user_sha1(self) -> None:
        """EdenFS addition (no CPython test): user.sha1 is the SHA-1 of the content

        Pins the contract: readable through a path, a followed symlink and a
        descriptor; ENODATA on a directory and on the symlink itself; not
        listed by listxattr. Over NFS there are no xattrs.
        """
        data = b"hello\n"
        self.create_file("n0", data)
        posix.mkdir("n1", 0o755)
        posix.symlink("n0", "n2")
        if self.use_nfs():
            with self.assert_errno(errno.EOPNOTSUPP):
                posix.getxattr("n0", "user.sha1")
            return
        expected = sha1_hex(data)
        self.assertEqual(expected, posix.getxattr("n0", "user.sha1"))
        self.assertEqual(expected, posix.getxattr(b"n0", b"user.sha1"))
        self.assertEqual(expected, posix.getxattr("n2", "user.sha1"))
        with self.open_fd("n0", posix.O_RDONLY) as fd:
            self.assertEqual(expected, posix.getxattr(fd, "user.sha1"))
        with self.assert_errno(errno.ENODATA):
            posix.getxattr("n1", "user.sha1")
        with self.assert_errno(errno.ENODATA):
            posix.getxattr("n2", "user.sha1", follow_symlinks=False)
        self.assertEqual([], posix.listxattr("n0"))
        self.create_file("n3")
        self.assertEqual(sha1_hex(b""), posix.getxattr("n3", "user.sha1"))
        with self.open_fd("n0", posix.O_WRONLY | posix.O_APPEND) as fd:
            posix.write(fd, b"world\n")
        self.assertEqual(sha1_hex(data + b"world\n"), posix.getxattr("n0", "user.sha1"))

    def test_getxattr_user_blake3(self) -> None:
        """EdenFS addition (no CPython test): user.blake3 is a 64-digit hex hash

        hashlib has no BLAKE3 (and EdenFS may key it), so the pin is the
        shape, stability and sensitivity to the content.
        """
        self.create_file("n0", b"hello\n")
        self.create_file("n1", b"hello\n")
        self.create_file("n2", b"other\n")
        if self.use_nfs():
            with self.assert_errno(errno.EOPNOTSUPP):
                posix.getxattr("n0", "user.blake3")
            return
        value = posix.getxattr("n0", "user.blake3")
        self.assertEqual(64, len(value))
        int(value, 16)
        self.assertEqual(value, posix.getxattr("n0", "user.blake3"))
        self.assertEqual(value, posix.getxattr("n1", "user.blake3"))
        self.assertNotEqual(value, posix.getxattr("n2", "user.blake3"))
        with self.open_fd("n0", posix.O_RDONLY) as fd:
            self.assertEqual(value, posix.getxattr(fd, "user.blake3"))
