#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""File-backed mmap on EdenFS: shared and private mappings, write-back,
resize, offsets, and maps that outlive the descriptor.

Ported from CPython's ``Lib/test/test_mmap.py`` (``MmapTests``), PSF
license, Python 3.12: one test method per CPython test, named after it, with
the CPython class and test in the docstring, calling the ``posix`` and
``mmap`` modules directly. Only the tests that map a file are ported; the
anonymous-mapping and Python-API tests (``find``, ``repr``, ``madvise``,
``subclass``) do not touch the file system. The zero fill of the page past
the end of the file is tested in ``eden/integration/mmap_test.py``.
"""

from __future__ import annotations

import mmap
import posix
import re

from eden.integration.lib import testcase

from .base import PosixTestBase


PAGESIZE: int = mmap.PAGESIZE


@testcase.eden_nfs_repo_test
class MmapTest(PosixTestBase):
    def test_basic(self) -> None:
        """test_mmap.MmapTests.test_basic: find, slice, write, flush, seek and resize"""
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT | posix.O_TRUNC) as fd:
            posix.write(fd, b"\0" * PAGESIZE)
            posix.write(fd, b"foo")
            posix.write(fd, b"\0" * (PAGESIZE - 3))
            m = mmap.mmap(fd, 2 * PAGESIZE)
        self.addCleanup(m.close)
        self.assertEqual(PAGESIZE, m.find(b"foo"))
        self.assertEqual(2 * PAGESIZE, len(m))
        self.assertEqual(0, m[0])
        self.assertEqual(b"\0\0\0", m[0:3])
        m[0] = b"3"[0]
        m[PAGESIZE + 3 : PAGESIZE + 3 + 3] = b"bar"
        self.assertEqual(b"3"[0], m[0])
        self.assertEqual(b"3\0\0", m[0:3])
        self.assertEqual(b"\0foobar\0", m[PAGESIZE - 1 : PAGESIZE + 7])
        m.flush()
        match = re.search(b"[A-Za-z]+", m)
        self.assertIsNotNone(match)
        assert match is not None
        self.assertEqual((PAGESIZE, PAGESIZE + 6), match.span(0))
        m.seek(0, 0)
        self.assertEqual(0, m.tell())
        m.seek(42, 1)
        self.assertEqual(42, m.tell())
        m.seek(0, 2)
        self.assertEqual(len(m), m.tell())
        m.resize(512)
        self.assertEqual(512, len(m))
        self.assertEqual(512, posix.stat("n0").st_size)
        self.assertEqual(512, m.size())
        self.assertEqual(b"3\0\0", self.read_bytes("n0")[:3])

    def test_access_parameter(self) -> None:
        """test_mmap.MmapTests.test_access_parameter: ACCESS_READ, ACCESS_WRITE, ACCESS_COPY"""
        mapsize = 10
        self.create_file("n0", b"a" * mapsize)
        with self.open_fd("n0", posix.O_RDONLY) as fd:
            m = mmap.mmap(fd, mapsize, access=mmap.ACCESS_READ)
            self.assertEqual(b"a" * mapsize, m[:])
            with self.assertRaises(TypeError):
                m[:] = b"b" * mapsize
            with self.assertRaises(TypeError):
                m[0] = b"b"[0]
            m.seek(0, 0)
            with self.assertRaises(TypeError):
                m.write(b"abc")
            m.seek(0, 0)
            with self.assertRaises(TypeError):
                m.write_byte(b"d"[0])
            with self.assertRaises(TypeError):
                m.resize(2 * mapsize)
            m.close()
            self.assertEqual(b"a" * mapsize, self.read_bytes("n0"))
        with self.open_fd("n0", posix.O_RDWR) as fd:
            with self.assertRaises(ValueError):
                mmap.mmap(fd, mapsize + 1)
        with self.open_fd("n0", posix.O_RDWR) as fd:
            m = mmap.mmap(fd, mapsize, access=mmap.ACCESS_WRITE)
            m[:] = b"c" * mapsize
            self.assertEqual(b"c" * mapsize, m[:])
            m.flush()
            m.close()
        self.assertEqual(b"c" * mapsize, self.read_bytes("n0"))
        with self.open_fd("n0", posix.O_RDWR) as fd:
            m = mmap.mmap(fd, mapsize, access=mmap.ACCESS_COPY)
            m[:] = b"d" * mapsize
            self.assertEqual(b"d" * mapsize, m[:])
            m.flush()
            self.assertEqual(b"c" * mapsize, self.read_bytes("n0"))
            with self.assertRaises(TypeError):
                m.resize(2 * mapsize)
            m.close()
        with self.open_fd("n0", posix.O_RDWR) as fd:
            with self.assertRaises(ValueError):
                mmap.mmap(fd, mapsize, access=4)
            with self.assertRaises(ValueError):
                mmap.mmap(
                    fd,
                    mapsize,
                    flags=mmap.MAP_PRIVATE,
                    prot=mmap.PROT_READ,
                    access=mmap.ACCESS_WRITE,
                )
            m = mmap.mmap(fd, mapsize, prot=mmap.PROT_READ | mmap.PROT_EXEC)
            with self.assertRaises(TypeError):
                m.write(b"abcdef")
            with self.assertRaises(TypeError):
                m.write_byte(0)
            m.close()

    def test_tougher_find(self) -> None:
        """test_mmap.MmapTests.test_tougher_find: the map outlives the descriptor"""
        data = b"aabaac\x00deef\x00\x00aa\x00"
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT | posix.O_TRUNC) as fd:
            posix.write(fd, data)
            m = mmap.mmap(fd, len(data))
        self.addCleanup(m.close)
        n = len(data)
        for start in range(n + 1):
            for finish in range(start, n + 1):
                piece = data[start:finish]
                self.assertEqual(data.find(piece), m.find(piece))
                self.assertEqual(-1, m.find(piece + b"x"))

    def test_double_close(self) -> None:
        """test_mmap.MmapTests.test_double_close: closing a read-only map twice"""
        self.create_file("n0", 2**16 * b"a")
        with self.open_fd("n0", posix.O_RDONLY) as fd:
            m = mmap.mmap(fd, 2**16, access=mmap.ACCESS_READ)
            m.close()
            m.close()

    def test_entire_file(self) -> None:
        """test_mmap.MmapTests.test_entire_file: length 0 maps the whole file"""
        self.create_file("n0", 2**16 * b"m")
        with self.open_fd("n0", posix.O_RDWR) as fd, mmap.mmap(fd, 0) as m:
            self.assertEqual(2**16, len(m))
            self.assertEqual(2**16 * b"m", m.read(2**16))

    def test_length_0_offset(self) -> None:
        """test_mmap.MmapTests.test_length_0_offset: length 0 with an offset maps the rest"""
        self.create_file("n0", (65536 * 2) * b"m")
        with (
            self.open_fd("n0", posix.O_RDONLY) as fd,
            mmap.mmap(fd, 0, offset=65536, access=mmap.ACCESS_READ) as m,
        ):
            self.assertEqual(65536, len(m))
            with self.assertRaises(IndexError):
                m[80000]

    def test_move(self) -> None:
        """test_mmap.MmapTests.test_move: move within a file-backed map

        The anonymous-mapping part of the CPython test is left out.
        """
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT | posix.O_TRUNC) as fd:
            posix.write(fd, b"ABCDEabcde")
            m = mmap.mmap(fd, 10)
            m.move(5, 0, 5)
            self.assertEqual(b"ABCDEABCDE", m[:])
            m.close()
        self.assertEqual(b"ABCDEABCDE", self.read_bytes("n0"))

    def test_empty_file(self) -> None:
        """test_mmap.MmapTests.test_empty_file: an empty file cannot be mapped"""
        self.create_file("n0")
        with self.open_fd("n0", posix.O_RDONLY) as fd:
            with self.assertRaisesRegex(ValueError, "cannot mmap an empty file"):
                mmap.mmap(fd, 0, access=mmap.ACCESS_READ)

    def test_offset(self) -> None:
        """test_mmap.MmapTests.test_offset: a map at an offset, then resized"""
        halfsize = mmap.ALLOCATIONGRANULARITY
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT | posix.O_TRUNC) as fd:
            posix.write(fd, b"\0" * halfsize)
            posix.write(fd, b"foo")
            posix.write(fd, b"\0" * (halfsize - 3))
            m = mmap.mmap(fd, 0)
            m.close()
        mapsize = halfsize * 2
        with self.open_fd("n0", posix.O_RDWR) as fd:
            for offset in (-2, -1):
                with self.assertRaises((ValueError, TypeError, OverflowError)):
                    mmap.mmap(fd, mapsize, offset=offset)
        with self.open_fd("n0", posix.O_RDWR) as fd:
            m = mmap.mmap(fd, mapsize - halfsize, offset=halfsize)
        self.addCleanup(m.close)
        self.assertEqual(b"foo", m[0:3])
        m.resize(512)
        self.assertEqual(512, len(m))
        with self.assertRaises(ValueError):
            m.seek(513, 0)
        self.assertEqual(b"foo", m[0:3])
        self.assertEqual(halfsize + 512, posix.stat("n0").st_size)
        self.assertEqual(halfsize + 512, m.size())

    def test_prot_readonly(self) -> None:
        """test_mmap.MmapTests.test_prot_readonly: PROT_READ rejects writes"""
        mapsize = 10
        self.create_file("n0", b"a" * mapsize)
        with self.open_fd("n0", posix.O_RDONLY) as fd:
            m = mmap.mmap(fd, mapsize, prot=mmap.PROT_READ)
            self.addCleanup(m.close)
            with self.assertRaises(TypeError):
                m.write(b"foo")

    def test_io_methods(self) -> None:
        """test_mmap.MmapTests.test_io_methods: write_byte, read_byte, read and write"""
        data = b"0123456789"
        self.create_file("n0", b"x" * len(data))
        with self.open_fd("n0", posix.O_RDWR) as fd:
            m = mmap.mmap(fd, len(data))
        self.addCleanup(m.close)
        for i in range(len(data)):
            self.assertEqual(i, m.tell())
            m.write_byte(data[i])
            self.assertEqual(i + 1, m.tell())
        with self.assertRaises(ValueError):
            m.write_byte(b"x"[0])
        self.assertEqual(data, m[:])
        m.seek(0)
        for i in range(len(data)):
            self.assertEqual(i, m.tell())
            self.assertEqual(data[i], m.read_byte())
            self.assertEqual(i + 1, m.tell())
        with self.assertRaises(ValueError):
            m.read_byte()
        m.seek(3)
        self.assertEqual(b"345", m.read(3))
        self.assertEqual(6, m.tell())
        m.seek(3)
        m.write(b"bar")
        self.assertEqual(6, m.tell())
        self.assertEqual(b"012bar6789", m[:])
        m.write(bytearray(b"baz"))
        self.assertEqual(9, m.tell())
        self.assertEqual(b"012barbaz9", m[:])
        with self.assertRaises(ValueError):
            m.write(b"ba")
        m.flush()
        self.assertEqual(b"012barbaz9", self.read_bytes("n0"))

    def test_map_shared_write_back(self) -> None:
        """EdenFS addition (no CPython test): a MAP_SHARED write reaches other readers"""
        self.create_file("n0", b"\0" * PAGESIZE)
        with self.open_fd("n0", posix.O_RDWR) as fd:
            m = mmap.mmap(fd, PAGESIZE, flags=mmap.MAP_SHARED)
            self.addCleanup(m.close)
            m[0:3] = b"abc"
            m.flush()
            with self.open_fd("n0", posix.O_RDONLY) as fd2:
                self.assertEqual(b"abc", posix.read(fd2, 3))
            posix.pwrite(fd, b"xyz", 3)
            self.assertEqual(b"abcxyz", m[0:6])

    def test_map_private(self) -> None:
        """EdenFS addition (no CPython test): a MAP_PRIVATE write stays private"""
        self.create_file("n0", b"\0" * PAGESIZE)
        with self.open_fd("n0", posix.O_RDWR) as fd:
            m = mmap.mmap(fd, PAGESIZE, flags=mmap.MAP_PRIVATE)
            self.addCleanup(m.close)
            m[0:3] = b"abc"
            m.flush()
        self.assertEqual(b"\0\0\0", self.read_bytes("n0")[:3])
