#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import csv
import getpass
import gzip
import io
import os
import platform
import re
import shlex
import shutil
import subprocess
import sys
import time
import traceback
from datetime import datetime, timedelta
from functools import wraps
from pathlib import Path
from typing import (
    Any,
    BinaryIO,
    Callable,
    cast,
    Dict,
    Generator,
    IO,
    Iterable,
    List,
    Optional,
    Tuple,
    TypeVar,
)

T = TypeVar("T")

from eden.scm.sapling import redact

from . import (
    debug as debug_mod,
    doctor as doctor_mod,
    hostname as hostname_mod,
    redirect as redirect_mod,
    stats as stats_mod,
    top as top_mod,
    ui as ui_mod,
    util as util_mod,
    version as version_mod,
)
from .config import (
    CheckoutPathProblemType,
    detect_checkout_path_problem,
    EdenInstance,
    MOUNT_CONFIG,
)

try:
    from .facebook.rage import (
        find_fb_cdb,
        get_host_dashboard_url,
        get_networking_environment,
        get_quickstack_cmd,
        setup_fb_env,
    )

except ImportError:

    def find_fb_cdb() -> Optional[Path]:
        return None

    def setup_fb_env(env: Dict[str, str]) -> Dict[str, str]:
        return env

    def get_host_dashboard_url(
        normalized_hostname: str, period_end: datetime
    ) -> Optional[str]:
        return None

    def get_quickstack_cmd(
        instance: EdenInstance,
    ) -> Optional[List[str]]:
        return None

    def get_networking_environment() -> Optional[str]:
        return None


try:
    from eden.fs.cli.doctor.facebook.check_vscode_extensions import (
        VSCodeExtensionsChecker,
    )

except ImportError:

    class VSCodeExtensionsChecker:
        def find_problematic_vscode_extensions(self, instance: EdenInstance) -> None:
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


class IOWithRedaction:
    def __init__(self, wrapped: IO[bytes]) -> None:
        self.wrapped = wrapped

    def write(self, s: str) -> int:
        redacted = redact.redactsensitiveinfo(s)
        return self.wrapped.write(redacted.encode())

    def writelines(self, lines: Iterable[str]) -> None:
        for line in lines:
            self.write(line)

    def flush(self) -> None:
        self.wrapped.flush()


THRIFT_COUNTER_REGEX = (
    r"thrift\.(EdenService|BaseService)\..*(time|num_samples|num_calls).*"
)

time_list: Dict[str, str] = {}  # Mapping of function names to duration strings


def section_title(message: str, out: IOWithRedaction) -> None:
    out.write(util_mod.underlined(message))


def print_top_5_items(input_dict: Dict[str, str], out: IOWithRedaction) -> None:
    section_title("Top 5 time consumption", out)
    # Sort the dictionary items by their numeric values (converted to float)
    sorted_items = sorted(input_dict.items(), key=lambda x: float(x[1]), reverse=True)

    top_5_dict = {k: f"{v}s" for k, v in sorted_items[:5]}

    for key, value in top_5_dict.items():
        out.write(f"{key}: {value}\n")


def format_stat_value(stat: os.stat_result) -> Dict[str, str]:
    def format_timestamp(value: float) -> str:
        try:
            return datetime.fromtimestamp(value).strftime("%Y-%m-%d %H:%M:%S")
        except (ValueError, OSError, OverflowError):
            # Just return original value if timestamp conversion fails
            return str(value)

    return {
        "Accessed": format_timestamp(stat.st_atime),
        "Modified": format_timestamp(stat.st_mtime),
    }


T = TypeVar("T")


def timer_decorator(func: Callable[..., T]) -> Callable[..., T]:
    @wraps(func)
    def wrapper(*args: Any, **kwargs: Any) -> T:
        start_time = time.time()
        result = func(*args, **kwargs)
        end_time = time.time()
        duration = end_time - start_time
        if args[0]:
            args[0].write(f"\n{func.__name__} took {duration:.2f} seconds\n")
        time_list[func.__name__] = f"{duration:.2f}"
        return result

    return wrapper


def get_watchman_log_path() -> Optional[Path]:
    watchman_log = ""
    for root in [
        "/var/facebook/watchman",
        "/opt/facebook/var/run/watchman",
        "/opt/facebook/watchman/var/run/watchman",
        os.environ.get("TEMP"),
        os.environ.get("TMP"),
    ]:
        if root is None or root == "":
            continue

        watchman_log = os.path.join(
            "%s/%s-state" % (root, os.environ.get("USER")), "log"
        )
        if os.path.isfile(watchman_log):
            break

    if sys.platform == "win32":
        appdata = os.environ.get("LOCALAPPDATA")
        if appdata:
            watchman_appdata = os.path.join(appdata, "watchman")
            if os.path.exists(watchman_appdata):
                watchman_log = os.path.join(watchman_appdata, "log")

    if os.path.isfile(watchman_log):
        return Path(watchman_log)
    return None


def get_upgrade_log_path() -> Optional[Path]:
    if sys.platform == "win32":
        return None

    for upgrade_log in [
        "/var/facebook/logs/edenfs_upgrade.log",
        "/Users/Shared/edenfs_upgrade.log",
    ]:
        if os.path.isfile(upgrade_log):
            return Path(upgrade_log)
    return None


def get_config_log_path() -> Optional[Path]:
    if sys.platform == "win32":
        return None

    for config_log in [
        "/var/facebook/logs/edenfs_config.log",
        "/Users/Shared/edenfs_config.log",
    ]:
        if os.path.isfile(config_log):
            return Path(config_log)
    return None


@timer_decorator
def print_host_dashboard(out: IOWithRedaction, host: str) -> None:
    host_dashboard_url = get_host_dashboard_url(host, datetime.now())
    if host_dashboard_url:
        section_title("Host dashboard:", out)
        out.write(f"{host_dashboard_url}\n")


def get_rotated_edenfs_log_path(instance: EdenInstance) -> Optional[Path]:
    all_rotated_logs = [
        (f)
        for f in instance.get_log_dir().iterdir()
        if f.name.endswith(".gz") and f.name.startswith("edenfs.log")
    ]
    all_rotated_logs.sort(key=lambda x: x.stat().st_ctime, reverse=True)
    if len(all_rotated_logs) == 0:
        return None
    else:
        return all_rotated_logs[0]


@timer_decorator
def get_eden_logs(
    out: IOWithRedaction, processor: str, instance: EdenInstance, dry_run: bool
) -> None:
    if not dry_run and processor:
        section_title("Verbose EdenFS logs:", out)
        paste_output(
            lambda sink: print_log_file(instance.get_log_path(), sink),
            processor,
            out,
            dry_run,
        )

        section_title("Rotated EdenFS log:", out)
        rotated_log_file = get_rotated_edenfs_log_path(instance)
        if rotated_log_file is not None:
            out.write(f"Snippet of rotated log file {str(rotated_log_file)}: ")
            paste_output(
                lambda sink, log=rotated_log_file: print_log_file(
                    log, sink, open_fn=gzip.open
                ),
                processor,
                out,
                dry_run,
            )
        else:
            out.write("No rotated logs found")


@timer_decorator
def print_watchman_log(out: IOWithRedaction, processor: str, dry_run: bool) -> None:
    watchman_log_path = get_watchman_log_path()

    if watchman_log_path:
        section_title("Watchman logs:", out)
        out.write(f"Logs from: {watchman_log_path}\n")
        paste_output(
            lambda sink: print_log_file(watchman_log_path, sink),
            processor,
            out,
            dry_run,
        )


@timer_decorator
def print_upgrade_path(out: IOWithRedaction, processor: str, dry_run: bool) -> None:
    upgrade_log_path = get_upgrade_log_path()
    if upgrade_log_path:
        section_title("EdenFS Upgrade logs:", out)
        out.write(f"Logs from: {upgrade_log_path}\n")
        paste_output(
            lambda sink: print_log_file(upgrade_log_path, sink),
            processor,
            out,
            dry_run,
        )
    elif sys.platform != "win32":
        section_title("EdenFS Upgrade logs:", out)
        out.write("Log file does not exist\n")


@timer_decorator
def print_config_path(out: IOWithRedaction, processor: str, dry_run: bool) -> None:
    config_log_path = get_config_log_path()
    if config_log_path:
        section_title("EdenFS Config logs:", out)
        out.write(f"Logs from: {config_log_path}\n")
        paste_output(
            lambda sink: print_log_file(config_log_path, sink),
            processor,
            out,
            dry_run,
        )
    elif sys.platform != "win32":
        section_title("EdenFS Config logs:", out)
        out.write("Log file does not exist\n")


def print_diagnostic_info(
    instance: EdenInstance, unsafe_out: IO[bytes], dry_run: bool
) -> None:
    # Wrap output stream with redaction logic so that we don't print secrets
    # (such as auth tokens) to the output buffer.
    out = IOWithRedaction(unsafe_out)
    host = hostname_mod.get_normalized_hostname()
    health_status = instance.check_health()
    edenfs_instance_pid = None
    if health_status.is_healthy():
        # assign to variable to make type checker happy :(
        edenfs_instance_pid = health_status.pid

    print_system_info(out, host)

    # Running eden doctor inside a hanged eden checkout can cause issues.
    # We will disable this until we figure out a work-around.
    # TODO(T113845692)
    # print_eden_doctor_report(instance, out)

    print_build_info(out, host, instance)
    print_host_dashboard(out, host)

    processor = instance.get_config_value("rage.reporter", default="")
    get_eden_logs(out, processor, instance, dry_run)

    print_watchman_log(out, processor, dry_run)
    print_upgrade_path(out, processor, dry_run)
    print_config_path(out, processor, dry_run)

    print_tail_of_log_file(out, instance.get_log_path())
    print_running_eden_process(out)
    print_crashed_edenfs_logs(out, processor, dry_run)

    edenfs_instance_pid = None
    if health_status.is_healthy():
        # assign to variable to make type checker happy :(
        edenfs_instance_pid = health_status.pid
        if health_status.is_healthy():
            if edenfs_instance_pid is not None:
                print_edenfs_process_tree(out, edenfs_instance_pid)
                if not dry_run and processor:
                    trace_running_edenfs(out, processor, edenfs_instance_pid, dry_run)

    print_eden_redirections(out, instance)

    print_eden_mounts(out, instance)
    if health_status.is_healthy():
        print_memory_usage(out, instance)
        # TODO(zeyi): enable this when memory usage collecting is implemented on Windows

    print_counters(out, instance, "EdenFS", top_mod.COUNTER_REGEX)
    print_counters(
        out,
        instance,
        "Thrift",
        THRIFT_COUNTER_REGEX,
    )

    if health_status.is_healthy() and not dry_run and processor:
        print_recent_events(out, processor, dry_run)

    if sys.platform == "win32":
        print_counters(out, instance, "Prjfs", r"prjfs\..*")

    print_eden_config(out, instance, processor, dry_run)

    print_prefetch_profiles_list(out, instance)

    print_third_party_vscode_extensions(out, instance)

    print_system_mount_table(out)
    print_env_variables(out, edenfs_instance_pid)

    get_disk_space_usauge(out, processor, dry_run)

    print_eden_doctor(out, processor, dry_run)

    print_system_load(out)

    get_quickstack(out, instance, processor, dry_run)

    print_ulimits(out)

    print_top_5_items(time_list, out)


@timer_decorator
def print_memory_usage(out: IOWithRedaction, instance: EdenInstance) -> None:
    # TODO(zeyi): enable this when memory usage collecting is implemented on Windows
    with io.StringIO() as stats_stream:
        stats_mod.do_stats_general(
            instance,
            # pyre-fixme[6]: For 1st argument expected `TextIOWrapper[Any]` but
            #  got `StringIO`.
            stats_mod.StatsGeneralOptions(out=stats_stream),
        )
        out.write(stats_stream.getvalue())


@timer_decorator
def print_eden_mounts(out: IOWithRedaction, instance: EdenInstance) -> None:
    section_title("List of mount points:", out)
    mountpoint_paths = []
    for key in sorted(instance.get_mount_paths()):
        out.write(f"{key}\n")
        mountpoint_paths.append(key)
    mounts = instance.get_mounts()
    mounts_data = {
        mount.path.as_posix(): mount.to_json_dict() for mount in mounts.values()
    }

    for checkout_path in mountpoint_paths:
        try:
            nested_checkout, __ = detect_checkout_path_problem(checkout_path, instance)
        except Exception:
            nested_checkout = None
        out.write(f"\nMount point info for path {checkout_path}:\n")
        checkout_data = instance.get_checkout_info(checkout_path)
        mount_data = mounts_data.get(checkout_path, {})
        # "data_dir" in mount_data and "state_dir" in checkout_data are duplicates
        if "data_dir" in mount_data:
            mount_data.pop("data_dir")

        if nested_checkout == CheckoutPathProblemType.NESTED_CHECKOUT:
            mount_data["nested_checkout"] = True
        else:
            mount_data["nested_checkout"] = False
        checkout_data.update(mount_data)
        for k, v in checkout_data.items():
            out.write("{:>20} : {}\n".format(k, v))

        # We've been getting reports of malformed config files, so let's print
        # the stat information to aid debugging. When that bug is fixed, we can
        # remove this code if it's not useful.
        out.write(f"\nMount config file stats for path {checkout_path}:\n")

        state_dir = checkout_data.get("state_dir")
        if not isinstance(state_dir, str) or not os.path.isdir(state_dir):
            out.write(f"Unable to find state directory for {checkout_path}\n")
            return

        mount_config_path = Path(state_dir) / MOUNT_CONFIG
        if mount_config_path.exists():
            try:
                file_stat = mount_config_path.stat()
                for k, v in format_stat_value(file_stat).items():
                    out.write("{:>20} : {}\n".format(k, v))
            except Exception as e:
                out.write(
                    f"Error getting mount config file stats for {mount_config_path}: {e}\n"
                )
        else:
            out.write(f"Mount config file does not exist at {mount_config_path}\n")


@timer_decorator
def print_build_info(out: IOWithRedaction, host: str, instance: EdenInstance) -> None:
    health_status = instance.check_health()
    if health_status.is_healthy():
        section_title("Build info:", out)
        debug_mod.do_buildinfo(instance, out.wrapped)
        out.write("uptime: ")
        instance.do_uptime(pretty=False, out=out.wrapped)


@timer_decorator
def print_system_info(out: IOWithRedaction, host: str) -> None:
    section_title("System info:", out)
    user = getpass.getuser()
    host = hostname_mod.get_normalized_hostname()
    net_env = get_networking_environment()
    header = (
        f"User                    : {user}\n"
        f"Hostname                : {host}\n"
        f"Version                 : {version_mod.get_current_version()}\n"
    )

    if net_env:
        header += f"Network                 : {net_env}\n"

    out.write(header)
    if sys.platform != "win32":
        # We attempt to report the RPM version on Linux as well as Mac, since Mac OS
        # can use RPMs as well.  If the RPM command fails this will just report that
        # and will continue reporting the rest of the rage data.
        print_rpm_version(out)
    print_os_version(out)
    if sys.platform == "darwin":
        cpu = "arm64" if util_mod.is_apple_silicon() else "x86_64"
        out.write(f"Architecture            : {cpu}\n")


@timer_decorator
def get_disk_space_usauge(out: IOWithRedaction, processor: str, dry_run: bool) -> None:
    section_title("Disk Space Usage:", out)
    paste_output(
        lambda sink: print_disk_space_usage(sink),
        processor,
        out,
        dry_run,
    )


@timer_decorator
def get_quickstack(
    out: IOWithRedaction, instance: EdenInstance, processor: str, dry_run: bool
) -> None:
    quickstack_cmd = get_quickstack_cmd(instance)
    if quickstack_cmd:
        section_title("Quickstack:", out)
        paste_output(
            lambda sink: run_cmd(quickstack_cmd, sink, out),
            processor,
            out,
            dry_run,
        )


def report_edenfs_bug(instance: EdenInstance, reporter: str) -> None:
    rage_lambda: Callable[[EdenInstance, IO[bytes]], None] = (
        lambda inst, sink: print_diagnostic_info(inst, sink, False)
    )
    _report_edenfs_bug(rage_lambda, instance, reporter)


def print_rpm_version(out: IOWithRedaction) -> None:
    try:
        rpm_version = version_mod.get_installed_eden_rpm_version()
        out.write(f"RPM Version             : {rpm_version}\n")
    except Exception as e:
        out.write(f"Error getting the RPM version : {e}\n")


def print_os_version(out: IOWithRedaction) -> None:
    version = None
    if sys.platform == "linux":
        release_file_name = "/etc/os-release"
        if os.path.isfile(release_file_name):
            with open(
                release_file_name, errors="backslashreplace"
            ) as release_info_file:
                release_info = {}
                for line in release_info_file:
                    parsed_line = line.rstrip().split("=")
                    if len(parsed_line) == 2:
                        release_info_piece, value = parsed_line
                        release_info[release_info_piece] = value.strip('"')
                if "PRETTY_NAME" in release_info:
                    version = release_info["PRETTY_NAME"]
    elif sys.platform == "darwin":
        # While upstream Python correctly returns the macOS version number from
        # platform.mac_ver(), the version we're currently using incorrectly
        # returns '10.16' on macOS Ventura.  So let's get the OS version from a
        # system helper instead.
        try:
            sw_vers = subprocess.check_output(["/usr/bin/sw_vers", "-productVersion"])
            version = (
                "MacOS " + sw_vers.decode("utf-8", errors="backslashreplace").rstrip()
            )
        except Exception:
            version = (
                "MacOS " + platform.mac_ver()[0] + " (platform.mac_ver() fallback)"
            )
    elif sys.platform == "win32":
        import winreg

        with winreg.OpenKey(
            winreg.HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"
        ) as k:
            build = winreg.QueryValueEx(k, "CurrentBuild")
        version = f"Windows {build[0]}"

    if not version:
        version = platform.system() + " " + platform.version()

    out.write(f"OS Version              : {version}\n")


@timer_decorator
def print_eden_doctor_report(out: IOWithRedaction, instance: EdenInstance) -> None:
    doctor_output = io.StringIO()
    try:
        doctor_rc = doctor_mod.cure_what_ails_you(
            instance, dry_run=True, wait=True, out=ui_mod.PlainOutput(doctor_output)
        )
        doctor_report_title = f"eden doctor --dry-run (exit code {doctor_rc}):"
        section_title(doctor_report_title, out)
        out.write(doctor_output.getvalue())
    except Exception:
        out.write("\nUnexpected exception thrown while running eden doctor checks:\n")
        out.write(f"{traceback.format_exc()}\n")


def read_chunk(
    logfile: BinaryIO,
) -> Generator[bytes, None, None]:
    CHUNK_SIZE = 20 * 1024
    while True:
        data = logfile.read(CHUNK_SIZE)
        if not data:
            break
        yield data


def print_log_file(
    path: Path,
    out: IOWithRedaction,
    open_fn: Callable[
        [Path, str],
        BinaryIO,
    ] = open,
    tail_limit: Optional[int] = 1000000,
) -> None:
    try:
        with open_fn(path, "rb") as logfile:
            if tail_limit is not None:
                LOG_AMOUNT = tail_limit
                size = logfile.seek(0, io.SEEK_END)
                logfile.seek(max(0, size - LOG_AMOUNT), io.SEEK_SET)
            for data in read_chunk(logfile):
                out.write(data.decode("utf-8", errors="backslashreplace"))
    except Exception as e:
        out.write(f"Error reading the log file: {e}\n")


def paste_output(
    output_generator: Callable[[IOWithRedaction], None],
    processor: str,
    out: IOWithRedaction,
    dry_run: bool,
) -> int:
    if dry_run:
        out.write(
            "Skipping generation of external paste output due `--dry-run` mode being used. Re-run without `--dry-run` to generate this section.\n"
        )
        return 0
    try:
        proc = subprocess.Popen(
            shlex.split(processor), stdin=subprocess.PIPE, stdout=subprocess.PIPE
        )
        sink = IOWithRedaction(cast(IO[bytes], proc.stdin))
        output = cast(IO[bytes], proc.stdout)

        try:
            output_generator(sink)
        finally:
            sink.wrapped.close()

            stdout = output.read().decode("utf-8", errors="backslashreplace")

            output.close()
            proc.wait()

        # Expected output to be in form "<str0>\n<str1>: <str2>\n"
        # and we want str1
        pattern = re.compile("^.*\\n[a-zA-Z0-9_.-]*: .*\\n$")
        match = pattern.match(stdout)

        if not match:
            out.write(stdout)
        else:
            paste, _ = stdout.split("\n")[1].split(": ")
            out.write(paste)
        return 0
    except Exception as e:
        out.write(f"Error generating paste: {e}\n")
        return 1


@timer_decorator
def print_tail_of_log_file(out: IOWithRedaction, path: Path) -> None:
    try:
        section_title("Most recent EdenFS logs:", out)
        LOG_AMOUNT = 20 * 1024
        with path.open("r", errors="backslashreplace") as logfile:
            size = logfile.seek(0, io.SEEK_END)
            logfile.seek(max(0, size - LOG_AMOUNT), io.SEEK_SET)
            data = logfile.read()
            out.write(data)
    except Exception as e:
        out.write(f"Error reading the log file: {e}\n")


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


@timer_decorator
def print_running_eden_process(out: IOWithRedaction) -> None:
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
            format_str.format("Pid", "PPid", "Start Time", "Elapsed Time", "Command")
        )
        for line in lines:
            out.write(format_str.format(*line))
    except Exception as e:
        out.write(f"Error getting the EdenFS processes: {e}\n")
        out.write(f"{traceback.format_exc()}\n")


@timer_decorator
def print_edenfs_process_tree(out: IOWithRedaction, pid: int) -> None:
    if sys.platform != "linux":
        return
    try:
        section_title("EdenFS process tree:", out)
        output = subprocess.check_output(["ps", "-o", "sid=", "-p", str(pid)])
        sid = output.decode("utf-8", errors="backslashreplace").strip()

        output = subprocess.check_output(
            ["ps", "f", "-o", "pid,s,comm,start_time,etime,cputime,drs", "-s", sid]
        )
        out.write(output.decode("utf-8", errors="backslashreplace"))
    except Exception as e:
        out.write(f"Error getting edenfs process tree: {e}\n")


@timer_decorator
def print_eden_redirections(out: IOWithRedaction, instance: EdenInstance) -> None:
    try:
        section_title("EdenFS redirections:", out)
        checkouts = instance.get_checkouts()
        for checkout in checkouts:
            out.write(f"{checkout.path}\n")
            output = redirect_mod.prepare_redirection_list(checkout, instance)
            # append a tab at the beginning of every new line to indent
            output = output.replace("\n", "\n\t")
            out.write(f"\t{output}\n")
    except Exception as e:
        out.write(f"Error getting EdenFS redirections {e}\n")
        out.write(f"{traceback.format_exc()}\n")


@timer_decorator
def print_counters(
    out: IOWithRedaction, instance: EdenInstance, counter_type: str, regex: str
) -> None:
    try:
        section_title(f"{counter_type} counters:", out)
        with instance.get_thrift_client_legacy(timeout=3) as client:
            counters = client.getRegexCounters(regex)
            for key, value in counters.items():
                out.write(f"{key}: {value}\n")
    except Exception as e:
        out.write(f"Error getting {counter_type} Thrift counters: {e}\n")


@timer_decorator
def print_env_variables(
    out: IOWithRedaction, edenfs_instance_pid: Optional[int]
) -> None:
    try:
        section_title("Host environment variables:", out)
        for k, v in sorted(os.environ.items()):
            out.write(f"{k}={v}\n")
    except Exception as e:
        out.write(f"Error getting host environment variables: {e}\n")

    # The running daemon may have different environment variable values than
    # the host. Print both.
    # TODO: It would be nice to show a diff of the host vs daemon env vars.
    try:
        section_title("EdenFS daemon environment variables:", out)

        # On some Windows hosts, we're unable to load the psutil_windows DLL:
        # ImportError: DLL load failed while importing _psutil_windows: The specified module could not be found.
        #
        # Put the import in the try/catch for now.
        import psutil

        if edenfs_instance_pid is not None:
            p = psutil.Process(edenfs_instance_pid)
            env_variables = p.environ()
            for k, v in sorted(env_variables.items()):
                out.write(f"{k}={v}\n")
        else:
            out.write("EdenFS daemon pid could not be determined\n")
    except Exception as e:
        out.write(f"Error getting EdenFS daemon environment variables: {e}")


@timer_decorator
def print_system_mount_table(out: IOWithRedaction) -> None:
    if sys.platform == "win32":
        return
    try:
        section_title("Mount table:", out)
        output = subprocess.check_output(["mount"])
        sorted_mounts = sorted(
            output.decode("utf-8", errors="backslashreplace").split("\n")
        )
        for m in sorted_mounts:
            out.write(f"{m}\n")
    except Exception as e:
        out.write(f"Error printing system mount table: {e}\n")


def print_disk_space_usage(out: IOWithRedaction) -> None:
    cmds = [["eden", "du", "--fast"]]
    if sys.platform == "darwin":
        cmds.extend(
            [
                ["diskutil", "apfs", "list"],
                [
                    "/System/Library/Filesystems/apfs.fs/Contents/Resources/apfs.util",
                    "-G",
                    str(Path.home()),
                ],
            ]
        )
    if sys.platform == "linux":
        cmds.extend([["df", "-h"]])
    for i, cmd in enumerate(cmds):
        try:
            subprocess.run(
                cmd,
                check=True,
                stderr=subprocess.STDOUT,
                stdout=out.wrapped,
                shell=False,
            )
            if i < len(cmds) - 1:
                out.write(
                    "\n-------------------------------------------------------------------\n"
                )

        except Exception as e:
            out.write(f"Error running {cmd}: {e}\n\n")


@timer_decorator
def print_system_load(out: IOWithRedaction) -> None:
    if sys.platform not in ("darwin", "linux"):
        return

    try:
        section_title("System load:", out)
        if sys.platform == "linux":
            output = subprocess.check_output(["top", "-b", "-n1"])

            # Limit to the first 16 lines of output because top on CentOS
            # doesn't seem to have a command-line flag for this.
            output_lines = output.decode("utf-8", errors="backslashreplace").split(
                os.linesep
            )[:17] + [""]
        elif sys.platform == "darwin":
            output = subprocess.check_output(["top", "-l2", "-n10"])

            # On macOS the first iteration of `top` will have incorrect CPU
            # usage values for processes.  So here we wait for the second
            # iteration and strip the first from the output.
            output_lines = output.decode("utf-8", errors="backslashreplace").split(
                os.linesep
            )
            output_lines = output_lines[len(output_lines) // 2 :]

        out.write(os.linesep.join(output_lines))
    except Exception as e:
        out.write(f"Error printing system load: {e}\n")


def run_cmd(
    cmd: List[str], sink: IOWithRedaction, out: IOWithRedaction, timeout: float = 10
) -> None:
    try:
        subprocess.run(
            cmd,
            check=True,
            stderr=subprocess.STDOUT,
            stdout=sink.wrapped,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        out.write(f"Command {' '.join(cmd)} timed out after {timeout} seconds\n")


@timer_decorator
def print_eden_doctor(out: IOWithRedaction, processor: str, dry_run: bool) -> None:
    section_title("EdenFS doctor:", out)
    cmd = ["edenfsctl", "doctor"]
    try:
        paste_output(
            lambda sink: run_cmd(cmd, sink, out, 120),
            processor,
            out,
            dry_run,
        )
    except Exception as e:
        out.write(f"Error printing {cmd}: {e}\n")


@timer_decorator
def print_eden_config(
    out: IOWithRedaction, instance: EdenInstance, processor: str, dry_run: bool
) -> None:
    section_title("EdenFS config:", out)
    fsconfig_cmd = ["edenfsctl", "fsconfig", "--all"]

    result = paste_output(
        lambda sink: run_cmd(fsconfig_cmd, sink, out),
        processor,
        out,
        dry_run,
    )
    if result == 0:
        return

    out.write("Falling back to manually parsing config\n")
    try:
        instance.print_full_config(out.wrapped)
    except Exception as e:
        out.write(f"Error printing EdenFS config: {e}\n")


@timer_decorator
def print_prefetch_profiles_list(out: IOWithRedaction, instance: EdenInstance) -> None:
    try:
        section_title("Prefetch Profiles list:", out)
        checkouts = instance.get_checkouts()
        for checkout in checkouts:
            profiles = subprocess.check_output(
                [
                    "edenfsctl",
                    "prefetch-profile",
                    "list",
                    "--checkout",
                    f"{checkout.path}",
                ]
            )
            if profiles:
                out.write(f"{checkout.path}:\n")
                output_lines = profiles.decode(
                    "utf-8", errors="backslashreplace"
                ).split(os.linesep)
                # The first line of output is "NAMES"; skip that and only list profiles
                for name in output_lines[1:]:
                    out.write(f"  - {name}\n")
            else:
                out.write(f"{checkout.path}: []\n")
    except Exception as e:
        out.write(f"Error printing Prefetch Profiles list: {e}\n")


@timer_decorator
def print_crashed_edenfs_logs(
    out: IOWithRedaction, processor: str, dry_run: bool
) -> None:
    section_title("EdenFS crashes and dumps:", out)
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
        out.write("No crashes found\n")
        return

    section_title("EdenFS crashes and dumps:", out)
    num_uploads = 0
    # pyre-fixme[10]: Name `crashes_paths` is used but not defined.
    for crashes_path in crashes_paths:
        try:
            if not crashes_path.exists():
                continue

            # Only upload crashes from the past week.
            date_threshold = datetime.now() - timedelta(weeks=1)
            for crash in crashes_path.iterdir():
                if crash.name.startswith("edenfs"):
                    crash_time = datetime.fromtimestamp(crash.stat().st_mtime)
                    human_crash_time = crash_time.strftime("%b %d %H:%M:%S")
                    out.write(f"{str(crash.name)} from {human_crash_time}: ")
                    if crash_time > date_threshold and num_uploads <= 2:
                        num_uploads += 1
                        paste_output(
                            lambda sink, crash=crash: print_log_file(
                                crash,
                                sink,
                                tail_limit=None,
                            ),
                            processor,
                            out,
                            dry_run,
                        )
                    else:
                        out.write(" not uploaded due to age or max num dumps\n")
        except Exception as e:
            out.write(f"Error accessing crash file at {crashes_path}: {e}\n")

    out.write("\n")


@timer_decorator
def trace_running_edenfs(
    out: IOWithRedaction, processor: str, pid: int, dry_run: bool
) -> None:
    section_title("EdenFS process trace", out)
    if sys.platform == "darwin":
        trace_fn = print_sample_trace
    elif sys.platform == "win32":
        trace_fn = print_cdb_backtrace
    else:
        out.write("Traces are logged only for Windows and MacOS\n")
        return

    try:
        # pyre-fixme[10]: Name `trace_fn` is used but not defined.
        paste_output(lambda sink: trace_fn(pid, sink), processor, out, dry_run)
    except Exception as e:
        out.write(f"Error getting EdenFS trace:{e}.\n")


@timer_decorator
def print_recent_events(out: IOWithRedaction, processor: str, dry_run: bool) -> None:
    section_title("EdenFS recent events", out)
    for opt in ["thrift", "sl", "inode"]:
        trace_cmd = [
            "edenfsctl",
            "trace",
            opt,
            "--retroactive",
        ]

        try:
            out.write(f"{opt}: ")
            paste_output(
                lambda sink, trace_cmd=trace_cmd: run_cmd(trace_cmd, sink, out),
                processor,
                out,
                dry_run,
            )
        except Exception as e:
            out.write(f"Error getting EdenFS trace events: {e}.\n")


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


@timer_decorator
def print_third_party_vscode_extensions(
    out: IOWithRedaction, instance: EdenInstance
) -> None:
    problematic_extensions = (
        VSCodeExtensionsChecker().find_problematic_vscode_extensions(instance)
    )

    if problematic_extensions is None:
        return

    section_title("Visual Studio Code Extensions:", out)

    out.write("Harmful extensions installed:\n")
    for extension in problematic_extensions.harmful:
        out.write(f"{extension}\n")
    if len(problematic_extensions.harmful) == 0:
        out.write("None\n")

    out.write("\nUnsupported extensions installed:\n")
    for extension in problematic_extensions.unsupported:
        out.write(f"{extension}\n")
    if len(problematic_extensions.unsupported) == 0:
        out.write("None\n")


@timer_decorator
def print_ulimits(out: IOWithRedaction) -> None:
    if sys.platform == "win32":
        return
    try:
        section_title("ulimit -a:", out)
        output = subprocess.check_output(["ulimit", "-a"])
        out.write(output.decode("utf-8", errors="backslashreplace"))
    except Exception as e:
        out.write(f"Error retrieving ulimit values: {e}\n")
