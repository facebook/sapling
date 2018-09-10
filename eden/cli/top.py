#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import argparse
import collections
import curses
import datetime
import os
import socket
from typing import Tuple

from . import cmd_util


REFRESH_SECONDS = 2


def refresh(stdscr, client):
    height, width = stdscr.getmaxyx()

    counts = client.getAccessCounts(REFRESH_SECONDS)
    exeNames = counts.exeNamesByPid

    countsByMountsAndBaseNames: collections.defaultdict[
        Tuple[bytes, bytes], int
    ] = collections.defaultdict(lambda: 0)

    for mount, fuseMountAccesses in counts.fuseAccessesByMount.items():
        for pid, accessCount in fuseMountAccesses.fuseAccesses.items():
            countsByMountsAndBaseNames[
                (
                    os.path.basename(mount),
                    os.path.basename(exeNames.get(pid, b"<unknown>")),
                )
            ] += accessCount.count

    hostname = socket.gethostname()[:width]
    date = datetime.datetime.now().strftime("%x %X")[:width]

    stdscr.addnstr(0, 0, "eden top", width)
    # center the date
    stdscr.addnstr(0, (width - len(date)) // 2, date, width)
    # right-align the hostname
    stdscr.addnstr(0, width - len(hostname), hostname, width)

    # TODO: daemon memory/inode stats on line 2
    stdscr.hline(1, 0, curses.ACS_HLINE, width)

    process_width = 15
    mount_width = 15
    fuse_width = 12
    padding = " " * 4

    heading = (
        f'{"PROCESS":{process_width}}{padding}'
        + f'{"MOUNT":{mount_width}}{padding}'
        + f'{"FUSE CALLS":>{fuse_width}}'
    )
    stdscr.addnstr(2, 0, heading.ljust(width), width, curses.A_REVERSE)

    line = 3
    for (mount, exe_name), count in sorted(
        countsByMountsAndBaseNames.items(), key=lambda kv: kv[1], reverse=True
    ):
        if line >= height:
            break
        exe_name_printed = os.fsdecode(exe_name)[:process_width]
        mount_printed = os.fsdecode(mount)[:mount_width]

        # Fully writing the last line is an error, so write one fewer character.
        max_line_width = width - 1 if line + 1 == height else width
        stdscr.addnstr(
            line,
            0,
            f"{exe_name_printed:{process_width}}{padding}"
            + f"{mount_printed:{mount_width}}{padding}"
            + f"{count:{fuse_width}}",
            max_line_width,
        )
        line += 1


def run(client):
    def mainloop(stdscr):
        stdscr.timeout(REFRESH_SECONDS * 1000)
        curses.curs_set(0)
        while True:
            stdscr.noutrefresh()
            stdscr.erase()
            refresh(stdscr, client)
            curses.doupdate()

            key = stdscr.getch()
            if key == curses.KEY_RESIZE:
                curses.update_lines_cols()
                stdscr.redrawwin()
            if key == ord("q"):
                break

    return mainloop


def show(args: argparse.Namespace) -> int:
    instance = cmd_util.get_eden_instance(args)
    with instance.get_thrift_client() as client:
        try:
            curses.wrapper(run(client))
        except KeyboardInterrupt:
            pass
    return 0
