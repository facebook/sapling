#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import getpass
import io
import os.path
import platform
import socket
import subprocess
import sys
import traceback
from pathlib import Path
from typing import IO

from . import (
    debug as debug_mod,
    doctor as doctor_mod,
    filesystem,
    mtab,
    proc_utils,
    redirect as redirect_mod,
    stats as stats_mod,
    ui as ui_mod,
    version as version_mod,
)
from .config import EdenInstance


def print_diagnostic_info(instance: EdenInstance, out: IO[bytes]) -> None:
    header = (
        f"User                    : {getpass.getuser()}\n"
        f"Hostname                : {socket.gethostname()}\n"
        f"Version                 : {version_mod.get_current_version()}\n"
    )
    out.write(header.encode("utf-8"))
    if sys.platform != "win32":
        # We attempt to report the RPM version on Linux as well as Mac, since Mac OS
        # can use RPMs as well.  If the RPM command fails this will just report that
        # and will continue reporting the rest of the rage data.
        print_rpm_version(out)
    print_os_version(out)

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

    print_eden_redirections(instance, out)

    out.write(b"\nList of mount points:\n")
    mountpoint_paths = []
    for key in sorted(instance.get_mount_paths()):
        out.write(key.encode() + b"\n")
        mountpoint_paths.append(key)
    for checkout_path in mountpoint_paths:
        out.write(b"\nMount point info for path %s:\n" % checkout_path.encode())
        for k, v in instance.get_checkout_info(checkout_path).items():
            out.write("{:>10} : {}\n".format(k, v).encode())
    if health_status.is_healthy():
        with io.StringIO() as stats_stream:
            stats_mod.do_stats_general(instance, out=stats_stream)
            out.write(stats_stream.getvalue().encode())


def print_rpm_version(out: IO[bytes]) -> None:
    try:
        rpm_version = version_mod.get_installed_eden_rpm_version()
        out.write(f"RPM Version             : {rpm_version}\n".encode("utf-8"))
    except Exception as e:
        out.write(f"Error getting the RPM version : {e}\n".encode("utf-8"))


def print_os_version(out: IO[bytes]) -> None:
    version = None
    if sys.platform == "linux":
        release_file_name = "/etc/os-release"
        if os.path.isfile(release_file_name):
            with open(release_file_name) as release_info_file:
                release_info = {}
                for line in release_info_file:
                    parsed_line = line.rstrip().split("=")
                    if len(parsed_line) == 2:
                        release_info_piece, value = parsed_line
                        release_info[release_info_piece] = value.strip('"')
                if "PRETTY_NAME" in release_info:
                    version = release_info["PRETTY_NAME"]
    elif sys.platform == "darwin":
        version = "MacOS " + platform.mac_ver()[0]

    if not version:
        version = platform.system() + " " + platform.version()

    out.write(f"OS Version              : {version}\n".encode("utf-8"))


def print_eden_doctor_report(instance: EdenInstance, out: IO[bytes]) -> None:
    doctor_output = io.StringIO()
    try:
        doctor_rc = doctor_mod.cure_what_ails_you(
            instance, dry_run=True, out=ui_mod.PlainOutput(doctor_output)
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
        # Note well: `comm` must be the last column otherwise it will be
        # truncated to ~12 characters wide on darwin, which is useless
        # because almost everything is started via an absolute path
        output = subprocess.check_output(
            ["ps", "-eo", "pid,ppid,start_time,etime,comm"]
            if sys.platform == "linux"
            else ["ps", "-Awwx", "-eo", "pid,ppid,start,etime,comm"]
        )
        output = output.decode()
        lines = output.split("\n")
        format_str = "{:>20} {:>20} {:>10} {:>20} {}\n"
        out.write(
            format_str.format(
                "Pid", "PPid", "Start Time", "Elapsed Time", "Command"
            ).encode()
        )
        for line in lines:
            if "edenfs" in line:
                word = line.split()
                out.write(format_str.format(*word).encode())
    except Exception as e:
        out.write(b"Error getting the eden processes: %s\n" % str(e).encode())


def print_edenfs_process_tree(pid: int, out: IO[bytes]) -> None:
    if sys.platform != "linux":
        return
    try:
        out.write(b"\nedenfs process tree:\n")
        output = subprocess.check_output(
            ["ps", "f", "-o", "pid,s,comm,start_time,etime,cputime,drs", "-s", str(pid)]
        )
        out.write(output)
    except Exception as e:
        out.write(b"Error getting edenfs process tree: %s\n" % str(e).encode())


def print_eden_redirections(instance: EdenInstance, out: IO[bytes]) -> None:
    try:
        out.write(b"\nedenfs redirections:\n")
        checkouts = instance.get_checkouts()
        for checkout in checkouts:
            out.write(bytes(checkout.path) + b"\n")
            output = redirect_mod.prepare_redirection_list(checkout)
            # append a tab at the beginning of every new line to indent
            output = output.replace("\n", "\n\t")
            out.write(b"\t" + output.encode() + b"\n")
    except Exception as e:
        out.write(b"Error getting edenfs redirections %s\n" % str(e).encode())
