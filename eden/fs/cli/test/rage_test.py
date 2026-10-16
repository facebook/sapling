#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import io
import string
import unittest
from pathlib import Path
from typing import BinaryIO

from eden.fs.cli.rage import IOWithRedaction, print_log_file


class LogRedactionTest(unittest.TestCase):
    def test_token_across_a_read_boundary(self) -> None:
        token = "ghp_" + (string.digits + string.ascii_lowercase)[:36]
        read_size = 20 * 1024
        body = ("." * (read_size - 10) + token + "." * 100 + "\n").encode()

        def open_log(path: Path, mode: str) -> BinaryIO:
            return io.BytesIO(body)

        sink = io.BytesIO()
        print_log_file(
            Path("edenfs.log"), IOWithRedaction(sink), open_fn=open_log, tail_limit=None
        )

        written = sink.getvalue().decode()
        self.assertIn(token, written)
        self.assertNotIn("<ACCESS_TOKEN_REDACTED>", written)

    def test_token_inside_one_read(self) -> None:
        token = "ghp_" + (string.digits + string.ascii_lowercase)[:36]
        body = ("." * 100 + token + "." * 100 + "\n").encode()

        def open_log(path: Path, mode: str) -> BinaryIO:
            return io.BytesIO(body)

        sink = io.BytesIO()
        print_log_file(
            Path("edenfs.log"), IOWithRedaction(sink), open_fn=open_log, tail_limit=None
        )

        written = sink.getvalue().decode()
        self.assertNotIn(token, written)
        self.assertIn("<ACCESS_TOKEN_REDACTED>", written)
