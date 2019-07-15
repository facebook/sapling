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
from typing import Dict, List, Tuple

from . import cmd_util


REFRESH_SECONDS = 2

process_width = 15
mount_width = 15
fuse_width = 12
pids_width = 25
padding = " " * 4


class Top:
    def __init__(self):
        self.running = False
        self.rows: List = []

    def start(self, args: argparse.Namespace) -> int:
        self.running = True

        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client() as client:
            try:
                curses.wrapper(self.run(client))
            except KeyboardInterrupt:
                pass
        return 0

    def run(self, client):
        def mainloop(stdscr):
            stdscr.timeout(REFRESH_SECONDS * 1000)
            curses.curs_set(0)

            while self.running:
                stdscr.noutrefresh()
                stdscr.erase()
                self.update(client)
                self.render(stdscr)
                curses.doupdate()

                self.get_keypress(stdscr)

        return mainloop

    def update(self, client):
        countsByMountsAndBaseNames = self.get_process_data(client)
        self.populate_rows(countsByMountsAndBaseNames)

    def get_process_data(self, client):
        counts = client.getAccessCounts(REFRESH_SECONDS)
        exeNames = counts.exeNamesByPid

        countsByMountsAndBaseNames: Dict[
            Tuple[bytes, bytes], List[Tuple[int, int]]
        ] = collections.defaultdict(lambda: [])

        for mount, fuseMountAccesses in counts.fuseAccessesByMount.items():
            for pid, accessCount in fuseMountAccesses.fuseAccesses.items():
                countsByMountsAndBaseNames[
                    (os.path.basename(mount), exeNames.get(pid, b"<unknown>"))
                ].append((accessCount.count, pid))

        return countsByMountsAndBaseNames

    def populate_rows(
        self,
        countsByMountsAndBaseNames: Dict[Tuple[bytes, bytes], List[Tuple[int, int]]],
    ):
        self.rows = []
        for (mount, exe_name), counts_and_pids in sorted(
            countsByMountsAndBaseNames.items(),
            key=lambda kv: self.compute_total(kv[1]),
            reverse=True,
        ):
            exe_name_printed = self.summarize_exe(os.fsdecode(exe_name))
            mount_printed = os.fsdecode(mount)[:mount_width]
            total_count = self.compute_total(counts_and_pids)
            pids_str = self.format_top_pids(counts_and_pids)

            row = (exe_name_printed, mount_printed, total_count, pids_str)
            self.rows.append(row)

    def compute_total(self, ls):
        return sum(c[0] for c in ls)

    def render(self, stdscr):
        height, width = stdscr.getmaxyx()

        self.render_top_bar(stdscr)
        # TODO: daemon memory/inode stats on line 2
        stdscr.hline(1, 0, curses.ACS_HLINE, width)

        self.render_column_titles(stdscr)
        self.render_rows(stdscr)

    def render_top_bar(self, stdscr):
        height, width = stdscr.getmaxyx()

        hostname = socket.gethostname()[:width]
        date = datetime.datetime.now().strftime("%x %X")[:width]

        stdscr.addnstr(0, 0, "eden top", width)
        # center the date
        stdscr.addnstr(0, (width - len(date)) // 2, date, width)
        # right-align the hostname
        stdscr.addnstr(0, width - len(hostname), hostname, width)

    def render_column_titles(self, stdscr):
        height, width = stdscr.getmaxyx()

        heading = (
            f'{"PROCESS":{process_width}}{padding}'
            + f'{"MOUNT":{mount_width}}{padding}'
            + f'{"FUSE CALLS":>{fuse_width}}{padding}'
            + f'{"TOP PIDS":{pids_width}}'
        )
        stdscr.addnstr(2, 0, heading.ljust(width), width, curses.A_REVERSE)

    def render_rows(self, stdscr):
        height, width = stdscr.getmaxyx()

        line = 3
        for exe_name_printed, mount_printed, total_count, pids_str in self.rows:
            if line >= height:
                break

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

    def summarize_exe(self, name):
        args = name.split("\x00", 2)
        # focus on just the basename as the paths can be quite long
        result = os.path.basename(args[0])[:process_width]
        if len(args) > 1 and len(result) < process_width:
            # show cmdline args too, provided they fit in the available space
            args = args[1].replace("\x00", " ")
            result += " "
            result += args[: process_width - len(result)]
        return result

    def format_top_pids(self, counts_and_pids):
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
        return pids_str

    def get_keypress(self, stdscr):
        key = stdscr.getch()
        if key == curses.KEY_RESIZE:
            curses.update_lines_cols()
            stdscr.redrawwin()
        if key == ord("q"):
            self.running = False
