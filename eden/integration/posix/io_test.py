#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""File descriptor I/O on EdenFS: read, write, the vectored and positioned
variants, copy_file_range, splice and sendfile.

Ported from CPython's ``Lib/test/test_posix.py`` (``PosixTester``) and
``Lib/test/test_os.py`` (``FileTests``, ``TestSendfile``), PSF license,
Python 3.12: one test method per CPython test, named after it, with the
CPython class and test in the docstring, calling the ``posix`` module
directly. ``README.md`` in this directory lists every CPython test with its
method and the checks that were left out.
"""

from __future__ import annotations

import errno
import posix
import socket
import threading

from eden.integration.lib import testcase

from .base import PosixTestBase, SEEK_CUR, SEEK_SET


@testcase.eden_nfs_repo_test
class IoTest(PosixTestBase):
    def test_pread(self) -> None:
        """test_posix.PosixTester.test_pread: pread does not move the offset"""
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT) as fd:
            posix.write(fd, b"test")
            posix.lseek(fd, 0, SEEK_SET)
            self.assertEqual(b"es", posix.pread(fd, 2, 1))
            self.assertEqual(b"te", posix.read(fd, 2))

    def test_preadv(self) -> None:
        """test_posix.PosixTester.test_preadv: preadv scatters into buffers"""
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT) as fd:
            posix.write(fd, b"test1tt2t3t5t6t6t8")
            buf = [bytearray(i) for i in [5, 3, 2]]
            self.assertEqual(10, posix.preadv(fd, buf, 3))
            self.assertEqual([b"t1tt2", b"t3t", b"5t"], [bytes(b) for b in buf])

    def test_preadv_flags(self) -> None:
        """test_posix.PosixTester.test_preadv_flags: preadv2 with RWF_HIPRI"""
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT) as fd:
            posix.write(fd, b"test1tt2t3t5t6t6t8")
            buf = [bytearray(i) for i in [5, 3, 2]]
            self.assertEqual(10, posix.preadv(fd, buf, 3, posix.RWF_HIPRI))
            self.assertEqual([b"t1tt2", b"t3t", b"5t"], [bytes(b) for b in buf])

    def test_pwrite(self) -> None:
        """test_posix.PosixTester.test_pwrite: pwrite does not move the offset"""
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT) as fd:
            posix.write(fd, b"test")
            posix.lseek(fd, 0, SEEK_SET)
            self.assertEqual(2, posix.pwrite(fd, b"xx", 1))
            self.assertEqual(b"txxt", posix.read(fd, 4))

    def test_pwritev(self) -> None:
        """test_posix.PosixTester.test_pwritev: pwritev gathers from buffers"""
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT) as fd:
            posix.write(fd, b"xx")
            posix.lseek(fd, 0, SEEK_SET)
            self.assertEqual(10, posix.pwritev(fd, [b"test1", b"tt2", b"t3"], 2))
            posix.lseek(fd, 0, SEEK_SET)
            self.assertEqual(b"xxtest1tt2t3", posix.read(fd, 100))

    def test_pwritev_flags(self) -> None:
        """test_posix.PosixTester.test_pwritev_flags: pwritev2 with RWF_SYNC

        CPython skips this test on every platform (it tests for the attribute
        ``os.RWF_SYNC`` on ``posix``) and would expect the truncated data
        ``b"xxtest1tt2"``; the port checks what pwritev2 actually writes.
        """
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT) as fd:
            posix.write(fd, b"xx")
            posix.lseek(fd, 0, SEEK_SET)
            self.assertEqual(
                10, posix.pwritev(fd, [b"test1", b"tt2", b"t3"], 2, posix.RWF_SYNC)
            )
            posix.lseek(fd, 0, SEEK_SET)
            self.assertEqual(b"xxtest1tt2t3", posix.read(fd, 100))

    def test_writev(self) -> None:
        """test_posix.PosixTester.test_writev: writev gathers from buffers"""
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT) as fd:
            self.assertEqual(10, posix.writev(fd, (b"test1", b"tt2", b"t3")))
            posix.lseek(fd, 0, SEEK_SET)
            self.assertEqual(b"test1tt2t3", posix.read(fd, 10))
            self.assertEqual(0, posix.writev(fd, []))

    def test_readv(self) -> None:
        """test_posix.PosixTester.test_readv: readv scatters into buffers"""
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT) as fd:
            posix.write(fd, b"test1tt2t3")
            posix.lseek(fd, 0, SEEK_SET)
            buf = [bytearray(i) for i in [5, 3, 2]]
            self.assertEqual(10, posix.readv(fd, buf))
            self.assertEqual([b"test1", b"tt2", b"t3"], [bytes(b) for b in buf])
            self.assertEqual(0, posix.readv(fd, []))

    def test_read(self) -> None:
        """test_os.FileTests.test_read: read returns the bytes written"""
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT) as fd:
            posix.write(fd, b"spam")
            posix.lseek(fd, 0, SEEK_SET)
            data = posix.read(fd, 4)
            self.assertIs(bytes, type(data))
            self.assertEqual(b"spam", data)

    def test_write(self) -> None:
        """test_os.FileTests.test_write: write takes bytes and buffer-like objects"""
        with self.open_fd("n0", posix.O_CREAT | posix.O_WRONLY) as fd:
            posix.write(fd, b"bacon\n")
            posix.write(fd, bytearray(b"eggs\n"))
            posix.write(fd, memoryview(b"spam\n"))
        self.assertEqual(
            [b"bacon", b"eggs", b"spam"], self.read_bytes("n0").splitlines()
        )

    def test_copy_file_range(self) -> None:
        """test_os.FileTests.test_copy_file_range: copy between two files"""
        data = b"0123456789"
        self.create_file("n0", data)
        self.create_file("n1")
        with (
            self.open_fd("n0", posix.O_RDONLY) as in_fd,
            self.open_fd("n1", posix.O_RDWR) as out_fd,
        ):
            copied = posix.copy_file_range(in_fd, out_fd, 5)
        self.assertIn(copied, range(0, 6))
        self.assertEqual(data[:copied], self.read_bytes("n1"))

    def test_copy_file_range_offset(self) -> None:
        """test_os.FileTests.test_copy_file_range_offset: explicit offsets"""
        data = b"0123456789"
        bytes_to_copy = 6
        in_skip = 3
        out_seek = 5
        self.create_file("n0", data)
        self.create_file("n1")
        with (
            self.open_fd("n0", posix.O_RDONLY) as in_fd,
            self.open_fd("n1", posix.O_RDWR) as out_fd,
        ):
            copied = posix.copy_file_range(
                in_fd, out_fd, bytes_to_copy, offset_src=in_skip, offset_dst=out_seek
            )
        self.assertIn(copied, range(0, bytes_to_copy + 1))
        read = self.read_bytes("n1")
        self.assertEqual(b"\x00" * out_seek, read[:out_seek])
        self.assertEqual(data[in_skip : in_skip + copied], read[out_seek:])

    def test_splice(self) -> None:
        """test_os.FileTests.test_splice: splice from a file into a pipe"""
        data = b"0123456789"
        self.create_file("n0", data)
        read_fd, write_fd = posix.pipe()
        self.addCleanup(posix.close, read_fd)
        self.addCleanup(posix.close, write_fd)
        with self.open_fd("n0", posix.O_RDONLY) as in_fd:
            spliced = posix.splice(in_fd, write_fd, 5)
        self.assertIn(spliced, range(0, 6))
        self.assertEqual(data[:spliced], posix.read(read_fd, 100))

    def test_splice_offset_in(self) -> None:
        """test_os.FileTests.test_splice_offset_in: splice with offset_src"""
        data = b"0123456789"
        bytes_to_copy = 6
        in_skip = 3
        self.create_file("n0", data)
        read_fd, write_fd = posix.pipe()
        self.addCleanup(posix.close, read_fd)
        self.addCleanup(posix.close, write_fd)
        with self.open_fd("n0", posix.O_RDONLY) as in_fd:
            spliced = posix.splice(in_fd, write_fd, bytes_to_copy, offset_src=in_skip)
        self.assertIn(spliced, range(0, bytes_to_copy + 1))
        self.assertEqual(data[in_skip : in_skip + spliced], posix.read(read_fd, 100))

    def test_splice_offset_out(self) -> None:
        """test_os.FileTests.test_splice_offset_out: splice with offset_dst"""
        data = b"0123456789"
        bytes_to_copy = 6
        out_seek = 3
        self.create_file("n0", data)
        read_fd, write_fd = posix.pipe()
        self.addCleanup(posix.close, read_fd)
        self.addCleanup(posix.close, write_fd)
        posix.write(write_fd, data)
        self.create_file("n1")
        with self.open_fd("n1", posix.O_RDWR) as out_fd:
            spliced = posix.splice(read_fd, out_fd, bytes_to_copy, offset_dst=out_seek)
        self.assertIn(spliced, range(0, bytes_to_copy + 1))
        read = self.read_bytes("n1")
        self.assertEqual(b"\x00" * out_seek, read[:out_seek])
        self.assertEqual(data[:spliced], read[out_seek:])

    def test_o_append(self) -> None:
        """EdenFS addition (no CPython test): O_APPEND writes at the end after lseek"""
        self.create_file("n0", b"data")
        with self.open_fd("n0", posix.O_WRONLY | posix.O_APPEND) as fd:
            self.assertEqual(1, posix.write(fd, b"X"))
            posix.lseek(fd, 0, SEEK_SET)
            self.assertEqual(1, posix.write(fd, b"Y"))
            self.assertEqual(6, posix.lseek(fd, 0, SEEK_CUR))
        self.assertEqual(b"dataXY", self.read_bytes("n0"))


SENDFILE_DATA: bytes = b"12345abcde" * 16 * 1024  # 160 KiB


@testcase.eden_nfs_repo_test
class SendfileTest(PosixTestBase):
    """``test_os.TestSendfile`` with a Unix socket pair in place of the
    asyncio TCP server: the receiving end is drained by a thread."""

    def setup_eden_test(self) -> None:
        super().setup_eden_test()
        self.create_file("n0", SENDFILE_DATA)
        self.client, self.server = socket.socketpair()
        self.addCleanup(self.client.close)
        self.addCleanup(self.server.close)
        self.sockno = self.client.fileno()
        self.fileno = posix.open("n0", posix.O_RDONLY)
        self.addCleanup(posix.close, self.fileno)
        self.server_buffer = b""
        self.receiver = threading.Thread(target=self._receive)
        self.receiver.start()

    def _receive(self) -> None:
        chunks = []
        while chunk := self.server.recv(65536):
            chunks.append(chunk)
        self.server_buffer = b"".join(chunks)

    def _close_client(self) -> None:
        self.client.shutdown(socket.SHUT_RDWR)
        self.receiver.join()

    def test_send_whole_file(self) -> None:
        """test_os.TestSendfile.test_send_whole_file: sendfile in 4 KiB pieces"""
        total_sent = 0
        offset = 0
        nbytes = 4096
        while total_sent < len(SENDFILE_DATA):
            sent = posix.sendfile(self.sockno, self.fileno, offset, nbytes)
            if sent == 0:
                break
            offset += sent
            total_sent += sent
            self.assertLessEqual(sent, nbytes)
            self.assertEqual(offset, total_sent)
        self.assertEqual(len(SENDFILE_DATA), total_sent)
        self._close_client()
        self.assertEqual(len(SENDFILE_DATA), len(self.server_buffer))
        self.assertEqual(SENDFILE_DATA, self.server_buffer)

    def test_send_at_certain_offset(self) -> None:
        """test_os.TestSendfile.test_send_at_certain_offset: sendfile from the middle"""
        total_sent = 0
        offset = len(SENDFILE_DATA) // 2
        must_send = len(SENDFILE_DATA) - offset
        nbytes = 4096
        while total_sent < must_send:
            sent = posix.sendfile(self.sockno, self.fileno, offset, nbytes)
            if sent == 0:
                break
            offset += sent
            total_sent += sent
            self.assertLessEqual(sent, nbytes)
        self._close_client()
        expected = SENDFILE_DATA[len(SENDFILE_DATA) // 2 :]
        self.assertEqual(len(expected), total_sent)
        self.assertEqual(len(expected), len(self.server_buffer))
        self.assertEqual(expected, self.server_buffer)

    def test_offset_overflow(self) -> None:
        """test_os.TestSendfile.test_offset_overflow: an offset past EOF sends nothing"""
        offset = len(SENDFILE_DATA) + 4096
        self.assertEqual(0, posix.sendfile(self.sockno, self.fileno, offset, 4096))
        self._close_client()
        self.assertEqual(b"", self.server_buffer)

    def test_invalid_offset(self) -> None:
        """test_os.TestSendfile.test_invalid_offset: a negative offset is EINVAL"""
        with self.assert_errno(errno.EINVAL):
            posix.sendfile(self.sockno, self.fileno, -1, 4096)
        self._close_client()
