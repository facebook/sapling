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
from enum import Enum
from typing import Any, Dict, List, Optional, Tuple

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

DEFAULT_COLOR_PAIR = 0  # white text, black background
COLOR_SELECTED = 1
LOWER_WARN_THRESHOLD_COLOR = 2
UPPER_WARN_THRESHOLD_COLOR = 3


class RequestStage(Enum):
    PENDING = "pending"
    LIVE = "live"


class ImportObject(Enum):
    BLOB = "blob"
    TREE = "tree"
    PREFETCH = "prefetch"


class RequestMetric(Enum):
    COUNT = "count"
    MAX_DURATION = "max_duration_us"


STATS_NOT_AVAILABLE = -1
STATS_NOT_IMPLEMENTED = -2

IMPORT_TIME_LOWER_WARN_THRESHOLD = 10  # seconds
IMPORT_TIME_UPPER_WARN_THRESHOLD = 30  # seconds


class Window:
    """
    Class to manage writing to the screen.
    To write to the screen: reset, write lines from top down, and refresh

      ```
      window.reset()

      window.write_line(...)
      ...
      window.write_line(...)

      window.refresh()
      ```
    """

    def __init__(self, stdscr, refresh_rate: int) -> None:
        self.stdscr = stdscr
        self.refresh_rate = refresh_rate
        self.stdscr.timeout(self.refresh_rate * 1000)

        self.height = 0
        self.width = 0
        self._update_screen_size()

        self.current_line = 0  # current line printed
        self.current_x_offset = 0  # horizontal position up to which
        # has been printed on the current line

    def _update_screen_size(self) -> None:
        self.height, self.width = self.stdscr.getmaxyx()

    def screen_resize(self) -> None:
        self._update_screen_size()
        self.stdscr.redrawwin()

    def reset(self) -> None:
        self.stdscr.erase()
        self.current_line = 0
        self.current_x_offset = 0

    def refresh(self) -> None:
        self.stdscr.refresh()

    def get_height(self) -> int:
        return self.height

    def get_width(self) -> int:
        return self.width

    def get_remaining_rows(self) -> int:
        return self.height - self.current_line

    def get_remaining_columns(self) -> int:
        return self.width - self.current_x_offset

    def write_new_line(self, scollable_ended: bool = False) -> None:
        self.current_line += 1
        self.current_x_offset = 0

    # note: this will start writing at what ever the `current_x_offset` is, it
    # will not start writing on a new line, adds a new line to the end of the
    # line printed
    def write_line(
        self, line: str, max_width: int = -1, attr: Optional[int] = None
    ) -> None:
        if max_width == -1:
            max_width = self.width - self.current_x_offset
        self._write(self.current_line, self.current_x_offset, line, max_width, attr)
        self.write_new_line()

    # prints starting from the `current_x_offset`, does NOT add a newline
    def write_part_of_line(
        self, part, max_width: int, attr: Optional[int] = None
    ) -> None:
        self._write(self.current_line, self.current_x_offset, part, max_width, attr)
        self.current_x_offset += min(max_width, len(part))

    # prints a line with the line right justified, adds a new line after printing
    # the line
    def write_line_right_justified(
        self, line, max_width: int, attr: Optional[int] = None
    ) -> None:
        max_width = min(max_width, self.width - self.current_x_offset)
        width = min(max_width, len(line))
        x = self.width - width
        self._write(self.current_line, x, line, max_width, attr)
        self.write_new_line()

    def _write(
        self, y: int, x: int, text: str, max_width: int, attr: Optional[int] = None
    ) -> None:
        try:
            if attr is None:
                attr = DEFAULT_COLOR_PAIR
            self.stdscr.addnstr(y, x, text, max_width, attr)
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

    def get_keypress(self) -> int:
        return int(self.stdscr.getch())


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

        self.pending_imports = {}
        self.fuse_requests_summary = {}

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
            window = Window(stdscr, self.refresh_rate)

            self.curses.curs_set(0)

            self.curses.use_default_colors()
            self.curses.init_pair(COLOR_SELECTED, self.curses.COLOR_GREEN, -1)
            self.curses.init_pair(
                LOWER_WARN_THRESHOLD_COLOR, self.curses.COLOR_YELLOW, -1
            )
            self.curses.init_pair(UPPER_WARN_THRESHOLD_COLOR, self.curses.COLOR_RED, -1)

            while self.running:
                self.update(client)
                self.render(window)
                self.get_keypress(window)

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
        counter_regex = r"((store\.hg.*)|(fuse\.([^\.]*)\..*requests.*))"
        counters = client.getRegexCounters(counter_regex)

        self.pending_imports = self._update_import_stats(counters)
        self.fuse_requests_summary = self._update_fuse_request_stats(counters)

    def _update_import_stats(self, counters):
        import_stats = {}
        for import_stage in RequestStage:
            import_stats[import_stage] = {}
            for import_type in ImportObject:
                import_stats[import_stage][import_type] = {}
                stage_counter_piece = f"{import_stage.value}_import"
                type_counter_piece = import_type.value
                counter_prefix = f"store.hg.{stage_counter_piece}.{type_counter_piece}"
                number_requests = counters.get(
                    f"{counter_prefix}.count", STATS_NOT_AVAILABLE
                )
                longest_outstanding_request = counters.get(
                    f"{counter_prefix}.max_duration_us", STATS_NOT_AVAILABLE
                )  # us
                longest_outstanding_request = (
                    STATS_NOT_AVAILABLE
                    if longest_outstanding_request == STATS_NOT_AVAILABLE
                    else (longest_outstanding_request / 1000000)
                )  # s
                import_stats[import_stage][import_type][
                    RequestMetric.COUNT
                ] = number_requests
                import_stats[import_stage][import_type][
                    RequestMetric.MAX_DURATION
                ] = longest_outstanding_request
        return import_stats

    def _update_fuse_request_stats(self, counters):
        # collect all the counters for each stage and metric -- this is
        # needed since the mount name is part of the counter name
        # and this should work no matter what the mount is
        raw_metrics = {
            stage: {metric: [] for metric in RequestMetric} for stage in RequestStage
        }
        for key in counters:
            pieces = self.parse_fuse_sumary_counter_name(key)
            if pieces is None:
                continue
            stage = pieces[0]
            metric = pieces[1]

            raw_metrics[stage][metric].append(counters[key])

        fuse_requests_stats = {}
        # combine the counters for each stage and metric
        for stage in RequestStage:
            fuse_requests_stats[stage] = {}
            for metric in RequestMetric:
                if raw_metrics[stage][metric] == []:
                    fuse_requests_stats[stage][metric] = STATS_NOT_AVAILABLE
                    continue

                if metric == RequestMetric.COUNT:
                    fuse_requests_stats[stage][metric] = sum(raw_metrics[stage][metric])
                elif metric == RequestMetric.MAX_DURATION:
                    fuse_requests_stats[stage][metric] = (
                        max(raw_metrics[stage][metric]) / 1000000
                    )  # s
                else:
                    raise Exception(f"Aggregation not implemented for: {metric.value}")

        fuse_requests_stats[RequestStage.PENDING][
            RequestMetric.MAX_DURATION
        ] = STATS_NOT_IMPLEMENTED
        return fuse_requests_stats

    # fuse summary counters have the form:
    # fuse.mount.stage.metric
    # returns a tuple of the parsed stage and metric if this counter
    # fits this form and None otherwise
    def parse_fuse_sumary_counter_name(
        self, counter_name: str
    ) -> Optional[Tuple[RequestStage, RequestMetric]]:
        pieces = counter_name.split(".")
        if pieces[0] != "fuse":
            return None

        raw_stage = pieces[2]
        raw_stage = raw_stage.split("_")[0]

        try:
            stage = RequestStage(raw_stage)
            metric = RequestMetric(pieces[3])
            return (stage, metric)
        except KeyError:
            return None

    def render(self, window: Window) -> None:
        window.reset()

        self.render_top_bar(window)
        window.write_new_line()
        self.render_summary_section(window)
        # TODO: daemon memory/inode stats on line 2
        self.render_column_titles(window)

        self.render_rows(window)

        window.refresh()

    def render_top_bar(self, window: Window) -> None:
        width = window.get_width()
        TITLE = "eden top"
        hostname = socket.gethostname()[:width]
        date = datetime.datetime.now().strftime("%x %X")[:width]
        extra_space = width - len(TITLE + hostname + date)

        # left: title
        window.write_part_of_line(TITLE + " " * (extra_space // 2), width)
        if extra_space >= 0:
            # center: date
            window.write_part_of_line(date, width)
            # right: hostname
            window.write_line_right_justified(hostname, width)

    def render_summary_section(self, window: Window) -> None:
        len_longest_stage = max(
            len(self.get_display_name_for_import_stage(stage)) for stage in RequestStage
        )

        fuse_request_header = "outstanding fuse requests:"
        window.write_line(
            fuse_request_header, window.get_width(), self.curses.A_UNDERLINE
        )
        self.render_fuse_request_section(window, len_longest_stage)

        imports_header = "outstanding object imports:"
        window.write_line(imports_header, window.get_width(), self.curses.A_UNDERLINE)
        self.render_import_section(window, len_longest_stage)

    def render_fuse_request_section(self, window, len_longest_stage):
        section_size = window.get_width() // 2
        separator = ""
        for stage in RequestStage:
            window.write_part_of_line(separator, len(separator))
            self.render_fuse_request_part(
                window, stage, len_longest_stage, section_size - len(separator)
            )

            separator = "  |  "
        window.write_new_line()

    def render_fuse_request_part(
        self, window, import_stage, len_longest_stage, section_size
    ):
        stage_display = self.get_display_name_for_import_stage(import_stage)
        header = f"{stage_display:<{len_longest_stage}} -- "
        window.write_part_of_line(header, len(header))

        self.render_request_metrics(
            window, self.fuse_requests_summary[import_stage], section_size - len(header)
        )

    def render_import_section(self, window, len_longest_stage):
        for import_stage in RequestStage:
            self.render_import_row(window, import_stage, len_longest_stage)

    def render_import_row(
        self, window: Window, import_stage: RequestStage, len_longest_stage: int
    ) -> None:
        width = window.get_width()
        stage_display = self.get_display_name_for_import_stage(import_stage)
        header = f"{stage_display:<{len_longest_stage}} -- "
        window.write_part_of_line(header, len(header))

        whole_mid_section_size = width - len(header)
        mid_section_size = whole_mid_section_size // 3

        separator = ""
        for import_type in ImportObject:
            label = f"{separator}{import_type.value}: "
            window.write_part_of_line(label, len(label))

            self.render_request_metrics(
                window,
                self.pending_imports[import_stage][import_type],
                mid_section_size - len(label),
            )

            separator = " | "
        window.write_new_line()

    def render_request_metrics(self, window, metrics, section_size):
        # split the section between the number of imports and
        # the duration of the longest import
        request_count_size = section_size // 2
        request_time_size = section_size - request_count_size

        # number of requests
        requests_for_type = metrics[RequestMetric.COUNT]
        if requests_for_type == STATS_NOT_AVAILABLE:
            requests_for_type = "N/A"
        elif requests_for_type == STATS_NOT_IMPLEMENTED:
            requests_for_type = ""
        requests_for_type_display = f"{requests_for_type:>{request_count_size-1}} "

        # duration of the longest request
        longest_request = metrics[RequestMetric.MAX_DURATION]  # s
        color = self._get_color_for_pending_import_display(longest_request)
        if longest_request == STATS_NOT_AVAILABLE:
            longest_request_display = "N/A"
            # wrap time in parens
            longest_request_display = f"({longest_request_display})"
        elif longest_request == STATS_NOT_IMPLEMENTED:
            longest_request_display = ""
        else:
            # 3 places after the decimal
            longest_request_display = f"{longest_request:.3f}"
            # set the maximum number of digits, will remove decimals if
            # not enough space & adds unit
            longest_request_display = (
                f"{longest_request_display:.{request_time_size-3}s}s"
            )
            # wrap time in parens
            longest_request_display = f"({longest_request_display})"

        # right align
        longest_request_display = f"{longest_request_display:>{request_time_size}}"

        request_display = f"{requests_for_type_display}{longest_request_display}"
        window.write_part_of_line(request_display, section_size, color)

    def get_display_name_for_import_stage(self, import_stage: RequestStage) -> str:
        if import_stage == RequestStage.PENDING:
            return "total pending"
        else:
            return str(import_stage.value)

    def _get_color_for_pending_import_display(
        self, longest_request: float
    ) -> Optional[int]:
        if longest_request == 0 or longest_request < IMPORT_TIME_LOWER_WARN_THRESHOLD:
            return None
        elif longest_request < IMPORT_TIME_UPPER_WARN_THRESHOLD:
            return int(self.curses.color_pair(LOWER_WARN_THRESHOLD_COLOR))
        else:
            return int(self.curses.color_pair(UPPER_WARN_THRESHOLD_COLOR))

    def render_column_titles(self, window: Window) -> None:
        self.render_row(window, COLUMN_TITLES, self.curses.A_REVERSE)

    def render_rows(self, window: Window) -> None:
        aggregated_processes: Dict[int, Process] = {}
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

        for process in sorted_processes:
            row = process.get_row()
            row = (fmt(data) for fmt, data in zip(COLUMN_FORMATTING, row))

            style = self.curses.A_BOLD if process.is_running else self.curses.A_NORMAL
            self.render_row(window, row, style)

    def render_row(self, window: Window, row: Row, style) -> None:

        row_data = zip(row, COLUMN_ALIGNMENT, COLUMN_SPACING)
        for i, (raw_text, align, space) in enumerate(row_data):
            remaining_space = window.get_remaining_columns()
            if remaining_space <= 0:
                break

            space = min(space, remaining_space)
            if i == len(COLUMN_SPACING) - 1:
                space = max(space, remaining_space)

            text = f"{raw_text:{align}{space}} "

            color = DEFAULT_COLOR_PAIR
            if i == self.selected_column:
                color = self.curses.color_pair(COLOR_SELECTED)
            window.write_part_of_line(text, space + 1, color | style)
        window.write_new_line()

    def get_keypress(self, window: Window) -> None:
        key = window.get_keypress()
        if key == self.curses.KEY_RESIZE:
            self.curses.update_lines_cols()
            window.screen_resize()
        elif key == ord("q"):
            self.running = False
        elif key == self.curses.KEY_LEFT:
            self.move_selector(-1)
        elif key == self.curses.KEY_RIGHT:
            self.move_selector(1)

    def move_selector(self, dx: int) -> None:
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
