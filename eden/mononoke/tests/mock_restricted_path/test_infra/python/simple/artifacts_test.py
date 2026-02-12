#!/usr/bin/env python3
# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

# pyre-strict
import os
import unittest


class ArtifactsTest(unittest.TestCase):
    def test_artifact_generation(self) -> None:
        artifacts_dir = os.environ.get("TEST_RESULT_ARTIFACTS_DIR")
        self.assertTrue(artifacts_dir is not None)
        if not os.path.exists(artifacts_dir):
            os.makedirs(artifacts_dir)

        annotations_dir = os.environ.get("TEST_RESULT_ARTIFACT_ANNOTATIONS_DIR")
        self.assertTrue(annotations_dir is not None)
        if not os.path.exists(annotations_dir):
            os.makedirs(annotations_dir)

        with open(os.path.join(artifacts_dir, "dummy_blob.txt"), "w") as f:
            f.write("Hello from dummy blob!\n")
