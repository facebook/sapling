# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Every synthetic corpus case, one module per corpus family, in a stable order."""

from __future__ import annotations

from typing import Any

from eden.integration.correctness.exploratory.test.corpus.author import Run, SpecCase
from eden.integration.correctness.exploratory.test.corpus.families import (
    aggregate_omission,
    attributes,
    bytes_status,
    checkout_interrupted,
    checkout_sl,
    checkout_thrift,
    cross_channel,
    differential,
    differential_validation,
    direct,
    errno_twins,
    existence,
    fd,
    journal,
    manifest,
    not_evaluated,
    readdir,
    rename,
    repeated_producers,
    task_key,
    windows,
)


def runs() -> list[Run]:
    found = [
        *aggregate_omission.runs(),
        *attributes.runs(),
        *bytes_status.runs(),
        *checkout_sl.runs(),
        *checkout_interrupted.runs(),
        *checkout_thrift.runs(),
        *cross_channel.runs(),
        *differential.runs(),
        *direct.runs(),
        *errno_twins.runs(),
        *existence.runs(),
        *fd.runs(),
        *journal.runs(),
        *manifest.runs(),
        *not_evaluated.runs(),
        *readdir.runs(),
        *rename.runs(),
        *repeated_producers.runs(),
        *task_key.runs(),
        *windows.runs(),
    ]
    return found


def spec_cases() -> list[SpecCase]:
    return differential_validation.spec_cases()


def twins() -> dict[str, Any]:
    return task_key.twins()
