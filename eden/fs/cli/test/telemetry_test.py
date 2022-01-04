#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import json
import math
import typing
import unittest

from ..telemetry import ExternalTelemetryLogger, JsonTelemetrySample


class TelemetryTest(unittest.TestCase):
    def test_base_log_data(self) -> None:
        logger = ExternalTelemetryLogger(["/bin/echo"])
        sample = logger.new_sample("test")
        sample_json = json.loads(typing.cast(JsonTelemetrySample, sample).get_json())
        self.assertIn("session_id", sample_json["int"])
        self.assertIn("type", sample_json["normal"])
        self.assertIn("user", sample_json["normal"])
        self.assertIn("host", sample_json["normal"])
        self.assertIn("os", sample_json["normal"])
        self.assertIn("osver", sample_json["normal"])
        self.assertIn("edenver", sample_json["normal"])

    def test_build_complex_sample_tags(self) -> None:
        logger = ExternalTelemetryLogger(["/bin/echo"])
        sample = logger.new_sample("testing")
        problems = {"hg_error", "version_error", "error"}
        sample.add_fields(testing=True, cost=12.99, problems=problems)
        sample_json = json.loads(typing.cast(JsonTelemetrySample, sample).get_json())
        self.assertEqual(1, sample_json["int"]["testing"])
        self.assertTrue(math.isclose(sample_json["double"]["cost"], 12.99))
        self.assertIn("session_id", sample_json["int"])
        self.assertIn("type", sample_json["normal"])
        self.assertIn("user", sample_json["normal"])
        self.assertIn("host", sample_json["normal"])
        self.assertIn("os", sample_json["normal"])
        self.assertIn("osver", sample_json["normal"])
        self.assertIn("edenver", sample_json["normal"])
        self.assertIn("problems", sample_json["tags"])
        self.assertIn("hg_error", sample_json["tags"]["problems"])
        self.assertIn("version_error", sample_json["tags"]["problems"])
        self.assertIn("error", sample_json["tags"]["problems"])

    def test_build_complex_sample_normvector(self) -> None:
        logger = ExternalTelemetryLogger(["/bin/echo"])
        sample = logger.new_sample("testing")
        patterns = ["**/TARGETS", "eden/fs/cli/*", "test"]
        sample.add_fields(testing=True, cost=12.99, patterns=patterns)
        sample_json = json.loads(typing.cast(JsonTelemetrySample, sample).get_json())
        self.assertEqual(1, sample_json["int"]["testing"])
        self.assertTrue(math.isclose(sample_json["double"]["cost"], 12.99))
        self.assertIn("session_id", sample_json["int"])
        self.assertIn("type", sample_json["normal"])
        self.assertIn("user", sample_json["normal"])
        self.assertIn("host", sample_json["normal"])
        self.assertIn("os", sample_json["normal"])
        self.assertIn("osver", sample_json["normal"])
        self.assertIn("edenver", sample_json["normal"])
        self.assertIn("patterns", sample_json["normvector"])
        self.assertIn("**/TARGETS", sample_json["normvector"]["patterns"])
        self.assertIn("eden/fs/cli/*", sample_json["normvector"]["patterns"])
        self.assertIn("test", sample_json["normvector"]["patterns"])
