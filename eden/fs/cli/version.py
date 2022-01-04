#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import subprocess
from typing import Optional, Tuple, cast


# returns (installed_version, release) tuple
def get_installed_eden_rpm_version_parts() -> Optional[Tuple[str, str]]:
    fields = ("version", "release")
    query_fmt = r"\n---\n".join(f"%{{{f}}}" for f in fields)
    cmd = ["rpm", "-q", "fb-eden", "--queryformat", query_fmt]
    proc = subprocess.run(cmd, stdout=subprocess.PIPE, encoding="utf-8")
    if proc.returncode != 0:
        return None
    # pyre-fixme[22]: The cast is redundant.
    parts = cast(str, proc.stdout).split("\n---\n")
    assert len(parts) == 2, f"unexpected output: {proc.stdout!r}"
    return (parts[0], parts[1])


def format_eden_version(parts: Optional[Tuple[str, str]]) -> str:
    if parts is None:
        return "<Not Installed>"
    version = parts[0] or ""
    release = parts[1] or ""
    return f"{version}-{release}"


def get_installed_eden_rpm_version() -> str:
    return format_eden_version(get_installed_eden_rpm_version_parts())


def get_current_version_parts() -> Tuple[str, str]:
    """Get a tuple containing (version, release) of the currently running code.

    The version and release strings will both be the empty string if this code is part
    of a development build that is not a released package.
    """
    import eden.config

    return (eden.config.VERSION, eden.config.RELEASE)


def get_current_version() -> str:
    """Get a human-readable string describing the version of the currently running code.

    Returns "-" when running a development build that is not part of an official
    versioned release.
    """
    return format_eden_version(get_current_version_parts())
