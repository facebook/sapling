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
import shlex
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

DEFAULT_COLOR_PAIR = 0  # white text, black background
COLOR_SELECTED = 1
LOWER_WARN_THRESHOLD_COLOR = 2
UPPER_WARN_THRESHOLD_COLOR = 3

IMPORT_TYPES = ["blob", "tree", "prefetch"]
IMPORT_TIME_LOWER_WARN_THRESHOLD = 10  # seconds
IMPORT_TIME_UPPER_WARN_THRESHOLD = 30  # seconds


class Top:
    def __init__(self):
        import curses

        self.curses = curses

        self.running = False
        self.ephemeral = False
        self.refresh_rate = 1
        self.current_line = 0  # current line printed
        self.current_x_offset = 0  # horizontal position up to which
        # has been printed on the current line

        # Processes are stored by PID
        self.processes: Dict[int, Process] = {}
        self.rows: List = []
        self.selected_column = COLUMN_TITLES.index("FUSE LAST")

        self.height = 0
        self.width = 0

        self.pending_imports = {import_type: {} for import_type in IMPORT_TYPES}

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
            self.curses.init_pair(
                LOWER_WARN_THRESHOLD_COLOR, self.curses.COLOR_YELLOW, -1
            )
            self.curses.init_pair(UPPER_WARN_THRESHOLD_COLOR, self.curses.COLOR_RED, -1)

            while self.running:
                self.update(client)
                self.render(stdscr)
                self.get_keypress(stdscr)

        return mainloop

    def update(self, client):
        if self.ephemeral:
            self.processes.clear()

        self._update_summary_stats(client)
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

    def _update_summary_stats(self, client):
        client.flushStatsNow()
        counters = client.getCounters()
        for import_type in IMPORT_TYPES:
            number_requests = counters.get(
                f"store.hg.pending_import.{import_type}.count", -1
            )
            longest_outstanding_request = counters.get(
                f"store.hg.pending_import.{import_type}.max_duration_us", -1
            )  # us
            longest_outstanding_request = (
                -1
                if longest_outstanding_request == -1
                else (longest_outstanding_request / 1000000)
            )  # s
            self.pending_imports[import_type]["number_requests"] = number_requests
            self.pending_imports[import_type][
                "max_request_duration"
            ] = longest_outstanding_request

    def render(self, stdscr):
        stdscr.erase()
        self.reset()

        self.render_top_bar(stdscr)
        self.write_new_line()
        self.render_summary_section(stdscr)
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
        self.write_part_of_line(stdscr, TITLE + " " * (extra_space // 2), self.width)
        if extra_space >= 0:
            # center: date
            self.write_part_of_line(stdscr, date, self.width)
            # right: hostname
            self.write_line_right_justified(stdscr, hostname, self.width)

    def render_summary_section(self, stdscr):
        imports_header = "outstanding object imports:"
        self.write_line(stdscr, imports_header, len(imports_header))

        mid_section_size = self.width // 3

        separator = ""
        for import_type in IMPORT_TYPES:
            label = f"{separator}{import_type}: "
            self.write_part_of_line(stdscr, label, len(label))

            # split the section between the number of imports and
            # the duration of the longest import
            import_metric_size = mid_section_size - len(label)
            import_count_size = import_metric_size // 2
            import_time_size = import_metric_size - import_count_size

            # number of imports
            imports_for_type = self.pending_imports[import_type]["number_requests"]
            if imports_for_type == -1:
                imports_for_type = "N/A"
            imports_for_type_display = f"{imports_for_type:>{import_count_size-1}} "

            # duration of the longest import
            longest_request = self.pending_imports[import_type][
                "max_request_duration"
            ]  # us
            color = self._get_color_for_pending_import_display(longest_request)
            if longest_request == -1:
                longest_request_display = "N/A"
            else:
                # 3 places after the decimal
                longest_request_display = f"{longest_request:.3f}"
                # set the maximum number of digits, will remove decimals if
                # not enough space & adds unit
                longest_request_display = (
                    f"{longest_request_display:.{import_time_size-3}s}s"
                )
            # wrap time in parens
            longest_request_display = f"({longest_request_display})"
            # right align
            longest_request_display = f"{longest_request_display:>{import_time_size}}"

            import_display = f"{imports_for_type_display}{longest_request_display}"
            self.write_part_of_line(stdscr, import_display, import_metric_size, color)

            separator = " | "
        self.write_new_line()

    def _get_color_for_pending_import_display(self, longest_request) -> Optional[int]:
        if longest_request == 0 or longest_request < IMPORT_TIME_LOWER_WARN_THRESHOLD:
            return None
        elif longest_request < IMPORT_TIME_UPPER_WARN_THRESHOLD:
            return self.curses.color_pair(LOWER_WARN_THRESHOLD_COLOR)
        else:
            return self.curses.color_pair(UPPER_WARN_THRESHOLD_COLOR)

    def render_column_titles(self, stdscr):
        self.render_row(stdscr, COLUMN_TITLES, self.curses.A_REVERSE)

    def render_rows(self, stdscr):
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

        for process in sorted_processes[: self.height]:
            row = process.get_row()
            row = (fmt(data) for fmt, data in zip(COLUMN_FORMATTING, row))

            style = self.curses.A_BOLD if process.is_running else self.curses.A_NORMAL
            self.render_row(stdscr, row, style)

    def render_row(self, stdscr, row, style):

        row_data = zip(row, COLUMN_ALIGNMENT, COLUMN_SPACING)
        for i, (raw_text, align, space) in enumerate(row_data):
            remaining_space = self.width - self.current_x_offset
            if remaining_space <= 0:
                break

            space = min(space, remaining_space)
            if i == len(COLUMN_SPACING) - 1:
                space = max(space, remaining_space)

            text = f"{raw_text:{align}{space}} "

            color = DEFAULT_COLOR_PAIR
            if i == self.selected_column:
                color = self.curses.color_pair(COLOR_SELECTED)
            self.write_part_of_line(stdscr, text, space + 1, color | style)
        self.write_new_line()

    def reset(self) -> None:
        self.current_line = 0
        self.current_x_offset = 0

    def write_new_line(self) -> None:
        self.current_line += 1
        self.current_x_offset = 0

    # note: this will start writing at what ever the `current_x_offset` is, it
    # will not start writing on a new line, adds a new line to the end of the
    # line printed
    def write_line(
        self, window, line: str, max_width: int, attr: Optional[int] = None
    ) -> None:
        self._write(
            window, self.current_line, self.current_x_offset, line, max_width, attr
        )
        self.write_new_line()

    # prints starting from the `current_x_offset`, does NOT add a newline
    def write_part_of_line(
        self, window, part, max_width: int, attr: Optional[int] = None
    ) -> None:
        self._write(
            window, self.current_line, self.current_x_offset, part, max_width, attr
        )
        self.current_x_offset += min(max_width, len(part))

    # prints a line with the line right justified, adds a new line after printing
    # the line
    def write_line_right_justified(
        self, window, line, max_width: int, attr: Optional[int] = None
    ) -> None:
        max_width = min(max_width, self.width - self.current_x_offset)
        width = min(max_width, len(line))
        x = self.width - width
        self._write(window, self.current_line, x, line, max_width, attr)
        self.write_new_line()

    def _write(
        self,
        window: Any,
        y: int,
        x: int,
        text: str,
        max_width: int,
        attr: Optional[int] = None,
    ) -> None:
        try:
            if attr is None:
                attr = DEFAULT_COLOR_PAIR
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
    args = os.fsdecode(cmd).split("\x00")

    # Focus on just the basename as the paths can be quite long
    cmd = args[0]
    if os.path.isabs(cmd):
        cmd = os.path.basename(cmd)

    # Show cmdline args too, if they exist
    return " ".join(shlex.quote(p) for p in [cmd] + args[1:])


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
