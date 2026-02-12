#!/usr/bin/env python3
# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

import pytest
from eden.mononoke.tests.mock_restricted_path.test_infra.python.simple.simple import add

# Simple tests to demonstrate pytest features


def test_upper():
    assert "FOO" == "foo".upper()


def test_isupper():
    assert "FOO".isupper() is True
    assert "Foo".isupper() is False


def test_split():
    s = "hello world"
    assert s.split() == ["hello", "world"]


# Assert that an exception is raised
def test_split_raises():
    s = "hello world"
    with pytest.raises(TypeError):
        s.split(2)


# Parameterized tests
@pytest.mark.parametrize("a, b, expected", [(1, 2, 3), (4, 2, 6), (5, 2, 7)])
def test_add(a, b, expected):
    assert add(a, b) == expected


# Fixtures
@pytest.fixture
def left_number():
    return 10


@pytest.fixture
def right_number():
    return 20


def test_add_with_fixtures(left_number, right_number):
    assert add(left_number, right_number) == 30
