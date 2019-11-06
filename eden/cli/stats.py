#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import io
import logging
import os
import sys
import textwrap
from typing import Dict, List, Optional, cast

from . import cmd_util, stats_print, subcmd as subcmd_mod
from .config import EdenInstance
from .subcmd import Subcmd


stats_cmd = subcmd_mod.Decorator()

log = logging.getLogger("eden.cli.stats")


DiagInfoCounters = Dict[str, int]
Table = Dict[str, List[int]]
Table2D = Dict[str, List[List[Optional[str]]]]

# TODO: https://github.com/python/typeshed/issues/1240
stdoutWrapper = cast(io.TextIOWrapper, sys.stdout)


# Shows information like memory usage, list of mount points and number of inodes
# loaded, unloaded, and materialized in the mount points, etc.
def do_stats_general(
    instance: EdenInstance, out: io.TextIOWrapper = stdoutWrapper
) -> None:
    with instance.get_thrift_client() as client:
        stat_info = client.getStatInfo()

    private_bytes = stats_print.format_size(stat_info.privateBytes)
    resident_bytes = stats_print.format_size(stat_info.vmRSSBytes)

    if stat_info.blobCacheStats is not None:
        blob_cache_size = stats_print.format_size(
            stat_info.blobCacheStats.totalSizeInBytes
        )
        blob_cache_entry_count = stat_info.blobCacheStats.entryCount
    else:
        blob_cache_size = None
        blob_cache_entry_count = None

    out.write(
        textwrap.dedent(
            f"""\
        edenfs memory usage
        ▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔
        private bytes: {private_bytes} ({resident_bytes} resident)
        """
        )
    )

    if blob_cache_size is not None and blob_cache_entry_count is not None:
        out.write(f"blob cache: {blob_cache_size} in {blob_cache_entry_count} blobs\n")

    out.write(
        textwrap.dedent(
            f"""\

        active mounts
        ▔▔▔▔▔▔▔▔▔▔▔▔▔
    """
        )
    )

    inode_info = stat_info.mountPointInfo
    for key in inode_info:
        info = inode_info[key]
        mount_path = os.fsdecode(key)

        files = info.loadedFileCount
        trees = info.loadedTreeCount
        in_memory = files + trees

        if stat_info.mountPointJournalInfo is None:
            journal = None
        else:
            journal = stat_info.mountPointJournalInfo.get(key)

        if journal is None:
            journalLine = ""
        else:
            entries = journal.entryCount
            mem = journal.memoryUsage
            duration = journal.durationSeconds
            if duration is None:
                journalLine = (
                    f"- Journal: {entries} entries "
                    f"({stats_print.format_size(mem)})\n"
                )
            else:
                journalLine = (
                    f"- Journal: {entries} entries over "
                    f"{stats_print.format_time(duration)} "
                    f"({stats_print.format_size(mem)})\n"
                )
        out.write(
            textwrap.dedent(
                f"""\
            {mount_path}
              - Inodes in memory: {in_memory} ({trees} trees, {files} files)
              - Unloaded, tracked inodes: {info.unloadedInodeCount}
              {journalLine}
            """
            )
        )


@stats_cmd("memory", "Show memory statistics for Eden")
class MemoryCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        out = sys.stdout
        stats_print.write_heading("Memory Stats for EdenFS", out)
        instance = cmd_util.get_eden_instance(args)

        with instance.get_thrift_client() as client:
            counters = client.getCounters()
            stats_print.write_mem_status_table(counters, out)

            # print memory counters
            heading = "Average values of Memory usage and availability"
            out.write("\n\n %s \n\n" % heading.center(80, " "))

            table = get_memory_counters(counters)
            stats_print.write_table(table, "", out)

        return 0


# Returns all the memory counters in ServiceData in a table format.
def get_memory_counters(counters: DiagInfoCounters) -> Table:
    table: Table = {}
    index = {"60": 0, "600": 1, "3600": 2}
    for key in counters:
        if key.startswith("memory") and key.find(".") != -1:
            tokens = key.split(".")
            memKey = tokens[0].replace("_", " ")
            if memKey not in table.keys():
                table[memKey] = [0, 0, 0, 0]
            if len(tokens) == 2:
                table[memKey][3] = counters[key]
            else:
                table[memKey][index[tokens[2]]] = counters[key]
    return table


@stats_cmd("io", "Show information about the number of I/O calls")
class IoCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-A",
            "--all",
            action="store_true",
            default=False,
            help="Show status for all the system calls",
        )

    def run(self, args: argparse.Namespace) -> int:
        out = sys.stdout
        stats_print.write_heading("Counts of I/O operations performed in EdenFs", out)
        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client() as client:
            counters = client.getCounters()

        # If the arguments has --all flag, we will have args.all set to
        # true.
        fuse_counters = get_fuse_counters(counters, args.all)
        stats_print.write_table(fuse_counters, "SystemCall", out)

        return 0


# Filters Fuse counters from all the counters in ServiceData and returns a
# printable form of the information in a table. If all_flg is true we get the
# counters for all the system calls, otherwise we get the counters of the
# system calls which are present in the list syscalls, which is a list of
# frequently called io system calls.
def get_fuse_counters(counters: DiagInfoCounters, all_flg: bool) -> Table:
    table: Table = {}
    index = {"60": 0, "600": 1, "3600": 2}

    # list of io system calls, if all flag is set we return counters for all the
    # systems calls, else we return counters for io systemcalls.
    syscalls = [
        "open",
        "read",
        "write",
        "symlink",
        "readlink",
        "mkdir",
        "mknod",
        "opendir",
        "readdir",
        "rmdir",
    ]

    for key in counters:
        if key.startswith("fuse") and key.find(".count") >= 0:
            tokens = key.split(".")
            syscall = tokens[1][:-3]  # _us
            if not all_flg and syscall not in syscalls:
                continue

            if syscall not in table.keys():
                table[syscall] = [0, 0, 0, 0]
            if len(tokens) == 3:
                table[syscall][3] = int(counters[key])
            else:
                table[syscall][index[tokens[3]]] = int(counters[key])

    return table


def insert_latency_record(
    table: Table2D, value: int, operation: str, percentile: str, period: Optional[str]
) -> None:
    period_table = {"60": 0, "600": 1, "3600": 2}
    percentile_table = {"avg": 0, "p50": 1, "p90": 2, "p99": 3}

    def with_microsecond_units(i: int) -> str:
        if i:
            return str(i) + " \u03BCs"  # mu for micro
        else:
            return str(i) + "   "

    if operation not in table.keys():
        # pyre-ignore[6]: T38220626
        table[operation] = [
            ["" for _ in range(len(percentile_table))]
            for _ in range(len(period_table) + 1)
        ]

    pct_index = percentile_table[percentile]
    if period:
        period_index = period_table[period]
    else:
        period_index = len(period_table)

    table[operation][pct_index][period_index] = with_microsecond_units(value)


@stats_cmd("latency", "Show information about the latency of I/O calls")
class LatencyCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-A",
            "--all",
            action="store_true",
            default=False,
            help="Show status for all the system calls",
        )

    def run(self, args: argparse.Namespace) -> int:
        TITLE = "Latencies of I/O operations performed in EdenFS"
        stats_print.write_heading(TITLE, sys.stdout)

        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client() as client:
            counters = client.getCounters()

        table = get_fuse_latency(counters, args.all)
        stats_print.write_latency_table(table, sys.stdout)

        return 0


# Returns all the latency information in ServiceData in a table format.
# If all_flg is true we get the counters for all the system calls, otherwise we
# get the counters of the system calls which are present in the list syscalls,
# which is a list of frequently called io system calls.
def get_fuse_latency(counters: DiagInfoCounters, all_flg: bool) -> Table2D:
    table: Table2D = {}
    syscalls = [
        "open",
        "read",
        "write",
        "symlink",
        "readlink",
        "mkdir",
        "mknod",
        "opendir",
        "readdir",
        "rmdir",
    ]

    for key in counters:
        if key.startswith("fuse") and key.find(".count") == -1:
            tokens = key.split(".")
            syscall = tokens[1][:-3]
            if not all_flg and syscall not in syscalls:
                continue
            percentile = tokens[2]
            period = None
            if len(tokens) > 3:
                period = tokens[3]
            insert_latency_record(table, counters[key], syscall, percentile, period)

    return table


@stats_cmd(
    "hgimporter",
    "Show the number of requests to hg-debugedenimporthelper",
    aliases=[
        "debugedenimporthelper",
        "hg-debugedenimporthelper",
        "hg",
        "hg-import",
        "hg-importer",
        "hgimport",
    ],
)
class HgImporterCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        TITLE = "Counts of HgImporter requests performed in EdenFS"
        stats_print.write_heading(TITLE, sys.stdout)

        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client() as client:
            counters = client.getCounters()

        table = get_counter_table(counters, ["hg_importer"], ["count"])
        stats_print.write_table(table, "HgImporter Request", sys.stdout)

        return 0


@stats_cmd("thrift", "Show the number of received thrift calls")
class ThriftCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        TITLE = "Counts of Thrift calls performed in EdenFS"
        stats_print.write_heading(TITLE, sys.stdout)

        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client() as client:
            counters = client.getRegexCounters("thrift.EdenService\\..*")

        PREFIX = ["thrift", "EdenService"]
        SUFFIX = ["num_calls", "sum"]
        table = get_counter_table(counters, PREFIX, SUFFIX)
        stats_print.write_table(table, "Thrift Call", sys.stdout)

        return 0


@stats_cmd("thrift-latency", "Show the latency of received thrift calls")
class ThriftLatencyCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        TITLE = "Latency of Thrift processing time performed in EdenFS"
        stats_print.write_heading(TITLE, sys.stdout)

        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client() as client:
            counters = client.getCounters()

        table = get_thrift_latency(counters)
        stats_print.write_latency_table(table, sys.stdout)

        return 0


def get_thrift_latency(counters: DiagInfoCounters) -> Table2D:
    table: Table2D = {}
    for key in counters:
        if key.startswith("thrift.EdenService.") and key.find("time_process_us") != -1:
            tokens = key.split(".")
            if len(tokens) < 5:
                continue
            method = tokens[2]
            percentile = tokens[4]
            period = None
            if len(tokens) > 5:
                period = tokens[5]
            insert_latency_record(table, counters[key], method, percentile, period)
    return table


@stats_cmd("hg-latency", "Show the latency of hg backing store")
class HgBackingStoreLatencyCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        return backing_store_latency("hg", args)


@stats_cmd("mononoke", "Show the latency of mononoke backing store")
class MononokeBackingStoreLatencyCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        return backing_store_latency("mononoke", args)


def backing_store_latency(store: str, args: argparse.Namespace) -> int:
    TITLE = "Latency of {} backing store operations in EdenFs".format(store)
    stats_print.write_heading(TITLE, sys.stdout)

    instance = cmd_util.get_eden_instance(args)
    with instance.get_thrift_client() as client:
        counters = client.getCounters()

    table = get_store_latency(counters, store)
    stats_print.write_latency_table(table, sys.stdout)

    return 0


def get_store_latency(counters: DiagInfoCounters, store: str) -> Table2D:
    table: Table2D = {}

    for key in counters:
        if key.startswith("store.{}".format(store)) and key.find(".count") == -1:
            tokens = key.split(".")
            method = tokens[2]
            percentile = tokens[3]
            period = None
            if len(tokens) > 4:
                period = tokens[4]
            insert_latency_record(table, counters[key], method, percentile, period)
    return table


@stats_cmd("local-store", "Show information about the local store data size")
class LocalStoreCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        # (name, ephemeral)
        column_families = [
            ("blob", True),
            ("blobmeta", True),
            ("hgcommit2tree", True),
            ("tree", False),
            ("hgproxyhash", False),
        ]

        out = sys.stdout
        stats_print.write_heading("EdenFS Local Store Stats", out)

        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client() as client:
            counters = client.getRegexCounters("local_store\\..*")

        columns = ("Table", "Ephemeral?", "Size")
        fmt = "{:<16} {:>10} {:>15}\n"
        out.write(fmt.format(*columns))
        out.write(f"-------------------------------------------\n")
        for name, ephemeral in column_families:
            size = stats_print.format_size(counters.get(f"local_store.{name}.size", 0))
            out.write(fmt.format(name, "Y" if ephemeral else "N", size))

        ephemeral_size = stats_print.format_size(
            counters.get("local_store.ephemeral.total_size", 0)
        )
        persistent_size = stats_print.format_size(
            counters.get("local_store.persistent.total_size", 0)
        )
        out.write("\n")
        out.write(f"Total Ephemeral Size:  {ephemeral_size:>20}\n")
        out.write(f"Total Persistent Size: {persistent_size:>20}\n")
        out.write("\n")

        out.write("Automatic Garbage Collection Data:\n")
        out.write(f"-------------------------------------------\n")
        auto_gc_running = bool(counters.get("local_store.auto_gc.running", 0))
        out.write(f"Auto-GC In Progress:      {'Y' if auto_gc_running else 'N'}\n")
        auto_gc_success = counters.get("local_store.auto_gc.success", 0)
        out.write(f"Successful Auto-GC Runs:  {auto_gc_success}\n")
        auto_gc_failure = counters.get("local_store.auto_gc.failure", 0)
        out.write(f"Failed Auto-GC Runs:      {auto_gc_failure}\n")
        last_gc_success = counters.get("local_store.auto_gc.last_run_succeeded", None)
        if last_gc_success is not None:
            last_gc_ms = counters.get("local_store.auto_gc.last_duration_ms", 0)
            last_gc_sec = last_gc_ms / 1000

            last_result_str = "Success" if last_gc_success == 1 else "Failure"
            out.write(f"Last Auto-GC Result:      {last_result_str}\n")
            out.write(f"Last Auto-GC Duration:    {last_gc_sec:.03f}s\n")

        return 0


@stats_cmd("object-store", "Show information about the object store pipeline")
class ObjectStoreCommand(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        TITLE = "Percentages of where data was found by the object store"
        stats_print.write_heading(TITLE, sys.stdout)

        eden = cmd_util.get_eden_instance(args)
        with eden.get_thrift_client() as thrift:
            counters = thrift.getRegexCounters("object_store\\..*")

        table = get_counter_table(counters, ["object_store"], ["pct"])
        stats_print.write_table(table, "Object Store", sys.stdout)

        return 0


def get_counter_table(counters: DiagInfoCounters, prefix: List, suffix: List) -> Table:
    table: Table = {}

    for key in counters:
        tags = key.split(".")
        if tags[-len(suffix) :] == suffix and tags[0 : len(prefix)] == prefix:
            TIME_SUFFIXES = (".60", ".600", ".3600", "")
            row_name = ".".join(tags[len(prefix) : -len(suffix)])
            table[row_name] = [counters[key + suffix] for suffix in TIME_SUFFIXES]

    return table


class StatsCmd(Subcmd):
    NAME = "stats"
    HELP = "Prints statistics information for eden"

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        self.add_subcommands(parser, stats_cmd.commands)

    def run(self, args: argparse.Namespace) -> int:
        instance = cmd_util.get_eden_instance(args)
        do_stats_general(instance)
        return 0
