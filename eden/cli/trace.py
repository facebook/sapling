#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import argparse

from . import cmd_util, subcmd as subcmd_mod
from .subcmd import Subcmd
from .trace_cmd import trace_cmd


try:
    import eden.cli.facebook.trace  # noqa: F401
except ImportError:
    pass


@trace_cmd("enable", "Enable tracing")
class EnableTraceCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client() as client:
            client.enableTracing()


@trace_cmd("disable", "Disable tracing")
class DisableTraceCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client() as client:
            client.disableTracing()


@subcmd_mod.subcmd("trace", "Commands for managing eden tracing")
class TraceCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        self.parser = parser
        self.add_subcommands(parser, trace_cmd.commands)

    def run(self, args: argparse.Namespace) -> int:
        self.parser.print_help()
        return 0
