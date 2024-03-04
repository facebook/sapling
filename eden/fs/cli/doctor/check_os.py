#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import platform
import re
import subprocess
import sys
from typing import Optional, Tuple

from eden.fs.cli import ui
from eden.fs.cli.config import EdenInstance
from eden.fs.cli.doctor.problem import Problem, ProblemTracker


class OSProblem(Problem):
    pass


class ProjFsBugProblem(Problem):
    pass


def _parse_os_kernel_version(version: str) -> Tuple[int, ...]:
    """Parses kernel version string.
    Example version string: 4.11.3-67_fbk17_4093_g2bf19e7a0b95
    Returns integer representations of the version, eg. (4, 11, 3, 67).
    """
    version = re.sub(r"[_-]", ".", version)
    split_version = version.split(".")[:4]
    parsed_kernel_version = tuple(map(int, split_version))
    if len(parsed_kernel_version) < 4:
        # right pad with zeros if the kernel version isn't 4 numbers
        # pyre-fixme[60]: Concatenation not yet support for multiple variadic
        #  tuples: `*parsed_kernel_version,
        #  *[0].__mul__(4.__sub__(len(parsed_kernel_version)))`.
        parsed_kernel_version = (
            *parsed_kernel_version,
            *[0] * (4 - len(parsed_kernel_version)),
        )
    return parsed_kernel_version


def _os_is_kernel_version_too_old(instance: EdenInstance, release: str) -> bool:
    min_kernel_version = instance.get_config_value(
        "doctor.minimum-kernel-version", default=""
    )
    if not min_kernel_version:
        return False
    try:
        return _parse_os_kernel_version(release) < _parse_os_kernel_version(
            min_kernel_version
        )
    except ValueError:
        # If the kernel version failed to parse because one of the
        # components wasn't an int, whatever.
        return False


def _os_is_bad_release(instance: EdenInstance, release: str) -> bool:
    known_bad_kernel_versions = instance.get_config_value(
        "doctor.known-bad-kernel-versions", default=""
    )
    if not known_bad_kernel_versions:
        return False
    for regex in known_bad_kernel_versions.split(","):
        if re.search(regex, release):
            return True  # matched known bad release
    return False  # no match to bad release


def _run_linux_os_checks(
    tracker: ProblemTracker, instance: EdenInstance, out: ui.Output
) -> None:
    # get kernel version string; same as "uname -r"
    current_kernel_release = platform.release()

    # check if version too low
    result = _os_is_kernel_version_too_old(instance, current_kernel_release)
    if result:
        tracker.add_problem(
            OSProblem(
                # TODO: Reword these messages prior to public release
                description=f"Kernel version {current_kernel_release} too low.",
                remediation=f"Reboot to upgrade kernel version.",
            )
        )
        # if the kernel version is too low, return here as continuing to
        # further checks has no benefit
        return

    # check against known bad versions
    result = _os_is_bad_release(instance, current_kernel_release)
    if result:
        tracker.add_problem(
            OSProblem(
                # TODO: Reword these messages prior to public release
                description=f"Kernel {current_kernel_release} is a known "
                + "bad kernel.",
                remediation="Reboot to upgrade kernel version.",
            )
        )
        return


def _windows_has_projfs_bug(build: int, revision: int) -> bool:
    # Check for KB5022906, which has the fix for the ProjFS bug.
    return build <= 10945 and revision < 2673


_WINDOWS_VERSION_PATTERN: "re.Pattern[str]" = re.compile(
    r"""Microsoft Windows \[Version (?P<major>\d+)\.(?P<minor>\d+)\.(?P<build>\d+)\.(?P<revision>\d+)\]"""
)


def _get_windows_build_and_revision() -> Optional[Tuple[int, int]]:
    # With Python3.8 on Windows 10 build 10944, sys.getwindowsversion() doesn't
    # include the build revision and platform.version() is wrong.  So we shell
    # out to `ver` to get the build and revision instead.
    try:
        ver = subprocess.run(
            "C:\\Windows\\system32\\cmd.exe /c ver",
            capture_output=True,
            check=True,
            text=True,
        )
    except subprocess.CalledProcessError:
        return None
    except UnicodeDecodeError:
        return None

    match = _WINDOWS_VERSION_PATTERN.search(ver.stdout)
    if not match:
        return None

    try:
        build = int(match.group("build"))
        revision = int(match.group("revision"))
    except ValueError:
        return None

    return (build, revision)


def _run_windows_os_checks(
    tracker: ProblemTracker, instance: EdenInstance, out: ui.Output
) -> None:
    build_revision = _get_windows_build_and_revision()
    if not build_revision:
        tracker.add_problem(
            OSProblem(description="Unable to determine Windows build and revision")
        )
        return

    build, revision = build_revision
    if _windows_has_projfs_bug(build, revision):
        tracker.add_problem(
            ProjFsBugProblem(
                description=f"Windows build {build}, revision {revision} doesn't include fix for ProjFS bug",
                remediation="Update to the latest available version of Windows",
            )
        )


def run_operating_system_checks(
    tracker: ProblemTracker, instance: EdenInstance, out: ui.Output
) -> None:
    if sys.platform == "linux":
        _run_linux_os_checks(tracker, instance, out)
    elif sys.platform == "win32":
        _run_windows_os_checks(tracker, instance, out)
