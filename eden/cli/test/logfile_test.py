#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import io
import os
import pathlib
import subprocess
import typing
import unittest

from eden.cli.logfile import follow_log_file, forward_log_file
from eden.test_support.temporary_directory import TemporaryDirectoryMixin


class _LogFileTestBase(unittest.TestCase, TemporaryDirectoryMixin):
    def make_empty_file(self) -> pathlib.Path:
        temp_dir = self.make_temporary_directory()
        path = pathlib.Path(temp_dir) / "file.txt"
        path.write_bytes(b"")
        return path


class LogForwarderTest(_LogFileTestBase):
    def test_forward_does_not_automatically_copy_existing_file_content(self) -> None:
        path = self.make_empty_file()
        path.write_bytes(b"hello world")
        output = io.BytesIO()
        with forward_log_file(path, output_file=output):
            self.assertEqual(output.getvalue(), b"")

    def test_forward_copies_existing_file_content_after_poll(self) -> None:
        path = self.make_empty_file()
        path.write_bytes(b"hello world")
        output = io.BytesIO()
        with forward_log_file(path, output_file=output) as forwarder:
            forwarder.poll()
            self.assertEqual(output.getvalue(), b"hello world")

    def test_forward_does_not_automatically_copy_concurrent_appends(self) -> None:
        path = self.make_empty_file()
        output = io.BytesIO()
        with forward_log_file(path, output_file=output):
            self.assertEqual(output.getvalue(), b"")
            with open(path, "ab") as file:
                file.write(b"hello")
                file.flush()
                self.assertEqual(output.getvalue(), b"")

    def test_polling_forward_copies_concurrent_appends(self) -> None:
        path = self.make_empty_file()
        output = io.BytesIO()
        with forward_log_file(path, output_file=output) as forwarder:
            self.assertEqual(output.getvalue(), b"")
            with open(path, "ab") as file:
                file.write(b"hello")
                file.flush()
                forwarder.poll()
                self.assertEqual(output.getvalue(), b"hello")

                file.write(b"world")
                file.flush()
                forwarder.poll()
                self.assertEqual(output.getvalue(), b"helloworld")

    def test_polling_forward_flushes_output_buffer(self) -> None:
        path = self.make_empty_file()
        unbuffered_output = io.BytesIO()
        # HACK(strager): BufferedWriter requires RawIOBase, but BytesIO inherits
        # from IOBase, not RawIOBase. BufferedWriter seems to work fine with
        # BytesIO though, and CPython's test suite even covers this use case:
        # https://github.com/python/cpython/blob/v3.6.5/Lib/test/test_io.py#L1648-L1650
        raw_output = typing.cast(io.RawIOBase, unbuffered_output)
        buffered_output = io.BufferedWriter(raw_output, buffer_size=1024)
        with forward_log_file(path, output_file=buffered_output) as forwarder:
            with open(path, "ab") as file:
                file.write(b"hello")
                file.flush()
                forwarder.poll()
                self.assertEqual(unbuffered_output.getvalue(), b"hello")


class LogFollowerTest(_LogFileTestBase):
    def test_empty_file_yields_no_updates(self) -> None:
        path = self.make_empty_file()
        with follow_log_file(path) as follower:
            self.assertEqual(follower.poll(), b"")
            self.assertEqual(follower.poll(), b"")

    def test_nonempty_file_yields_one_update(self) -> None:
        path = self.make_empty_file()
        path.write_bytes(b"hello world")
        with follow_log_file(path) as follower:
            self.assertEqual(follower.poll(), b"hello world")
            self.assertEqual(follower.poll(), b"")

    def test_large_file_yields_one_update(self) -> None:
        file_size = 1024 * 1024
        path = self.make_empty_file()

        data = bytearray(file_size)
        data[0] = 100
        data[file_size // 2] = 101
        data[-1] = 102

        path.write_bytes(bytes(data))
        with follow_log_file(path) as follower:
            self.assertEqual(follower.poll(), data)
            self.assertEqual(follower.poll(), b"")

    def test_file_yields_updates_after_concurrent_appends(self) -> None:
        path = self.make_empty_file()
        with follow_log_file(path) as follower:
            self.assertEqual(follower.poll(), b"")

            with open(path, "ab") as file:
                file.write(b"hello")
                file.flush()
                self.assertEqual(follower.poll(), b"hello")

                file.write(b"world")
                file.flush()
                self.assertEqual(follower.poll(), b"world")

    def test_output_from_a_separate_process_is_visible_after_it_exits(self) -> None:
        path = self.make_empty_file()
        with follow_log_file(path) as follower:
            env = dict(os.environ)
            env["file"] = str(path)
            subprocess.check_call(["sh", "-c", 'echo hello >>"${file}"'], env=env)

            self.assertEqual(follower.poll(), b"hello\n")
