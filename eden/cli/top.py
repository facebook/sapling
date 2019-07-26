#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import collections
import copy
import datetime
import os
import socket
from typing import Dict, List, Tuple

from facebook.eden.ttypes import AccessCounts

from . import cmd_util


PID_WIDTH = 7
CMD_WIDTH = 20
MOUNT_WIDTH = 15
READS_WIDTH = 10
WRITES_WIDTH = 11
TOTAL_WIDTH = 10

TITLES = ("TOP PID", "COMMAND", "MOUNT", "FUSE READS", "FUSE WRITES", "FUSE TOTAL")
SPACING = (PID_WIDTH, CMD_WIDTH, MOUNT_WIDTH, READS_WIDTH, WRITES_WIDTH, TOTAL_WIDTH)


class Top:
    def __init__(self):
        import curses

        self.curses = curses

        self.running = False
        self.ephemeral = False
        self.refresh_rate = 1

        # Processes are stored by PID
        self.processes: Dict[int, Process] = collections.OrderedDict()

        self.height = 0
        self.width = 0
        self.rows: List[Tuple[int, bytes, bytes, int, int, int]] = []

    def start(self, args: argparse.Namespace) -> int:
        self.running = True
        self.ephemeral = args.ephemeral
        self.refresh_rate = args.refresh_rate

        eden = cmd_util.get_eden_instance(args)
        with eden.get_thrift_client() as client:
            try:
                self.curses.wrapper(self.run(client))
            except KeyboardInterrupt:
                pass
        return 0

    def run(self, client):
        def mainloop(stdscr):
            self.height, self.width = stdscr.getmaxyx()
            stdscr.timeout(self.refresh_rate * 1000)
            self.curses.curs_set(0)

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

        counts = client.getAccessCounts(self.refresh_rate)

        for mount, accesses in counts.fuseAccessesByMount.items():
            for pid, access_counts in accesses.fuseAccesses.items():
                if pid in self.processes:
                    # Has accessed FUSE again, so move to end of OrderedDict.
                    temp = self.processes[pid]
                    del self.processes[pid]
                    self.processes[pid] = temp
                else:
                    cmd = counts.exeNamesByPid.get(pid, b"<unknown>")
                    mount = os.path.basename(mount)
                    self.processes[pid] = Process(pid, cmd, mount)

                self.processes[pid].access_counts.reads += access_counts.reads
                self.processes[pid].access_counts.writes += access_counts.writes
                self.processes[pid].access_counts.total += access_counts.total

    def update_rows(self):
        ordered_processes = reversed(list(self.processes.values()))

        # Group same-named processes
        aggregated_processes = collections.OrderedDict()
        for process in ordered_processes:
            key = process.get_key()
            if key in aggregated_processes:
                aggregated_processes[key].aggregate(process)
            else:
                aggregated_processes[key] = copy.deepcopy(process)

        self.rows = []
        for process in aggregated_processes.values():
            row = (
                process.pid,
                format_cmd(process.cmd),
                format_mount(process.mount),
                process.access_counts.reads,
                process.access_counts.writes,
                process.access_counts.total,
            )
            self.rows.append(row)

    def render(self, stdscr):
        stdscr.clear()

        self.render_top_bar(stdscr)
        # TODO: daemon memory/inode stats on line 2
        self.render_column_titles(stdscr)
        self.render_rows(stdscr)

        stdscr.refresh()

    def render_top_bar(self, stdscr):
        TITLE = "eden top"
        hostname = socket.gethostname()[: self.width]
        date = datetime.datetime.now().strftime("%x %X")[: self.width]
        extra_space = self.width - len(TITLE + hostname + date)

        # left: title
        stdscr.addnstr(0, 0, TITLE, self.width)
        # center: date
        stdscr.addnstr(0, len(TITLE) + extra_space // 2, date, self.width)
        # right: hostname
        stdscr.addnstr(0, self.width - len(hostname), hostname, self.width)

    def render_column_titles(self, stdscr):
        LINE = 2
        self.render_row(stdscr, LINE, TITLES, self.curses.A_REVERSE)

    def render_rows(self, stdscr):
        START_LINE = 3
        line_numbers = range(START_LINE, self.height - 1)

        for line, row in zip(line_numbers, self.rows):
            self.render_row(stdscr, line, row, self.curses.A_NORMAL)

    def render_row(self, stdscr, y, data, style):
        text = " ".join(f"{str:{len}}"[:len] for str, len in zip(data, SPACING))
        stdscr.addnstr(y, 0, text.ljust(self.width), self.width, style)

    def get_keypress(self, stdscr):
        key = stdscr.getch()
        if key == self.curses.KEY_RESIZE:
            self.curses.update_lines_cols()
            stdscr.redrawwin()
            self.height, self.width = stdscr.getmaxyx()
        elif key == ord("q"):
            self.running = False


class Process:
    def __init__(self, pid, cmd, mount):
        self.pid = pid
        self.cmd = cmd
        self.mount = mount
        self.access_counts = AccessCounts(0, 0, 0)

    def get_key(self):
        return (self.cmd, self.mount)

    def aggregate(self, other):
        self.pid = other.pid

        self.access_counts.reads += other.access_counts.reads
        self.access_counts.writes += other.access_counts.writes
        self.access_counts.total += other.access_counts.total


def format_cmd(cmd):
    args = os.fsdecode(cmd).split("\x00", 2)

    # Focus on just the basename as the paths can be quite long
    cmd = os.path.basename(args[0])[:CMD_WIDTH]

    # Show cmdline args too, provided they fit in the remaining space
    remaining_space = CMD_WIDTH - len(cmd) - len(" ")
    if len(args) > 1 and remaining_space > 0:
        arg_str = args[1].replace("\x00", " ")[:remaining_space]
        cmd += f" {arg_str}"

    return cmd


def format_mount(mount):
    return os.fsdecode(mount)[:MOUNT_WIDTH]
