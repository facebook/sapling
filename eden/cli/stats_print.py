#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

# Helper function to print the heading of a Stat Call.

from typing import TextIO


def write_heading(heading: str, out: TextIO) -> None:
    format_str = '{:^80}\n'
    border = '*' * len(heading)
    out.write(format_str.format(border))
    out.write(format_str.format(heading))
    out.write(format_str.format(border) + '\n')


def write_mem_status_table(fuse_counters, out: TextIO) -> None:
    format_str = '{:>40} {:^1} {:<20}\n'
    keys = [
        'memory_free', 'memory_free_percent', 'memory_usage',
        'memory_usage_percent'
    ]
    for key in keys:
        if key.endswith('_percent'):
            value = '%d%s' % (fuse_counters[key], '%')
        else:
            value = '%f(GB)' % (fuse_counters[key] / (10**6))
        out.write(format_str.format(key.replace('_', ' '), ':', value))


# Prints a record of latencies with 50'th,90'th and 99'th percentile.
def write_latency_record(syscall: str, matrix, out: TextIO) -> None:
    border = '-' * 80
    format_str = '{:^12} {:^4} {:^10}  {:^10}  {:^15}  {:^10} {:^10}\n'
    percentile = {0: 'P50', 1: 'p90', 2: 'p99'}

    for i in range(len(percentile)):
        syscall_name = ''
        if i == int(len(percentile) / 2):
            syscall_name = syscall
        out.write(
            format_str.format(
                syscall_name, '|', percentile[i], matrix[i][0], matrix[i][1],
                matrix[i][2], matrix[i][3]
            )
        )
    out.write(border + '\n')


def write_latency_table(table, out: TextIO) -> None:
    format_str = '{:^12} {:^4} {:^10}  {:^10}  {:^15}  {:^10} {:^10}\n'
    out.write(
        format_str.format(
            'SystemCall', '|', 'Percentile', 'Last Minute', 'Last 10 Minutes',
            'Last Hour', 'All Time'
        )
    )
    border = '-' * 80
    out.write(border + '\n')
    for key in table:
        write_latency_record(key, table[key], out)


def write_table(table, heading: str, out: TextIO) -> None:
    format_str = '{:^20}{:^15}{:^15}{:^15}{:^15}\n'
    out.write(
        format_str.format(
            heading, 'Last minute', 'Last 10 minutes', 'Last Hour', 'All Time'
        )
    )
    border = '-' * 80
    out.write(border + '\n')
    for key in table:
        value = table[key]
        out.write(
            format_str.format(key, value[0], value[1], value[2], value[3])
        )
