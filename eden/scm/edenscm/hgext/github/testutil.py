# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""test utilities for doctests in this folder
"""


class FakeContext:
    def __init__(self, desc: str):
        self._desc = desc

    def description(self) -> str:
        return self._desc


def fake_args():
    return {
        "cache": {},
        "revcache": {},
    }
