#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib import edenclient, testcase


class HealthTest(testcase.EdenTestCase):
    def test_is_healthy(self) -> None:
        self.assertTrue(self.eden.is_healthy())
        self.eden.shutdown()
        self.assertFalse(self.eden.is_healthy())

    def test_disconnected_daemon_is_not_healthy(self) -> None:
        # Create a new edenfs instance that is never started, and make sure
        # it is not healthy.
        with edenclient.EdenFS() as client:
            self.assertFalse(client.is_healthy())
