#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib import edenclient, testcase


class HealthTest(testcase.EdenTestCase):
    def test_connected_client_is_healthy(self):
        client = edenclient.EdenClient(self)
        client.daemon_cmd()
        self.assertTrue(client.is_healthy())
        client.shutdown_cmd()
        self.assertFalse(client.is_healthy())

    def test_disconnected_client_is_not_healthy(self):
        client = edenclient.EdenClient(self)
        self.assertFalse(client.is_healthy())
