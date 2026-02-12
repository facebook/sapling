# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

import pytest


@pytest.fixture(scope="module")
def my_fixture():
    return 123
