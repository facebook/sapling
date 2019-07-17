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
from typing import DefaultDict, List, Tuple

from . import cmd_util


REFRESH_SECONDS = 2

NAME_WIDTH = 15
MOUNT_WIDTH = 15
CALLS_WIDTH = 12
PIDS_WIDTH = 25


class Top:
    def __init__(self):
        self.running = False
        self.ephemeral = False

        # Maps (mount, name) pairs to another dictionary,
        # which tracks the # of FUSE calls per PID
        self.processes: DefaultDict[
            Tuple[bytes, bytes], DefaultDict[int, int]
        ] = collections.defaultdict(lambda: collections.defaultdict(lambda: 0))

        self.height = 0
        self.width = 0
        self.rows: List[Tuple[bytes, bytes, int, bytes]] = []

    def start(self, args: argparse.Namespace) -> int:
        self.running = True
        self.ephemeral = args.ephemeral

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
        self.update_processes(client)
        self.update_rows()

    def update_processes(self, client):
        if self.ephemeral:
            self.processes.clear()

        counts = client.getAccessCounts(REFRESH_SECONDS)
        names_by_pid = counts.exeNamesByPid

        for mount, accesses in counts.fuseAccessesByMount.items():
            for pid, calls in accesses.fuseAccesses.items():
                mount = os.path.basename(mount)
                name = names_by_pid.get(pid, b"<unknown>")
                process = (mount, name)

                calls_by_pid = self.processes[process]
                # Delete, increment, and re-add to end of OrderedDict
                del self.processes[process]
                calls_by_pid[pid] += calls.count
                self.processes[process] = calls_by_pid

    def update_rows(self):
        self.rows = []

        ordered_processes = reversed(list(self.processes.items()))
        for (mount, name), calls_per_pid in ordered_processes:
            name = format_name(name)
            mount = format_mount(mount)
            calls = sum(calls_per_pid.values())

            # Sort PIDs by fuse calls
            sorted_pairs = sorted(calls_per_pid.items(), key=lambda kv: kv[1])
            pids = [pid for pid, _ in reversed(sorted_pairs)]
            pids = format_pids(pids)

            row = (name, mount, calls, pids)
            self.rows.append(row)

    def compute_total(self, ls):
        return sum(c[0] for c in ls)

    def render(self, stdscr):
        stdscr.clear()

        self.render_top_bar(stdscr)
        # TODO: daemon memory/inode stats on line 2
        stdscr.hline(1, 0, curses.ACS_HLINE, self.width)
        self.render_column_titles(stdscr)
        self.render_rows(stdscr)

        stdscr.refresh()

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


def format_name(name):
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


def format_mount(mount):
    return os.fsdecode(mount)[:MOUNT_WIDTH]


def format_pids(pids):
    if not pids:
        return ""

    pids_str = str(pids[0])
    for pid in pids[1:]:
        new_str = f"{pids_str}, {pid}"
        if len(new_str) <= PIDS_WIDTH:
            pids_str = new_str
    return pids_str
