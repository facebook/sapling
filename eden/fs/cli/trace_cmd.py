# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import, division, print_function, unicode_literals

import argparse
import os
import pathlib
import sys

from . import subcmd as subcmd_mod
from .cmd_util import require_checkout
from .subcmd import Subcmd


trace_cmd = subcmd_mod.Decorator()


def get_trace_stream_command() -> pathlib.Path:
    try:
        return pathlib.Path(os.environ["EDENFS_TRACE_STREAM"])
    except KeyError:
        return pathlib.Path("/usr/local/libexec/eden/eden_trace_stream")


@trace_cmd("hg", "Trace hg object fetches")
class TraceHgCommand(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "checkout", default=None, nargs="?", help="Path to the checkout"
        )

    async def run(self, args: argparse.Namespace) -> int:
        if sys.platform == "win32":
            print("Not yet supported on Windows", file=sys.stderr)
            return 1
        instance, checkout, _rel_path = require_checkout(args, args.checkout)

        trace_stream_command = get_trace_stream_command()
        # TODO: Use subprocess.call on Windows.
        os.execl(
            trace_stream_command,
            os.fsencode(trace_stream_command),
            b"--mountRoot",
            os.fsencode(checkout.path),
            b"--trace=hg",
        )
