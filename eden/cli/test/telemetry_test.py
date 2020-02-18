#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import math
import unittest

from eden.cli.config import EdenInstance

from ..telemetry import build_base_sample


class TelemetryTest(unittest.TestCase):
    def test_base_log_data(self) -> None:
        sample = build_base_sample("test")
        sample_json = json.loads(sample.get_json())
        self.assertIn("session_id", sample_json["int"])
        self.assertIn("type", sample_json["normal"])
        self.assertIn("user", sample_json["normal"])
        self.assertIn("host", sample_json["normal"])
        self.assertIn("os", sample_json["normal"])
        self.assertIn("osver", sample_json["normal"])
        self.assertIn("edenver", sample_json["normal"])

    def test_build_complex_sample(self) -> None:
        instance = EdenInstance(
            config_dir="/home/johndoe/.eden",
            etc_eden_dir="/etc/eden",
            home_dir="/home/johndoe",
        )
        sample = instance.build_sample("testing", testing=True, cost=12.99)
        sample_json = json.loads(sample.get_json())
        self.assertEqual(1, sample_json["int"]["testing"])
        self.assertTrue(math.isclose(sample_json["double"]["cost"], 12.99))
        self.assertIn("session_id", sample_json["int"])
        self.assertIn("type", sample_json["normal"])
        self.assertIn("user", sample_json["normal"])
        self.assertIn("host", sample_json["normal"])
        self.assertIn("os", sample_json["normal"])
        self.assertIn("osver", sample_json["normal"])
        self.assertIn("edenver", sample_json["normal"])
