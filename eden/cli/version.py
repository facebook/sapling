#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import subprocess
from typing import Optional, Tuple, cast


# returns (installed_version, release) tuple
def get_installed_eden_rpm_version_parts() -> Tuple[Optional[str], Optional[str]]:
    fields = ("version", "release")
    query_fmt = r"\n---\n".join(f"%{{{f}}}" for f in fields)
    cmd = ["rpm", "-q", "fb-eden", "--queryformat", query_fmt]
    proc = subprocess.run(cmd, stdout=subprocess.PIPE, encoding="utf-8")
    if proc.returncode != 0:
        return "<Not Installed>", None
    # pyre-fixme[22]: The cast is redundant.
    parts = cast(str, proc.stdout).split("\n---\n")
    assert len(parts) == 2, f"unexpected output: {proc.stdout!r}"
    return (parts[0], parts[1])


def format_eden_version(parts: Tuple[Optional[str], Optional[str]]) -> str:
    version = parts[0] or ""
    release = parts[1] or ""
    return f"{version}-{release}"


def get_installed_eden_rpm_version() -> str:
    return format_eden_version(get_installed_eden_rpm_version_parts())
