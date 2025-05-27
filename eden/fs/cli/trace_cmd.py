# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

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


@trace_cmd("sl", "Trace sapling object fetches", aliases=["hg"])
class TraceSlCommand(Subcmd):
    DESCRIPTION = """Trace EdenFS object fetches from Sapling.

With the --retroactive flag, this will print a list of the past N object fetches.
By default, it will print up to N=100 events, but this can be configured with the
following config option:
[telemetry]
activitybuffer-max-events = 100

Events are encoded using the following emojis:

Event Type:
\u2193 START
\u2714 FINISH
\u26a0 FAILURE

Resource Type:
\U0001f4c4 Blob (file content)
\U0001f332 Tree (directory content)
\U0001f5c2\U00000020 Blob Aux Data (sha1, size, blake3)
\U000100b7 Tree Aux Data (digest size, digest hash)

Import Priority (--verbose):
\U0001f535 LOW
\U0001f7e1 NORMAL
\U0001f534 HIGH

Import Cause (--verbose):
\u2753 UNKNOWN
\U0001f4c1 FS
\U0001f4e0 THRIFT
\U0001f4c5 PREFETCH

Fetched Source in Sapling (--verbose):
\U0001f4bb LOCAL
\U0001f310 REMOTE
\U0001f937 UNKNOWN
\U00000020\U00000020 not available yet

"""
    # pyre-fixme[15]: Type typing.Type[argparse.RawDescriptionHelpFormatter] is not a
    # subtype of the overridden attribute typing.Optional[argparse.HelpFormatter]
    FORMATTER = argparse.RawDescriptionHelpFormatter

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "checkout",
            default=None,
            nargs=argparse.OPTIONAL,
            help="Path to the checkout",
        )
        parser.add_argument(
            "--verbose",
            action="store_true",
            default=False,
            help="Show import priority, cause, and fetched source",
        )
        parser.add_argument(
            "--retroactive",
            action="store_true",
            default=False,
            help="Provide stored sapling events (from a buffer) across past changes",
        )

    async def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = require_checkout(args, args.checkout)
        trace_stream_command = get_trace_stream_command()
        return execute_cmd(
            [
                trace_stream_command,
                "--mountRoot",
                checkout.path,
                "--trace=hg",
                f"--verbose={'true' if args.verbose else 'false'}",
                f"--retroactive={'true' if args.retroactive else 'false'}",
            ]
        )


@trace_cmd("fs", "Monitor filesystem requests.")
class TraceFsCommand(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "checkout",
            default=None,
            nargs=argparse.OPTIONAL,
            help="Path to the checkout",
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


@trace_cmd("inode", "Monitor inode state changes (loads and materializations).")
class TraceInodeCommand(Subcmd):
    DESCRIPTION = """Trace EdenFS inode state changes including inode loads and materializations.

With the --retroactive flag, this will print a list of the past N inode changes.
By default, it will print up to N=100 events, but this can be configured with the
following config option:
[telemetry]
activitybuffer-max-events = 100

Loading an inode refers to fetching state for the inode to store into memory.
While loading can some times cause fetching data contents for an inode, this is
not always the case, and fetching can sometimes happen in other cases. Content
data fetches from sapling servers can be traced with the eden trace sl command. Note:
loading an inode will cause its parent inode to be loaded if it isn't already.

Materializing an inode refers to modifying the inode's data such that no source
control object ID can be used to refer to the inode's data. This causes the
inode's data to be saved locally in EdenFS's overlay and causes further
materializations of the inode's parent. Note: this means one materialization
will cause materializations in parent inodes until a previously materialized
parent inode is reached (this may materialize inodes all the way to the root).

Further definitions regarding inodes state changes can be found at
https://github.com/facebook/sapling/blob/main/eden/fs/docs/Glossary.md

Events for this command are encoded using the following emojis/letters:

Event Progress:
\u2193 START
\u2714 FINISH
\u26a0 FAILURE

Resource Type:
\U0001f4c4 BLOB/FILE
\U0001f332 TREE/DIRECTORY

Event Type
L LOAD
M MATERIALIZE
"""
    # pyre-fixme[15]: Type typing.Type[argparse.RawDescriptionHelpFormatter] is not a
    # subtype of the overridden attribute typing.Optional[argparse.HelpFormatter]
    FORMATTER = argparse.RawDescriptionHelpFormatter

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "checkout",
            default=None,
            nargs=argparse.OPTIONAL,
            help="Path to the checkout",
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
    DESCRIPTION = """Trace thrift requests made to EdenFS.

    With the --retroactive flag, this will print a list of the past N thrift requests.
    By default, it will print up to N=100 events, but this can be configured with the
    following config option:
    [telemetry]
    activitybuffer-max-events = 100

    A '+' represents a thrift call starting, and a '-' represents it's end. Additionally,
    the thrift request ID is displayed along with what process id (pid) the call was made
    from. Finally, the thrift method name and duration for how long the thrift call took
    are displayed as well.
    """

    # pyre-fixme[15]: Type typing.Type[argparse.RawDescriptionHelpFormatter] is not a
    # subtype of the overridden attribute typing.Optional[argparse.HelpFormatter]
    FORMATTER = argparse.RawDescriptionHelpFormatter

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "checkout",
            default=None,
            nargs=argparse.OPTIONAL,
            help="Path to the checkout",
        )
        parser.add_argument(
            "--retroactive",
            action="store_true",
            default=False,
            help="Provide stored thrift request events (from a buffer) across past changes",
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
                f"--retroactive={'true' if args.retroactive else 'false'}",
            ]
        )


@trace_cmd("task", "Monitor tasks in EdenFS daemon")
class TraceTaskCmd(Subcmd):
    DESCRIPTION = """Trace tasks happening inside EdenFS daemon. The tasks
    must be pre-instrumented.

    Use `--chrome-trace` flag if you want to analyze the output with Chrome
    trace tools.
    """

    # pyre-fixme[15]: Type typing.Type[argparse.RawDescriptionHelpFormatter] is not a
    # subtype of the overridden attribute typing.Optional[argparse.HelpFormatter]
    FORMATTER = argparse.RawDescriptionHelpFormatter

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "checkout",
            default=None,
            nargs=argparse.OPTIONAL,
            help="Path to the checkout",
        )
        parser.add_argument(
            "--chrome-trace",
            action="store_true",
            default=False,
            help="Set the output in Chrome trace format",
        )

    async def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = require_checkout(args, args.checkout)
        trace_stream_command = get_trace_stream_command()
        return execute_cmd(
            [
                trace_stream_command,
                "--mountRoot",
                checkout.path,
                f"--trace={'task-chrome-trace' if args.chrome_trace else 'task'}",
            ]
        )
