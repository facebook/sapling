#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import errno
import glob
import os
import subprocess
import sys
from typing import Dict, List

from . import proc_utils
from .util import get_environment_suitable_for_subprocess

# In the EdenFS buck integration tests we build buck from source
# in these tests we need to use the source built buck. The path for
# this will be in the SOURCE_BUILT_BUCK environment variable. Otherwise we use
# the default buck in our path.
def get_buck_command() -> str:
    return os.environ.get("SOURCE_BUILT_BUCK", "buck")


def get_env_with_buck_version(path: str) -> Dict[str, str]:
    env = get_environment_suitable_for_subprocess()
    if os.environ.get("SOURCE_BUILT_BUCK") is not None:
        # If we are going to use locally built buck we don't need to set a buck
        # version. The locally build buck will only use the locally built
        # version
        return env

    # Using BUCKVERSION=last here to avoid triggering a download of a new
    # version of buck just to kill off buck.  This is specific to Facebook's
    # deployment of buck, and has no impact on the behavior of the opensource
    # buck executable.
    # On Windows, "last" doesn't work, fallback to reading the .buck-java11 file.

    # TODO(T135622175): setting `BUCKVERSION=last` has caused issues with repos that are checked
    # out to commits that are more than a few days old. For now, let's disable this code path.
    # This will hinder performance a bit, but it should make `buck kill` more reliable.
    if False:
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
    except ValueError:
        return False
    except OSError as exc:
        if exc.errno == errno.ENOENT:
            return False
        if exc.errno == errno.EINVAL and sys.platform == "win32":
            print(
                f"Could not read project {path} buck pid file due to Eden stopping too recently. Attempting to proceed, please try again in a few minutes on failure."
            )
            return False

        raise

    return proc_utils.new().is_process_alive(buckd_pid)


# Buck is sensitive to many environment variables, so we need to set them up
# properly before calling into buck
def run_buck_command(
    buck_command: List[str], path: str
) -> "subprocess.CompletedProcess[bytes]":

    env = get_env_with_buck_version(path)
    return subprocess.run(
        buck_command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=path,
        env=env,
        check=True,
    )


def stop_buckd_for_path(path: str) -> None:
    print(f"Stopping buck in {path}...")

    run_buck_command([get_buck_command(), "kill"], path)


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
            ["env", "NO_BUCKD=true", "BUCKVERSION=last", get_buck_command(), "clean"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            cwd=project,
        )
