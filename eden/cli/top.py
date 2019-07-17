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


class Top:
    def __init__(self):
        self.running = False

        self.height = 0
        self.width = 0
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
            self.height, self.width = stdscr.getmaxyx()

            stdscr.timeout(REFRESH_SECONDS * 1000)
            curses.curs_set(0)

            # Avoid displaying a blank screen during the first update()
            self.render(stdscr)

            while self.running:
                self.update(client)
                self.render(stdscr)
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
                key = (os.path.basename(mount), names_by_pid.get(pid, b"<unknown>"))
                val = (calls.count, pid)
                processes[key].append(val)

        return processes

    def populate_rows(
        self, processes: Dict[Tuple[bytes, bytes], List[Tuple[int, int]]]
    ):
        self.rows = []
        for (mount, name), calls_and_pids in sorted(
            processes.items(), key=lambda kv: self.compute_total(kv[1]), reverse=True
        ):
            name = self.format_name(name)
            mount = self.format_mount(mount)
            calls = self.compute_total(calls_and_pids)
            pids = self.format_pids(calls_and_pids)

            row = (name, mount, calls, pids)
            self.rows.append(row)

    def format_name(self, name):
        name = os.fsdecode(name)
        args = name.split("\x00", 2)

        # Focus on just the basename as the paths can be quite long
        cmd = args[0]
        name = os.path.basename(cmd)[:NAME_WIDTH]

        # Show cmdline args too, provided they fit in the remaining space
        remaining_space = NAME_WIDTH - len(name) - len(" ")
        if len(args) > 1 and remaining_space > 0:
            arg_str = args[1].replace("\x00", " ")[:remaining_space]
            name += f" {arg_str}"

        return name

    def format_mount(self, mount):
        return os.fsdecode(mount)[:MOUNT_WIDTH]

    def format_pids(self, calls_and_pids):
        pids = [pid for _, pid in sorted(calls_and_pids)]

        if not pids:
            return ""

        pids_str = str(pids[0])
        for pid in pids[1:]:
            new_str = f"{pids_str}, {pid}"
            if len(new_str) <= PIDS_WIDTH:
                pids_str = new_str
        return pids_str

    def compute_total(self, ls):
        return sum(c[0] for c in ls)

    def render(self, stdscr):
        stdscr.noutrefresh()
        stdscr.erase()

        self.render_top_bar(stdscr)
        # TODO: daemon memory/inode stats on line 2
        stdscr.hline(1, 0, curses.ACS_HLINE, self.width)
        self.render_column_titles(stdscr)
        self.render_rows(stdscr)

        curses.doupdate()

    def render_top_bar(self, stdscr):
        TITLE = "eden top"
        hostname = socket.gethostname()[: self.width]
        date = datetime.datetime.now().strftime("%x %X")[: self.width]

        # left: title
        stdscr.addnstr(0, 0, TITLE, self.width)
        # center: date
        stdscr.addnstr(0, (self.width - len(date)) // 2, date, self.width)
        # right: hostname
        stdscr.addnstr(0, self.width - len(hostname), hostname, self.width)

    def render_column_titles(self, stdscr):
        LINE = 2
        ROW = ("PROCESS", "MOUNT", "FUSE CALLS", "TOP PIDS")
        self.render_row(stdscr, LINE, ROW, curses.A_REVERSE)

    def render_rows(self, stdscr):
        START_LINE = 3
        line_numbers = range(START_LINE, self.height - 1)

        for line, row in zip(line_numbers, self.rows):
            self.render_row(stdscr, line, row)

    def render_row(self, stdscr, y, data, style=curses.A_NORMAL):
        SPACING = (NAME_WIDTH, MOUNT_WIDTH, CALLS_WIDTH, PIDS_WIDTH)
        PAD = " " * 4
        text = "".join(f"{str:{len}}{PAD}" for str, len in zip(data, SPACING))

        stdscr.addnstr(y, 0, text.ljust(self.width), self.width, style)

    def get_keypress(self, stdscr):
        key = stdscr.getch()
        if key == curses.KEY_RESIZE:
            curses.update_lines_cols()
            stdscr.redrawwin()
            self.height, self.width = stdscr.getmaxyx()
        elif key == ord("q"):
            self.running = False
