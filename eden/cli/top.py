#!/usr/bin/env python3
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
from typing import Any, Dict, List, Optional

from facebook.eden.ttypes import AccessCounts

from . import cmd_util


Row = collections.namedtuple(
    "Row",
    "top_pid mount fuse_reads fuse_writes fuse_total fuse_backing_store_imports fuse_duration fuse_last_access command",
)

COLUMN_TITLES = Row(
    top_pid="TOP PID",
    mount="MOUNT",
    fuse_reads="FUSE R",
    fuse_writes="FUSE W",
    fuse_total="FUSE COUNT",
    fuse_backing_store_imports="IMPORTS",
    fuse_duration="FUSE TIME",
    fuse_last_access="FUSE LAST",
    command="CMD",
)
COLUMN_SPACING = Row(
    top_pid=7,
    mount=12,
    fuse_reads=10,
    fuse_writes=10,
    fuse_total=10,
    fuse_backing_store_imports=10,
    fuse_duration=10,
    fuse_last_access=10,
    command=25,
)
COLUMN_ALIGNMENT = Row(
    top_pid=">",
    mount="<",
    fuse_reads=">",
    fuse_writes=">",
    fuse_total=">",
    fuse_backing_store_imports=">",
    fuse_duration=">",
    fuse_last_access=">",
    command="<",
)
COLUMN_REVERSE_SORT = Row(
    top_pid=False,
    mount=False,
    fuse_reads=True,
    fuse_writes=True,
    fuse_total=True,
    fuse_backing_store_imports=True,
    fuse_duration=True,
    fuse_last_access=True,
    command=False,
)

COLOR_SELECTED = 1


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
        self.selected_column = COLUMN_TITLES.index("FUSE LAST")

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

            self.curses.use_default_colors()
            self.curses.init_pair(COLOR_SELECTED, self.curses.COLOR_GREEN, -1)

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
                    cmd = counts.cmdsByPid.get(pid, b"<kernel>")
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
        self._write(stdscr, 0, 0, TITLE, self.width)
        if extra_space >= 0:
            # center: date
            self._write(stdscr, 0, len(TITLE) + extra_space // 2, date, self.width)
            # right: hostname
            self._write(stdscr, 0, self.width - len(hostname), hostname, self.width)

    def render_column_titles(self, stdscr):
        LINE = 2
        self._write(
            stdscr, LINE, 0, " " * self.width, self.width, self.curses.A_REVERSE
        )
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
            reverse=COLUMN_REVERSE_SORT[self.selected_column],
        )

        for line, process in zip(line_numbers, sorted_processes):
            row = process.get_row()
            row = (fmt(data) for fmt, data in zip(COLUMN_FORMATTING, row))

            style = self.curses.A_BOLD if process.is_running else self.curses.A_NORMAL
            self.render_row(stdscr, line, row, style)

    def render_row(self, stdscr, y, row, style):
        x = 0

        row_data = zip(row, COLUMN_ALIGNMENT, COLUMN_SPACING)
        for i, (str, align, space) in enumerate(row_data):
            remaining_space = self.width - x
            if remaining_space <= 0:
                break

            space = min(space, remaining_space)
            if i == len(COLUMN_SPACING) - 1:
                space = max(space, remaining_space)

            text = f"{str:{align}{space}}"

            color = 0
            if i == self.selected_column:
                color = self.curses.color_pair(COLOR_SELECTED)

            self._write(stdscr, y, x, text, space, color | style)
            x += space + 1

    def _write(
        self,
        window: Any,
        y: int,
        x: int,
        text: str,
        max_width: int,
        attr: Optional[int] = 0,
    ) -> None:
        try:
            window.addnstr(y, x, text, max_width, attr)
        except Exception as ex:
            # When attempting to write to the very last terminal cell curses will
            # successfully display the data but will return an error since the logical
            # cursor cannot be advanced to the next cell.
            #
            # We just ignore the error to handle this case.
            # If you do want to look at errors during development you can enable the
            # following code, but note that the error messages from the curses module
            # usually are not very informative.
            if False:
                with open("/tmp/eden_top.log", "a") as f:
                    f.write(f"error at ({y}, {x}): {ex}\n")

    def get_keypress(self, stdscr):
        key = stdscr.getch()
        if key == self.curses.KEY_RESIZE:
            self.curses.update_lines_cols()
            stdscr.redrawwin()
            self.height, self.width = stdscr.getmaxyx()
        elif key == ord("q"):
            self.running = False
        elif key == self.curses.KEY_LEFT:
            self.move_selector(-1)
        elif key == self.curses.KEY_RIGHT:
            self.move_selector(1)

    def move_selector(self, dx):
        self.selected_column = (self.selected_column + dx) % len(COLUMN_TITLES)


class Process:
    def __init__(self, pid, cmd, mount):
        self.pid = pid
        self.cmd = format_cmd(cmd)
        self.mount = format_mount(mount)
        self.access_counts = AccessCounts(0, 0, 0, 0, 0)
        self.last_access_time = time.monotonic()
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
        self.access_counts.fuseDurationNs += access_counts.fuseDurationNs

    def get_row(self):
        return Row(
            top_pid=self.pid,
            mount=self.mount,
            fuse_reads=self.access_counts.fuseReads,
            fuse_writes=self.access_counts.fuseWrites,
            fuse_total=self.access_counts.fuseTotal,
            fuse_backing_store_imports=self.access_counts.fuseBackingStoreImports,
            fuse_duration=self.access_counts.fuseDurationNs,
            fuse_last_access=self.last_access,
            command=self.cmd,
        )


def format_mount(mount):
    return os.fsdecode(os.path.basename(mount))


def format_duration(duration):
    modulos = (1000, 1000, 1000, 60, 60, 24)
    suffixes = ("ns", "us", "ms", "s", "m", "h", "d")
    return format_time(duration, modulos, suffixes)


def format_last_access(last_access):
    elapsed = int(time.monotonic() - last_access)

    modulos = (60, 60, 24)
    suffixes = ("s", "m", "h", "d")
    return format_time(elapsed, modulos, suffixes)


def format_time(elapsed, modulos, suffixes):
    for modulo, suffix in zip(modulos, suffixes):
        if elapsed < modulo:
            return f"{elapsed}{suffix}"
        elapsed //= modulo

    last_suffix = suffixes[-1]
    return f"{elapsed}{last_suffix}"


def format_cmd(cmd):
    args = os.fsdecode(cmd).split("\x00", 1)

    # Focus on just the basename as the paths can be quite long
    cmd = args[0]
    if os.path.isabs(cmd):
        cmd = os.path.basename(cmd)

    # Show cmdline args too, if they exist
    if len(args) > 1:
        arg_str = args[1].replace("\x00", " ")
        cmd += f" {arg_str}"

    return cmd


COLUMN_FORMATTING = Row(
    top_pid=lambda x: x,
    mount=lambda x: x,
    fuse_reads=lambda x: x,
    fuse_writes=lambda x: x,
    fuse_total=lambda x: x,
    fuse_backing_store_imports=lambda x: x,
    fuse_duration=format_duration,
    fuse_last_access=format_last_access,
    command=lambda x: x,
)
