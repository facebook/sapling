#!/usr/bin/env python3
# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

# pyre-strict


# pyre-fixme[2]: Parameter must be annotated.
def add(a, b) -> int:
    value = a + b
    return value


# pyre-fixme[2]: Parameter must be annotated.
def sub(a, b) -> int:
    value = a - b
    return value
