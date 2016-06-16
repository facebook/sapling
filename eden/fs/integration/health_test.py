# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from __future__ import absolute_import
from __future__ import division
from __future__ import print_function
from __future__ import unicode_literals
from eden.fs.integration import edenclient, testcase


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
