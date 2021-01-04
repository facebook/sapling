# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import, division, print_function, unicode_literals

import argparse
import os

from . import subcmd as subcmd_mod
from .cmd_util import require_checkout
from .subcmd import Subcmd


trace_cmd = subcmd_mod.Decorator()


@trace_cmd("hg", "Trace hg object fetches")
class TraceHgCommand(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "checkout", default=None, nargs="?", help="Path to the checkout"
        )

    async def run(self, args: argparse.Namespace) -> int:
        from eden.fs.service.streamingeden.types import (  # @manual
            HgEventType,
            HgResourceType,
        )

        instance, checkout, _rel_path = require_checkout(args, args.checkout)
        async with await instance.get_thrift_client() as client:
            trace = await client.traceHgEvents(os.fsencode(checkout.path))

            # TODO: Like `edenfsctl strace`, it would be nice to see any
            # outstanding imports started before the trace begins.

            active_requests = {}

            async for event in trace:
                queue_event = None
                start_event = None
                if event.eventType == HgEventType.QUEUE:
                    active_requests[event.unique] = {"queue": event}
                elif event.eventType == HgEventType.START:
                    active_requests.setdefault(event.unique, {})["start"] = event
                    queue_event = active_requests[event.unique].get("queue")
                elif event.eventType == HgEventType.FINISH:
                    start_event = active_requests.pop(event.unique, {}).get("start")

                event_type_str = {
                    HgEventType.QUEUE: " ",
                    HgEventType.START: "\u21E3",
                    HgEventType.FINISH: "\u2193",
                }.get(event.eventType, "?")
                resource_type_str = {
                    HgResourceType.BLOB: "\U0001F954",
                    HgResourceType.TREE: "\U0001F332",
                }.get(event.resourceType, "?")

                if event.eventType == HgEventType.QUEUE:
                    # TODO: Might be interesting to add an option to see queuing events.
                    continue

                time_annotation = ""
                if event.eventType == HgEventType.START:
                    if queue_event:
                        queue_time = (
                            event.times.monotonic_time_ns
                            - queue_event.times.monotonic_time_ns
                        )
                        # Don't bother printing queue time under 1 ms.
                        if queue_time >= 1000000:
                            time_annotation = (
                                f" queued for {self.format_time(queue_time)}"
                            )
                elif event.eventType == HgEventType.FINISH:
                    if start_event:
                        fetch_time = (
                            event.times.monotonic_time_ns
                            - start_event.times.monotonic_time_ns
                        )
                        time_annotation = f" fetched in {self.format_time(fetch_time)}"

                print(
                    f"{event_type_str} {resource_type_str} {os.fsdecode(event.path)}{time_annotation}"
                )

            print(f"{checkout.path} was unmounted")

        return 0

    def format_time(self, ns: float) -> str:
        return "{:.3f} ms".format(ns / 1000000.0)
