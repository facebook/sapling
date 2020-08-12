#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import errno
import glob
import os
import subprocess
import sys
from typing import List

from . import proc_utils


def find_buck_projects_in_repo(path: str) -> List[str]:
    # This is a largely Facebook specific way to discover the likely
    # buck project locations in our repos.
    # While fbsource has a top level buckconfig, we don't really use
    # it in our projects today.  Instead, our projects tend to have
    # their own configuration files one level down.  This glob()
    # finds those directories for us.
    buck_configs = glob.glob(f"{path}/*/.buckconfig")
    projects = [os.path.dirname(config) for config in buck_configs]
    if os.path.isfile(f"{path}/.buckconfig"):
        projects.append(path)
    return projects


def is_buckd_running_for_path(path: str) -> bool:
    pid_file = os.path.join(path, ".buckd", "pid")
    try:
        with open(pid_file, "r") as f:
            buckd_pid = int(f.read().strip())
    except OSError as exc:
        if exc.errno == errno.ENOENT:
            return False
        raise

    return proc_utils.new().is_process_alive(buckd_pid)


def stop_buckd_for_path(path: str) -> None:
    print(f"Stopping buck in {path}...")

    # Using BUCKVERSION=last here to avoid triggering a download of a new
    # version of buck just to kill off buck.  This is specific to Facebook's
    # deployment of buck, and has no impact on the behavior of the opensource
    # buck executable.
    # On Windows, "last" doesn't work, fallback to reading the .buckversion file.
    if sys.platform != "win32":
        buckversion = "last"
    else:
        with open(os.path.join(path, ".buckversion"), "r") as f:
            buckversion = f.read().strip()

    env = os.environ.copy()
    env["BUCKVERSION"] = buckversion

    subprocess.run(
        ["buck", "kill"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=path,
        env=env,
        check=True,
    )


def stop_buckd_for_repo(path: str) -> None:
    """Stop the major buckd instances that are likely to be running for path"""
    for project in find_buck_projects_in_repo(path):
        if is_buckd_running_for_path(project):
            stop_buckd_for_path(project)


def buck_clean_repo(path: str) -> None:
    for project in find_buck_projects_in_repo(path):
        print(f"Cleaning buck in {project}...")
        subprocess.run(
            # Using BUCKVERSION=last here to avoid triggering a download
            # of a new version of buck just to remove some dirs
            # This is specific to Facebook's deployment of buck, and has
            # no impact on the behavior of the opensource buck executable.
            ["env", "NO_BUCKD=true", "BUCKVERSION=last", "buck", "clean"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            cwd=project,
        )
