# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

from eden.mononoke.tests.mock_restricted_path.test_infra.python.pytest.test_lib import (
    method,
)


def test_coverage():
    assert method() == 123
