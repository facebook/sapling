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

NAME_WIDTH = 15
MOUNT_WIDTH = 15
CALLS_WIDTH = 12
PIDS_WIDTH = 25
PADDING = " " * 4


class Top:
    def __init__(self):
        self.running = False
        self.rows: List = []

    def start(self, args: argparse.Namespace) -> int:
        self.running = True

        eden = cmd_util.get_eden_instance(args)
        with eden.get_thrift_client() as client:
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
        processes = self.get_process_data(client)
        self.populate_rows(processes)

    def get_process_data(self, client):
        counts = client.getAccessCounts(REFRESH_SECONDS)
        names_by_pid = counts.exeNamesByPid

        processes: Dict[
            Tuple[bytes, bytes], List[Tuple[int, int]]
        ] = collections.defaultdict(lambda: [])

        for mount, accesses in counts.fuseAccessesByMount.items():
            for pid, calls in accesses.fuseAccesses.items():
                processes[
                    (os.path.basename(mount), names_by_pid.get(pid, b"<unknown>"))
                ].append((calls.count, pid))

        return processes

    def populate_rows(
        self, processes: Dict[Tuple[bytes, bytes], List[Tuple[int, int]]]
    ):
        self.rows = []
        for (mount, name), calls_and_pids in sorted(
            processes.items(), key=lambda kv: self.compute_total(kv[1]), reverse=True
        ):
            name = self.format_name(os.fsdecode(name))
            mount = self.format_mount(mount)
            calls = self.compute_total(calls_and_pids)
            pids = self.format_pids(calls_and_pids)

            row = (name, mount, calls, pids)
            self.rows.append(row)

    def format_name(self, name):
        args = name.split("\x00", 2)
        # focus on just the basename as the paths can be quite long
        result = os.path.basename(args[0])[:NAME_WIDTH]
        if len(args) > 1 and len(result) < NAME_WIDTH:
            # show cmdline args too, provided they fit in the available space
            args = args[1].replace("\x00", " ")
            result += " "
            result += args[: NAME_WIDTH - len(result)]
        return result

    def format_mount(self, mount):
        return os.fsdecode(mount)[:MOUNT_WIDTH]

    def format_pids(self, calls_and_pids):
        pids_str = ""
        for _, pid in sorted(calls_and_pids):
            if not pid:
                continue
            if not pids_str:
                pids_str = str(pid)
            else:
                new_str = pids_str + "," + str(pid)
                if len(new_str) > PIDS_WIDTH:
                    break
                pids_str = new_str
        return pids_str

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
            f'{"PROCESS":{NAME_WIDTH}}{PADDING}'
            + f'{"MOUNT":{MOUNT_WIDTH}}{PADDING}'
            + f'{"FUSE CALLS":>{CALLS_WIDTH}}{PADDING}'
            + f'{"TOP PIDS":{PIDS_WIDTH}}'
        )
        stdscr.addnstr(2, 0, heading.ljust(width), width, curses.A_REVERSE)

    def render_rows(self, stdscr):
        height, width = stdscr.getmaxyx()

        line = 3
        for name, mount, calls, pids in self.rows:
            if line >= height:
                break

            # Fully writing the last line is an error, so write one fewer character.
            max_line_width = width - 1 if line + 1 == height else width
            stdscr.addnstr(
                line,
                0,
                f"{name:{NAME_WIDTH}}{PADDING}"
                + f"{mount:{MOUNT_WIDTH}}{PADDING}"
                + f"{calls:{CALLS_WIDTH}}{PADDING}"
                + f"{pids:{PIDS_WIDTH}}",
                max_line_width,
            )
            line += 1

    def get_keypress(self, stdscr):
        key = stdscr.getch()
        if key == curses.KEY_RESIZE:
            curses.update_lines_cols()
            stdscr.redrawwin()
        if key == ord("q"):
            self.running = False
