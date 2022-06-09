# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import, division, print_function, unicode_literals

import argparse
import os
import pathlib
import subprocess
import sys
from typing import List, Union

from . import subcmd as subcmd_mod
from .cmd_util import require_checkout
from .subcmd import Subcmd


trace_cmd = subcmd_mod.Decorator()


def get_trace_stream_command() -> pathlib.Path:
    # TODO(T111405470) Rewrite in rust so we can avoid hardcoding these paths
    try:
        return pathlib.Path(os.environ["EDENFS_TRACE_STREAM"])
    except KeyError:
        if sys.platform == "win32":
            return pathlib.Path("C:/tools/eden/libexec/trace_stream.exe")
        return pathlib.Path("/usr/local/libexec/eden/eden_trace_stream")


def execute_cmd(arg_list: List[Union[pathlib.Path, str]]) -> int:
    if sys.platform == "win32":
        return subprocess.call(arg_list)
    else:
        encoded_args = [os.fsencode(arg) for arg in arg_list]
        os.execv(
            arg_list[0],
            encoded_args,
        )


@trace_cmd("hg", "Trace hg object fetches")
class TraceHgCommand(Subcmd):
    DESCRIPTION = """Trace EdenFS object fetches from Mercurial.
Events are encoded using the following emojis:

Event Type:
\u21E3 START
\u2193 FINISH

Resource Type:
\U0001F954 BLOB
\U0001F332 TREE

Import Priority (--verbose):
\U0001F7E5 LOW
\U0001F536 NORMAL
\U0001F7E2 HIGH

Import Cause (--verbose):
\u2753 UNKNOWN
\U0001F4C1 FS
\U0001F4E0 THRIFT
\U0001F4C5 PREFETCH
"""
    # pyre-fixme[15]: Type typing.Type[argparse.RawDescriptionHelpFormatter] is not a
    # subtype of the overridden attribute typing.Optional[argparse.HelpFormatter]
    FORMATTER = argparse.RawDescriptionHelpFormatter

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "checkout", default=None, nargs="?", help="Path to the checkout"
        )
        parser.add_argument(
            "--verbose",
            action="store_true",
            default=False,
            help="Show import priority and cause",
        )

    async def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = require_checkout(args, args.checkout)
        trace_stream_command = get_trace_stream_command()
        # TODO the verbose flag can be added directly to the list passed to execute_cmd
        # after the daemon with this new flag is running everywhere using
        # f"--verbose={'true' if args.verbose else 'false'}"
        verbose = []
        if args.verbose:
            verbose.append("--verbose=true")
        return execute_cmd(
            [
                trace_stream_command,
                "--mountRoot",
                checkout.path,
                "--trace=hg",
            ]
            + verbose
        )


@trace_cmd("fs", "Monitor filesystem requests.")
class TraceFsCommand(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "checkout", default=None, nargs="?", help="Path to the checkout"
        )
        parser.add_argument(
            "--reads",
            action="store_true",
            default=False,
            help="Limit trace to read operations",
        )
        parser.add_argument(
            "--writes",
            action="store_true",
            default=False,
            help="Limit trace to write operations",
        )

    async def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = require_checkout(args, args.checkout)
        trace_stream_command = get_trace_stream_command()
        return execute_cmd(
            [
                trace_stream_command,
                "--mountRoot",
                checkout.path,
                "--trace=fs",
                f"--reads={'true' if args.reads else 'false'}",
                f"--writes={'true' if args.writes else 'false'}",
            ]
        )


@trace_cmd("inode", "Monitor Inode Changes.")
class TraceInodeCommand(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "checkout", default=None, nargs="?", help="Path to the checkout"
        )
        parser.add_argument(
            "--retroactive",
            action="store_true",
            default=False,
            help="Provide stored inode events (from a buffer) across past changes",
        )

    async def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = require_checkout(args, args.checkout)
        trace_stream_command = get_trace_stream_command()
        return execute_cmd(
            [
                trace_stream_command,
                "--mountRoot",
                checkout.path,
                "--trace=inode",
                f"--retroactive={'true' if args.retroactive else 'false'}",
            ]
        )


@trace_cmd("thrift", "Monitor Thrift requests.")
class TraceThriftCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "checkout", default=None, nargs="?", help="Path to the checkout"
        )

    async def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = require_checkout(args, args.checkout)
        trace_stream_command = get_trace_stream_command()
        return execute_cmd(
            [
                trace_stream_command,
                "--mountRoot",
                checkout.path,
                "--trace=thrift",
            ]
        )
