#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import unittest
from contextlib import contextmanager
from typing import Any, Iterator
from unittest.mock import patch

from eden.thrift import client as thrift_client
from thrift.python.exceptions import TransportError, TransportErrorType


class FakeEdenClient:
    pass


@contextmanager
def connected_client(*args: Any, **kwargs: Any) -> Iterator[FakeEdenClient]:
    yield FakeEdenClient()


@contextmanager
def refused_connection(*args: Any, **kwargs: Any) -> Iterator[FakeEdenClient]:
    raise TransportError(TransportErrorType.NOT_OPEN, "connection refused", 111, 0)
    yield FakeEdenClient()


class CreateThriftClientTest(unittest.TestCase):
    def test_transport_error_while_connecting(self) -> None:
        with patch.object(thrift_client, "get_sync_client", refused_connection):
            with self.assertRaises(thrift_client.EdenNotRunningError):
                with thrift_client.create_thrift_client(socket_path="/tmp/eden.sock"):
                    pass

    def test_transport_error_raised_inside_the_body(self) -> None:
        with patch.object(thrift_client, "get_sync_client", connected_client):
            with self.assertRaises(thrift_client.EdenNotRunningError):
                with thrift_client.create_thrift_client(socket_path="/tmp/eden.sock"):
                    raise TransportError(
                        TransportErrorType.TIMED_OUT, "request timed out", 0, 0
                    )
