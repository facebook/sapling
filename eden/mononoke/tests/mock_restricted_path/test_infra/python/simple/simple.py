#!/usr/bin/env python3
# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

# pyre-strict


def add(a: int, b: int) -> int:
    result = a + b
    return result


def div(a: int, b: int) -> int:
    result = a / b
    # pyre-fixme[7]: Expected `int` but got `float`.
    return result


def mul(a: int, b: int) -> int:
    result = a * b
    return result


def sub(a: int, b: int) -> int:  # pragma: no cover
    result = a - b
    return result
