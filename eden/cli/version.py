#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import subprocess
from typing import Optional, Tuple, cast

from .config import EdenInstance


def get_installed_eden_rpm_version() -> str:
    proc = subprocess.run(
        ["rpm", "-q", "fb-eden", "--queryformat", "%{version}-%{release}"],
        stdout=subprocess.PIPE,
        encoding="utf-8",
    )
    if proc.returncode != 0:
        return "<Not Installed>"
    return cast(str, proc.stdout)


# returns (runing_version, release) tuple
def get_running_eden_version_parts(
    instance: EdenInstance
) -> Tuple[Optional[str], Optional[str]]:
    bi = instance.get_server_build_info()
    return (bi.get("build_package_version"), bi.get("build_package_release"))


def format_running_eden_version(parts: Tuple[Optional[str], Optional[str]]) -> str:
    running_version, release = parts
    if running_version is None:
        running_version = ""
    if release is None:
        release = ""
    return f"{running_version}-{release}"


def get_running_eden_version(instance: EdenInstance) -> str:
    return format_running_eden_version(get_running_eden_version_parts(instance))
