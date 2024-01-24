#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
import socket

try:
    from eden.fs.cli.facebook.hostcaps import normalize_hostname
except ImportError:

    def normalize_hostname(hostname: str) -> str:
        return hostname


def get_normalized_hostname() -> str:
    """Get the system's normalized hostname for logging and telemetry purposes."""

    return normalize_hostname(socket.gethostname())


def get_host_prefix(hostname: str) -> str:
    """
    Get the prefix of a hostname, e.g., "devvm", "od".

    Returns an empty string if no prefix is found.
    """
    match = re.match(r"([a-zA-Z\-]+)\d+.*", hostname)
    if not match:
        return ""

    return match.group(1)
