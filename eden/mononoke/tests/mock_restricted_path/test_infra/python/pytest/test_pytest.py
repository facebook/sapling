#!/usr/bin/env python3
# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

import os

import pytest
from eden.mononoke.tests.mock_restricted_path.test_infra.python.simple.simple import add

TEST_DATA = [("the answer", 21, 21, 42), ("simple", 1, 1, 2)]


@pytest.mark.parametrize("name,a,b,res", TEST_DATA)
def test_with_params(name, a, b, res):
    assert add(a, b) == res


def test_playground():
    assert add(21, 21) == 42


def test_playground2():
    assert 42 == 42


class TestClass:
    def test_one(self):
        pass

    @pytest.mark.parametrize("foo", [1, 2])
    def test_two(self, foo):
        pass


def test_with_fixture(my_fixture):
    assert my_fixture == 123


def test_playground_should_have_test_env_set():
    assert os.environ.get("TPX_IS_TEST_EXECUTION") is not None
