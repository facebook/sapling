#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import getpass
import io
import socket
import subprocess
from typing import IO

from . import (
    config as config_mod,
    debug as debug_mod,
    doctor as doctor_mod,
    filesystem,
    mtab,
    stats as stats_mod,
)


def print_diagnostic_info(config: config_mod.Config, out: IO[bytes]) -> None:
    out.write(b"User                    : %s\n" % getpass.getuser().encode())
    out.write(b"Hostname                : %s\n" % socket.gethostname().encode())
    print_rpm_version(out)

    health_status = config.check_health()
    if health_status.is_healthy():
        out.write(b"\n")
        debug_mod.do_buildinfo(config, out)
        out.write(b"uptime: ")
        debug_mod.do_uptime(config, out)
        print_eden_doctor_report(config, out)
    else:
        out.write(b"Eden is not running. Some debug info will be omitted.\n")

    print_tail_of_log_file(config.get_log_path(), out)
    print_running_eden_process(out)

    out.write(b"\nList of mount points:\n")
    mountpoint_paths = []
    for key in sorted(config.get_mount_paths()):
        key = key.encode()
        out.write(key)
        mountpoint_paths.append(key)
    for key, val in config.get_all_client_config_info().items():
        out.write(b"\nMount point info for path %s:\n" % key.encode())
        for k, v in val.items():
            out.write("{:>10} : {}\n".format(k, v).encode())
    if health_status.is_healthy():
        with io.StringIO() as stats_stream:
            stats_mod.do_stats_general(config, out=stats_stream)
            out.write(stats_stream.getvalue().encode())


def print_rpm_version(out: IO[bytes]):
    try:
        queryformat = "%{VERSION}"
        output = subprocess.check_output(["rpm", "-q", "--qf", queryformat, "fb-eden"])
        out.write(b"Rpm Version             : %s\n" % output)
    except Exception as e:
        out.write(b"Error getting the Rpm version : %s\n" % str(e).encode())


def print_eden_doctor_report(config, out: IO[bytes]):
    dry_run = True
    doctor_output = io.StringIO()
    doctor_rc = doctor_mod.cure_what_ails_you(
        config,
        dry_run,
        doctor_output,
        mount_table=mtab.LinuxMountTable(),
        fs_util=filesystem.LinuxFsUtil(),
    )
    out.write(
        b"\neden doctor --dry-run (exit code %d):\n%s\n"
        % (doctor_rc, doctor_output.getvalue().encode())
    )


def print_tail_of_log_file(path: str, out: IO[bytes]):
    try:
        out.write(b"\nMost recent Eden logs:\n")
        LOG_AMOUNT = 20 * 1024
        with open(path, "rb") as logfile:
            size = logfile.seek(0, io.SEEK_END)
            logfile.seek(max(0, size - LOG_AMOUNT), io.SEEK_SET)
            data = logfile.read()
            out.write(data)
    except Exception as e:
        out.write(b"Error reading the log file: %s\n" % str(e).encode())


def print_running_eden_process(out: IO[bytes]):
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
