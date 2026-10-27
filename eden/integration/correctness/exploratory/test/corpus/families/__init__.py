# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Every synthetic corpus case, one module per corpus family, in a stable order."""

from __future__ import annotations

from eden.integration.correctness.exploratory.test.corpus.author import Run
from eden.integration.correctness.exploratory.test.corpus.families import (
    aggregate_omission,
)


def runs() -> list[Run]:
    found = [
        *aggregate_omission.runs(),
    ]
    return found
