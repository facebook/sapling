#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import subprocess
import sys
from subprocess import CalledProcessError
from typing import Dict, List

from .util import get_environment_suitable_for_subprocess


def get_buck_command() -> str:
    return "buck2"


def get_env_with_buck_version(path: str) -> Dict[str, str]:
    env = get_environment_suitable_for_subprocess()

    # Using BUCKVERSION=last here to avoid triggering a download of a new
    # version of buck just to kill off buck.  This is specific to Facebook's
    # deployment of buck, and has no impact on the behavior of the opensource
    # buck executable.
    # On Windows, "last" doesn't work, fallback to reading the .buck-java11 file.
    if sys.platform != "win32":
        buckversion = "last"
    else:
        buckversion = subprocess.run(
            [get_buck_command(), "--version-fast"],
            stdout=subprocess.PIPE,
            cwd=path,
            encoding="utf-8",
        ).stdout.strip()

    env["BUCKVERSION"] = buckversion

    return env


def is_buckd_running_for_repo(path: str) -> bool:
    buck_status = subprocess.run(
        [get_buck_command(), "status"],
        stdout=subprocess.PIPE,
        cwd=path,
        encoding="utf-8",
    ).stdout.strip()
    if buck_status.find("no buckd running") != -1:
        return True
    return False


# Buck is sensitive to many environment variables, so we need to set them up
# properly before calling into buck
def run_buck_command(
    buck_command: List[str], path: str
) -> "subprocess.CompletedProcess[bytes]":

    env = get_env_with_buck_version(path)
    try:
        return subprocess.run(
            buck_command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=path,
            env=env,
            check=True,
        )
    except CalledProcessError as e:
        print(
            f"{e}\n\nFailed to kill buck. Please manually run `buck2 kill` in `{path}`"
        )
        raise e


def stop_buckd_for_repo(path: str) -> None:
    """Stop the major buck2d instances that are likely to be running for path"""
    if is_buckd_running_for_repo(path):
        print(f"Stopping buck2 in {path}...")
        run_buck_command([get_buck_command(), "kill"], path)


def buck_clean_repo(path: str) -> None:
    print(f"Cleaning buck2 in {path}...")
    subprocess.run(
        # Using BUCKVERSION=last here to avoid triggering a download
        # of a new version of buck just to remove some dirs
        # This is specific to Facebook's deployment of buck, and has
        # no impact on the behavior of the opensource buck executable.
        ["env", "NO_BUCKD=true", "BUCKVERSION=last", get_buck_command(), "clean"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        cwd=path,
    )
