#!/usr/bin/env python3
# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

# pyre-strict
import os
import unittest


class MetricsTest(unittest.TestCase):
    def test_with_metrics_file(self) -> None:
        self.assertTrue(os.environ.get("TEST_METRICS_FILE"))
        metrics_file = os.environ.get("TEST_METRICS_FILE")
        # pyre-fixme[6]: For 1st argument expected `Union[PathLike[bytes],
        #  PathLike[str], bytes, int, str]` but got `Optional[str]`.
        with open(metrics_file, "w") as f:
            f.write("max_memory_kb,1\nnot_existing_metric,2\n")

    def test_with_metrics_file_and_test_name(self) -> None:
        self.assertTrue(os.environ.get("TEST_METRICS_FILE"))
        metrics_file = os.environ.get("TEST_METRICS_FILE")
        # pyre-fixme[6]: For 1st argument expected `Union[PathLike[bytes],
        #  PathLike[str], bytes, int, str]` but got `Optional[str]`.
        with open(metrics_file, "w") as f:
            f.write("max_memory_kb,1,test_with_metrics_file_and_test_name\n")

    def test_without_metrics_file(self) -> None:
        self.assertTrue(os.environ.get("TEST_METRICS_FILE"))

    def test_with_incorrect_metrics_file(self) -> None:
        self.assertTrue(os.environ.get("TEST_METRICS_FILE"))
        metrics_file = os.environ.get("TEST_METRICS_FILE")
        # pyre-fixme[6]: For 1st argument expected `Union[PathLike[bytes],
        #  PathLike[str], bytes, int, str]` but got `Optional[str]`.
        with open(metrics_file, "w") as f:
            f.write("this is not a csv")
