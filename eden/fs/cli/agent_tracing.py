# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os

try:
    from artillery.artillery2.api.cpp.cli_lite.python import art_cli_lite
except ImportError:
    art_cli_lite = None


def trace() -> None:
    if art_cli_lite:
        art_cli_lite.trace_cli_for_process("cli.edenfsctl")


def exit_interrupted() -> None:
    try:
        if art_cli_lite:
            art_cli_lite.finish(timeout_s=1)
    finally:
        os._exit(130)
