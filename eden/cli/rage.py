#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import getpass
import io
import socket
import subprocess
import traceback
from pathlib import Path
from typing import IO

from . import (
    debug as debug_mod,
    doctor as doctor_mod,
    filesystem,
    mtab,
    process_finder,
    stats as stats_mod,
    ui as ui_mod,
)
from .config import EdenInstance


def print_diagnostic_info(instance: EdenInstance, out: IO[bytes]) -> None:
    out.write(b"User                    : %s\n" % getpass.getuser().encode())
    out.write(b"Hostname                : %s\n" % socket.gethostname().encode())
    print_rpm_version(out)

    health_status = instance.check_health()
    if health_status.is_healthy():
        out.write(b"\n")
        debug_mod.do_buildinfo(instance, out)
        out.write(b"uptime: ")
        debug_mod.do_uptime(instance, out)

    print_eden_doctor_report(instance, out)
    print_tail_of_log_file(instance.get_log_path(), out)
    print_running_eden_process(out)

    if health_status.is_healthy() and health_status.pid is not None:
        # pyre-fixme[6]: Expected `int` for 1st param but got `Optional[int]`.
        print_edenfs_process_tree(health_status.pid, out)

    out.write(b"\nList of mount points:\n")
    mountpoint_paths = []
    for key in sorted(instance.get_mount_paths()):
        key_bytes = key.encode()
        out.write(key_bytes)
        mountpoint_paths.append(key_bytes)
    for key, val in instance.get_all_client_config_info().items():
        out.write(b"\nMount point info for path %s:\n" % key.encode())
        for k, v in val.items():
            out.write("{:>10} : {}\n".format(k, v).encode())
    if health_status.is_healthy():
        with io.StringIO() as stats_stream:
            stats_mod.do_stats_general(instance, out=stats_stream)
            out.write(stats_stream.getvalue().encode())


def print_rpm_version(out: IO[bytes]) -> None:
    try:
        queryformat = "%{VERSION}"
        output = subprocess.check_output(["rpm", "-q", "--qf", queryformat, "fb-eden"])
        out.write(b"Rpm Version             : %s\n" % output)
    except Exception as e:
        out.write(b"Error getting the Rpm version : %s\n" % str(e).encode())


def print_eden_doctor_report(instance: EdenInstance, out: IO[bytes]) -> None:
    dry_run = True
    doctor_output = io.StringIO()
    try:
        doctor_rc = doctor_mod.cure_what_ails_you(
            instance=instance,
            dry_run=dry_run,
            mount_table=mtab.new(),
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=process_finder.new(),
            out=ui_mod.PlainOutput(doctor_output),
        )
        out.write(
            b"\neden doctor --dry-run (exit code %d):\n%s\n"
            % (doctor_rc, doctor_output.getvalue().encode())
        )
    except Exception:
        out.write(b"\nUnexpected exception thrown while running eden doctor checks:\n")
        out.write(traceback.format_exc().encode("utf-8") + b"\n")


def print_tail_of_log_file(path: Path, out: IO[bytes]) -> None:
    try:
        out.write(b"\nMost recent Eden logs:\n")
        LOG_AMOUNT = 20 * 1024
        with path.open("rb") as logfile:
            size = logfile.seek(0, io.SEEK_END)
            logfile.seek(max(0, size - LOG_AMOUNT), io.SEEK_SET)
            data = logfile.read()
            out.write(data)
    except Exception as e:
        out.write(b"Error reading the log file: %s\n" % str(e).encode())


def print_running_eden_process(out: IO[bytes]) -> None:
    try:
        out.write(b"\nList of running Eden processes:\n")
        output = subprocess.check_output(
            ["ps", "-eo", "pid,ppid,comm,start_time,etime"]
        )
        output = output.decode()
        lines = output.split("\n")
        format_str = "{:>20} {:>20} {:>10} {:>20} {:>20}\n"
        out.write(
            format_str.format(
                "Pid", "PPid", "Command", "Start Time", "Elapsed Time"
            ).encode()
        )
        for line in lines:
            if "edenfs" in line:
                word = line.split()
                out.write(format_str.format(*word).encode())
    except Exception as e:
        out.write(b"Error getting the eden processes: %s\n" % str(e).encode())


def print_edenfs_process_tree(pid: int, out: IO[bytes]) -> None:
    try:
        out.write(b"\nedenfs process tree:\n")
        output = subprocess.check_output(
            ["ps", "f", "-o", "pid,s,comm,start_time,etime,cputime,drs", "-s", str(pid)]
        )
        out.write(output)
    except Exception as e:
        out.write(b"Error getting edenfs process tree: %s\n" % str(e).encode())
