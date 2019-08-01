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
import time
from typing import Dict, List

from facebook.eden.ttypes import AccessCounts

from . import cmd_util


Row = collections.namedtuple(
    "Row",
    "top_pid command mount fuse_reads fuse_writes fuse_total fuse_backing_store_imports fuse_last_access",
)

COLUMN_TITLES = Row(
    top_pid="TOP PID",
    command="COMMAND",
    mount="MOUNT",
    fuse_reads="FUSE READS",
    fuse_writes="FUSE WRITES",
    fuse_total="FUSE TOTAL",
    fuse_backing_store_imports="BACKING IMPORTS",
    fuse_last_access="LST",
)
COLUMN_SPACING = Row(
    top_pid=7,
    command=25,
    mount=15,
    fuse_reads=10,
    fuse_writes=11,
    fuse_total=10,
    fuse_backing_store_imports=15,
    fuse_last_access=3,
)


class Top:
    def __init__(self):
        import curses

        self.curses = curses

        self.running = False
        self.ephemeral = False
        self.refresh_rate = 1

        # Processes are stored by PID
        self.processes: Dict[int, Process] = {}
        self.rows: List = []
        self.selected_column = COLUMN_TITLES.index("LST")

        self.height = 0
        self.width = 0

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
        if self.ephemeral:
            self.processes.clear()

        counts = client.getAccessCounts(self.refresh_rate)

        for mount, accesses in counts.accessesByMount.items():
            for pid, access_counts in accesses.accessCountsByPid.items():
                if pid not in self.processes:
                    cmd = counts.cmdsByPid.get(pid, b"<unknown>")
                    self.processes[pid] = Process(pid, cmd, mount)

                self.processes[pid].increment_counts(access_counts)
                self.processes[pid].last_access = time.monotonic()

        for pid in self.processes.keys():
            self.processes[pid].is_running = os.path.exists(f"/proc/{pid}/")

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
        self.render_row(stdscr, LINE, COLUMN_TITLES, self.curses.A_REVERSE)

    def render_rows(self, stdscr):
        START_LINE = 3
        line_numbers = range(START_LINE, self.height - 1)

        aggregated_processes = {}
        for process in self.processes.values():
            key = process.get_key()
            if key in aggregated_processes:
                aggregated_processes[key].aggregate(process)
            else:
                aggregated_processes[key] = copy.deepcopy(process)

        sorted_processes = sorted(
            aggregated_processes.values(),
            key=lambda p: p.get_row()[self.selected_column],
            reverse=True,
        )

        for line, process in zip(line_numbers, sorted_processes):
            row = process.get_row()
            row = (fmt(data) for fmt, data in zip(COLUMN_FORMATTING, row))

            style = self.curses.A_BOLD if process.is_running else self.curses.A_NORMAL
            self.render_row(stdscr, line, row, style)

    def render_row(self, stdscr, y, data, style):
        text = " ".join(f"{str:{len}}"[:len] for str, len in zip(data, COLUMN_SPACING))
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
        self.cmd = format_cmd(cmd)
        self.mount = format_mount(mount)
        self.access_counts = AccessCounts(0, 0, 0, 0)
        self.last_access = time.monotonic()
        self.is_running = True

    def get_key(self):
        return (self.cmd, self.mount)

    def aggregate(self, other):
        self.increment_counts(other.access_counts)
        self.is_running |= other.is_running

        # Check if other is more relevant
        if other.is_running or other.last_access > self.last_access:
            self.pid = other.pid
            self.last_access = other.last_access

    def increment_counts(self, access_counts):
        self.access_counts.fuseReads += access_counts.fuseReads
        self.access_counts.fuseWrites += access_counts.fuseWrites
        self.access_counts.fuseTotal += access_counts.fuseTotal
        self.access_counts.fuseBackingStoreImports += (
            access_counts.fuseBackingStoreImports
        )

    def get_row(self):
        return Row(
            top_pid=self.pid,
            command=self.cmd,
            mount=self.mount,
            fuse_reads=self.access_counts.fuseReads,
            fuse_writes=self.access_counts.fuseWrites,
            fuse_total=self.access_counts.fuseTotal,
            fuse_backing_store_imports=self.access_counts.fuseBackingStoreImports,
            fuse_last_access=self.last_access,
        )


def format_cmd(cmd):
    args = os.fsdecode(cmd).split("\x00", 1)

    # Focus on just the basename as the paths can be quite long
    cmd = os.path.basename(args[0])

    # Show cmdline args too, if they exist
    if len(args) > 1:
        arg_str = args[1].replace("\x00", " ")
        cmd += f" {arg_str}"

    return cmd


def format_mount(mount):
    return os.fsdecode(os.path.basename(mount))


def format_last_access(last_access):
    elapsed = int(time.monotonic() - last_access)
    return format_time(elapsed)


def format_time(elapsed):
    modulos = (60, 60, 24)
    suffixes = ("s", "m", "h", "d")

    for modulo, suffix in zip(modulos, suffixes):
        if elapsed < modulo:
            return f"{elapsed}{suffix}"
        elapsed //= modulo

    last_suffix = suffixes[-1]
    return f"{elapsed}{last_suffix}"


COLUMN_FORMATTING = Row(
    top_pid=lambda x: x,
    command=lambda x: x,
    mount=lambda x: x,
    fuse_reads=lambda x: x,
    fuse_writes=lambda x: x,
    fuse_total=lambda x: x,
    fuse_backing_store_imports=lambda x: x,
    fuse_last_access=format_last_access,
)
