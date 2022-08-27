#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import csv
import getpass
import io
import os
import platform
import re
import shlex
import shutil
import socket
import subprocess
import sys
import traceback
from datetime import datetime, timedelta
from pathlib import Path
from typing import Callable, cast, Dict, Generator, IO, List, Optional, Tuple

from . import (
    debug as debug_mod,
    doctor as doctor_mod,
    redirect as redirect_mod,
    stats as stats_mod,
    top as top_mod,
    ui as ui_mod,
    util as util_mod,
    version as version_mod,
)
from .config import EdenInstance

try:
    from .facebook.rage import find_fb_cdb, setup_fb_env

except ImportError:

    def find_fb_cdb() -> Optional[Path]:
        return None

    def setup_fb_env(env: Dict[str, str]) -> Dict[str, str]:
        return env


try:
    from eden.fs.cli.doctor.facebook.check_vscode_extensions import (
        find_problematic_vscode_extensions,
    )

except ImportError:

    def find_problematic_vscode_extensions() -> None:
        return


try:
    from .facebook.rage import _report_edenfs_bug
except ImportError:

    def _report_edenfs_bug(
        rage_lambda: Callable[[EdenInstance, IO[bytes]], None],
        instance: EdenInstance,
        reporter: str,
    ) -> None:
        print("_report_edenfs_bug() is unimplemented.", file=sys.stderr)
        return None


def section_title(message: str, out: IO[bytes]) -> None:
    out.write(util_mod.underlined(message).encode())


def print_diagnostic_info(
    instance: EdenInstance, out: IO[bytes], dry_run: bool
) -> None:
    section_title("System info:", out)
    header = (
        f"User                    : {getpass.getuser()}\n"
        f"Hostname                : {socket.gethostname()}\n"
        f"Version                 : {version_mod.get_current_version()}\n"
    )
    out.write(header.encode())
    if sys.platform != "win32":
        # We attempt to report the RPM version on Linux as well as Mac, since Mac OS
        # can use RPMs as well.  If the RPM command fails this will just report that
        # and will continue reporting the rest of the rage data.
        print_rpm_version(out)
    print_os_version(out)
    if sys.platform == "darwin":
        cpu = "arm64" if util_mod.is_apple_silicon() else "x86_64"
        out.write(f"Architecture            : {cpu}\n".encode())

    health_status = instance.check_health()
    if health_status.is_healthy():
        section_title("Build info:", out)
        debug_mod.do_buildinfo(instance, out)
        out.write(b"uptime: ")
        instance.do_uptime(pretty=False, out=out)

    # Running eden doctor inside a hanged eden checkout can cause issues.
    # We will disable this until we figure out a work-around.
    # TODO(T113845692)
    # print_eden_doctor_report(instance, out)

    processor = instance.get_config_value("rage.reporter", default="")
    if not dry_run and processor:
        section_title("Verbose EdenFS logs:", out)
        paste_output(
            lambda sink: print_log_file(
                instance.get_log_path(), sink, whole_file=False
            ),
            processor,
            out,
        )
    print_tail_of_log_file(instance.get_log_path(), out)
    print_running_eden_process(out)
    print_crashed_edenfs_logs(processor, out)

    if health_status.is_healthy():
        # assign to variable to make type checker happy :(
        edenfs_instance_pid = health_status.pid
        if edenfs_instance_pid is not None:
            print_edenfs_process_tree(edenfs_instance_pid, out)
            if not dry_run and processor:
                trace_running_edenfs(processor, edenfs_instance_pid, out)

    print_eden_redirections(instance, out)

    section_title("List of mount points:", out)
    mountpoint_paths = []
    for key in sorted(instance.get_mount_paths()):
        out.write(key.encode() + b"\n")
        mountpoint_paths.append(key)
    for checkout_path in mountpoint_paths:
        out.write(b"\nMount point info for path %s:\n" % checkout_path.encode())
        for k, v in instance.get_checkout_info(checkout_path).items():
            out.write("{:>20} : {}\n".format(k, v).encode())
    if health_status.is_healthy():
        # TODO(zeyi): enable this when memory usage collecting is implemented on Windows
        with io.StringIO() as stats_stream:
            stats_mod.do_stats_general(
                instance,
                stats_mod.StatsGeneralOptions(out=stats_stream),
            )
            out.write(stats_stream.getvalue().encode())

    print_counters(instance, "EdenFS", top_mod.COUNTER_REGEX, out)

    if sys.platform == "win32":
        print_counters(instance, "Prjfs", r"prjfs\..*", out)

    print_eden_config(instance, out)

    print_prefetch_profiles_list(instance, out)

    print_third_party_vscode_extensions(out)

    print_env_variables(out)

    print_system_mount_table(out)


def report_edenfs_bug(instance: EdenInstance, reporter: str) -> None:
    rage_lambda: Callable[
        [EdenInstance, IO[bytes]], None
    ] = lambda inst, sink: print_diagnostic_info(inst, sink, False)
    _report_edenfs_bug(rage_lambda, instance, reporter)


def print_rpm_version(out: IO[bytes]) -> None:
    try:
        rpm_version = version_mod.get_installed_eden_rpm_version()
        out.write(f"RPM Version             : {rpm_version}\n".encode())
    except Exception as e:
        out.write(f"Error getting the RPM version : {e}\n".encode())


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
    elif sys.platform == "win32":
        import winreg

        with winreg.OpenKey(
            winreg.HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"
        ) as k:
            build = winreg.QueryValueEx(k, "CurrentBuild")
        version = f"Windows {build[0]}"

    if not version:
        version = platform.system() + " " + platform.version()

    out.write(f"OS Version              : {version}\n".encode("utf-8"))


def print_eden_doctor_report(instance: EdenInstance, out: IO[bytes]) -> None:
    doctor_output = io.StringIO()
    try:
        doctor_rc = doctor_mod.cure_what_ails_you(
            instance, dry_run=True, out=ui_mod.PlainOutput(doctor_output)
        )
        doctor_report_title = f"eden doctor --dry-run (exit code {doctor_rc}):"
        section_title(doctor_report_title, out)
        out.write(doctor_output.getvalue().encode())
    except Exception:
        out.write(b"\nUnexpected exception thrown while running eden doctor checks:\n")
        out.write(traceback.format_exc().encode("utf-8") + b"\n")


def read_chunk(logfile: IO[bytes]) -> Generator[bytes, None, None]:
    CHUNK_SIZE = 20 * 1024
    while True:
        data = logfile.read(CHUNK_SIZE)
        if not data:
            break
        yield data


def print_log_file(
    path: Path, out: IO[bytes], whole_file: bool, size: int = 1000000
) -> None:
    try:
        with path.open("rb") as logfile:
            if not whole_file:
                LOG_AMOUNT = size
                size = logfile.seek(0, io.SEEK_END)
                logfile.seek(max(0, size - LOG_AMOUNT), io.SEEK_SET)
            for data in read_chunk(logfile):
                out.write(data)
    except Exception as e:
        out.write(b"Error reading the log file: %s\n" % str(e).encode())


def paste_output(
    output_generator: Callable[[IO[bytes]], None], processor: str, out: IO[bytes]
) -> None:
    try:
        proc = subprocess.Popen(
            shlex.split(processor), stdin=subprocess.PIPE, stdout=subprocess.PIPE
        )
        sink = cast(IO[bytes], proc.stdin)
        output = cast(IO[bytes], proc.stdout)

        try:
            output_generator(sink)
        finally:
            sink.close()

            stdout = output.read().decode("utf-8")

            output.close()
            proc.wait()

        # Expected output to be in form "<str0>\n<str1>: <str2>\n"
        # and we want str1
        pattern = re.compile("^.*\\n[a-zA-Z0-9_.-]*: .*\\n$")
        match = pattern.match(stdout)

        if not match:
            out.write(stdout.encode())
        else:
            paste, _ = stdout.split("\n")[1].split(": ")
            out.write(paste.encode())
    except Exception as e:
        out.write(b"Error generating paste: %s\n" % str(e).encode())


def print_tail_of_log_file(path: Path, out: IO[bytes]) -> None:
    try:
        section_title("Most recent EdenFS logs:", out)
        LOG_AMOUNT = 20 * 1024
        with path.open("rb") as logfile:
            size = logfile.seek(0, io.SEEK_END)
            logfile.seek(max(0, size - LOG_AMOUNT), io.SEEK_SET)
            data = logfile.read()
            out.write(data)
    except Exception as e:
        out.write(b"Error reading the log file: %s\n" % str(e).encode())


def _get_running_eden_process_windows() -> List[Tuple[str, str, str, str, str, str]]:
    output = subprocess.check_output(
        [
            "wmic",
            "process",
            "where",
            "name like '%eden%'",
            "get",
            "processid,parentprocessid,creationdate,commandline",
            "/format:csv",
        ]
    )
    reader = csv.reader(io.StringIO(output.decode().strip()))
    next(reader)  # skip column header
    lines = []
    for line in reader:
        start_time: datetime = datetime.strptime(line[2][:-4], "%Y%m%d%H%M%S.%f")
        elapsed = str(datetime.now() - start_time)
        # (pid, ppid, start_time, etime, comm)
        lines.append(
            (line[4], line[3], start_time.strftime("%b %d %H:%M"), elapsed, line[1])
        )
    return lines


def print_running_eden_process(out: IO[bytes]) -> None:
    try:
        section_title("List of running EdenFS processes:", out)
        if sys.platform == "win32":
            lines = _get_running_eden_process_windows()
        else:
            # Note well: `comm` must be the last column otherwise it will be
            # truncated to ~12 characters wide on darwin, which is useless
            # because almost everything is started via an absolute path
            output = subprocess.check_output(
                ["ps", "-eo", "pid,ppid,start_time,etime,comm"]
                if sys.platform == "linux"
                else ["ps", "-Awwx", "-eo", "pid,ppid,start,etime,comm"]
            )
            output = output.decode()
            lines = [line.split() for line in output.split("\n") if "eden" in line]

        format_str = "{:>20} {:>20} {:>20} {:>20} {}\n"
        out.write(
            format_str.format(
                "Pid", "PPid", "Start Time", "Elapsed Time", "Command"
            ).encode()
        )
        for line in lines:
            out.write(format_str.format(*line).encode())
    except Exception as e:
        out.write(b"Error getting the EdenFS processes: %s\n" % str(e).encode())
        out.write(traceback.format_exc().encode() + b"\n")


def print_edenfs_process_tree(pid: int, out: IO[bytes]) -> None:
    if sys.platform != "linux":
        return
    try:
        section_title("EdenFS process tree:", out)
        output = subprocess.check_output(["ps", "-o", "sid=", "-p", str(pid)])
        sid = output.decode("utf-8").strip()
        output = subprocess.check_output(
            ["ps", "f", "-o", "pid,s,comm,start_time,etime,cputime,drs", "-s", sid]
        )
        out.write(output)
    except Exception as e:
        out.write(b"Error getting edenfs process tree: %s\n" % str(e).encode())


def print_eden_redirections(instance: EdenInstance, out: IO[bytes]) -> None:
    try:
        section_title("EdenFS redirections:", out)
        checkouts = instance.get_checkouts()
        for checkout in checkouts:
            out.write(bytes(checkout.path) + b"\n")
            output = redirect_mod.prepare_redirection_list(checkout, instance)
            # append a tab at the beginning of every new line to indent
            output = output.replace("\n", "\n\t")
            out.write(b"\t" + output.encode() + b"\n")
    except Exception as e:
        out.write(b"Error getting EdenFS redirections %s\n" % str(e).encode())
        out.write(traceback.format_exc().encode() + b"\n")


def print_counters(
    instance: EdenInstance, type: str, regex: str, out: IO[bytes]
) -> None:
    try:
        section_title(f"{type} counters:", out)
        with instance.get_thrift_client_legacy(timeout=3) as client:
            counters = client.getRegexCounters(regex)
            for key, value in counters.items():
                out.write(f"{key}: {value}\n".encode())
    except Exception as e:
        out.write(f"Error getting {type} Thrift counters: {str(e)}\n".encode())


def print_env_variables(out: IO[bytes]) -> None:
    try:
        section_title("Environment variables:", out)
        for k, v in os.environ.items():
            out.write(f"{k}={v}\n".encode())
    except Exception as e:
        out.write(f"Error getting environment variables: {e}\n".encode())


def print_system_mount_table(out: IO[bytes]) -> None:
    if sys.platform == "win32":
        return
    try:
        section_title("Mount table:", out)
        output = subprocess.check_output(["mount"])
        out.write(output)
    except Exception as e:
        out.write(f"Error printing system mount table: {e}\n".encode())


def print_eden_config(instance: EdenInstance, out: IO[bytes]) -> None:
    try:
        section_title("EdenFS config:", out)
        instance.print_full_config(out)
    except Exception as e:
        out.write(f"Error printing EdenFS config: {e}\n".encode())


def print_prefetch_profiles_list(instance: EdenInstance, out: IO[bytes]) -> None:
    try:
        section_title("Prefetch Profiles list:", out)
        checkouts = instance.get_checkouts()
        for checkout in checkouts:
            profiles = sorted(checkout.get_config().active_prefetch_profiles)
            if profiles:
                out.write(f"{checkout.path}:\n".encode())
                for name in profiles:
                    out.write(f"  - {name}\n".encode())
            else:
                out.write(f"{checkout.path}: []\n".encode())
    except Exception as e:
        out.write(f"Error printing Prefetch Profiles list: {e}\n".encode())


def print_crashed_edenfs_logs(processor: str, out: IO[bytes]) -> None:
    if sys.platform == "darwin":
        crashes_paths = [
            Path("/Library/Logs/DiagnosticReports"),
            Path.home() / Path("Library/Logs/DiagnosticReports"),
        ]
    elif sys.platform == "win32":
        import winreg

        key = winreg.OpenKey(
            winreg.HKEY_LOCAL_MACHINE,
            "SOFTWARE\\Microsoft\\Windows\\Windows Error Reporting\\LocalDumps",
        )
        crashes_paths = [Path(winreg.QueryValueEx(key, "DumpFolder")[0])]
    else:
        return

    section_title("EdenFS crashes:", out)
    num_uploads = 0
    for crashes_path in crashes_paths:
        if not crashes_path.exists():
            continue

        # Only upload crashes from the past week.
        date_threshold = datetime.now() - timedelta(weeks=1)
        for crash in crashes_path.iterdir():
            if crash.name.startswith("edenfs"):
                crash_time = datetime.fromtimestamp(crash.stat().st_mtime)
                human_crash_time = crash_time.strftime("%b %d %H:%M:%S")
                out.write(f"{str(crash.name)} from {human_crash_time}: ".encode())
                if crash_time > date_threshold and num_uploads <= 2:
                    num_uploads += 1
                    paste_output(
                        lambda sink: print_log_file(crash, sink, whole_file=True),
                        processor,
                        out,
                    )
                else:
                    out.write(" not uploaded due to age or max num dumps\n".encode())

    out.write("\n".encode())


def trace_running_edenfs(processor: str, pid: int, out: IO[bytes]) -> None:
    if sys.platform == "darwin":
        trace_fn = print_sample_trace
    elif sys.platform == "win32":
        trace_fn = print_cdb_backtrace
    else:
        return

    section_title("EdenFS process trace", out)
    try:
        paste_output(
            lambda sink: trace_fn(pid, sink),
            processor,
            out,
        )
    except Exception as e:
        out.write(b"Error getting EdenFS trace: %s.\n" % str(e).encode())


def find_cdb() -> Optional[Path]:
    wdk_path = Path("C:/Program Files (x86)/Windows Kits/10/Debuggers/x64/cdb.exe")
    if wdk_path.exists():
        return wdk_path
    else:
        return find_fb_cdb()


def print_cdb_backtrace(pid: int, sink: IO[bytes]) -> None:
    cdb_path = find_cdb()
    if cdb_path is None:
        raise Exception("No cdb.exe found.")

    cdb_cmd = [cdb_path.as_posix()]

    cdb_cmd += [
        "-p",
        str(pid),
        "-pvr",  # Do not add a breakpoint,
        "-y",  # Add the following to the symbol path
        "C:/tools/eden/libexec/",
        "-lines",  # Print lines if possible
        "-c",  # Execute the following command
    ]

    debugger_command = [
        "~*k",  # print backtraces of all threads
        "qd",  # Detach and quit
    ]
    cdb_cmd += [";".join(debugger_command)]

    env = os.environ.copy()
    env = setup_fb_env(env)

    subprocess.run(cdb_cmd, check=True, stderr=subprocess.STDOUT, stdout=sink, env=env)


def print_sample_trace(pid: int, sink: IO[bytes]) -> None:
    # "sample" is specific to MacOS. Check if it exists before running.
    stack_trace_cmd = []

    sample_full_path = shutil.which("sample")
    if sample_full_path is None:
        return

    if util_mod.is_apple_silicon():
        stack_trace_cmd += ["arch", "-arm64"]

    stack_trace_cmd += [sample_full_path, str(pid), "1", "100"]

    subprocess.run(
        stack_trace_cmd,
        check=True,
        stderr=subprocess.STDOUT,
        stdout=sink,
    )


def print_third_party_vscode_extensions(out: IO[bytes]) -> None:
    problematic_extensions = find_problematic_vscode_extensions()

    if problematic_extensions is None:
        return

    section_title("Visual Studio Code Extensions:", out)

    out.write(b"Blocked extensions installed:\n")
    for extension in problematic_extensions.blocked:
        out.write(f"{extension}\n".encode())
    if len(problematic_extensions.blocked) == 0:
        out.write(b"None\n")

    out.write(b"\nUnsupported extensions installed:\n")
    for extension in problematic_extensions.unsupported:
        out.write(f"{extension}\n".encode())
    if len(problematic_extensions.unsupported) == 0:
        out.write(b"None\n")
