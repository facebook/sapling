#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import collections
import curses
import datetime
import os
import socket
from typing import List, Tuple

from . import cmd_util


REFRESH_SECONDS = 2


def refresh(stdscr, client):
    height, width = stdscr.getmaxyx()

    counts = client.getAccessCounts(REFRESH_SECONDS)
    exeNames = counts.exeNamesByPid

    countsByMountsAndBaseNames: collections.defaultdict[
        Tuple[bytes, bytes], List[Tuple[int, int]]
    ] = collections.defaultdict(lambda: [])

    for mount, fuseMountAccesses in counts.fuseAccessesByMount.items():
        for pid, accessCount in fuseMountAccesses.fuseAccesses.items():
            countsByMountsAndBaseNames[
                (os.path.basename(mount), exeNames.get(pid, b"<unknown>"))
            ].append((accessCount.count, pid))

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
    pids_width = 25
    padding = " " * 4

    heading = (
        f'{"PROCESS":{process_width}}{padding}'
        + f'{"MOUNT":{mount_width}}{padding}'
        + f'{"FUSE CALLS":>{fuse_width}}{padding}'
        + f'{"TOP PIDS":{pids_width}}'
    )
    stdscr.addnstr(2, 0, heading.ljust(width), width, curses.A_REVERSE)

    def compute_total(ls):
        return sum(c[0] for c in ls)

    def summarize_exe(name):
        args = name.split("\x00", 2)
        # focus on just the basename as the paths can be quite long
        result = os.path.basename(args[0])[:process_width]
        if len(args) > 1 and len(result) < process_width:
            # show cmdline args too, provided they fit in the available space
            args = args[1].replace("\x00", " ")
            result += " "
            result += args[: process_width - len(result)]
        return result

    line = 3
    for (mount, exe_name), counts_and_pids in sorted(
        countsByMountsAndBaseNames.items(),
        key=lambda kv: compute_total(kv[1]),
        reverse=True,
    ):
        if line >= height:
            break

        total_count = compute_total(counts_and_pids)

        exe_name_printed = summarize_exe(os.fsdecode(exe_name))
        mount_printed = os.fsdecode(mount)[:mount_width]

        pids_str = ""
        for _count, pid in sorted(counts_and_pids):
            if not pid:
                continue
            if not pids_str:
                pids_str = str(pid)
            else:
                new_str = pids_str + "," + str(pid)
                if len(new_str) > pids_width:
                    break
                pids_str = new_str

        # Fully writing the last line is an error, so write one fewer character.
        max_line_width = width - 1 if line + 1 == height else width
        stdscr.addnstr(
            line,
            0,
            f"{exe_name_printed:{process_width}}{padding}"
            + f"{mount_printed:{mount_width}}{padding}"
            + f"{total_count:{fuse_width}}{padding}"
            + f"{pids_str:{pids_width}}",
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
