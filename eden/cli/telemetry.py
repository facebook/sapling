#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import getpass
import json
import logging
import platform
import random
import socket
from typing import Optional, Tuple, TypeVar

from . import version


log = logging.getLogger(__name__)

_session_id: Optional[int] = None

T = TypeVar("T", bound="TelemetryPayload")


class TelemetryPayload:
    def __init__(self):
        self.ints = {}
        self.strings = {}
        self.doubles = {}

    def add_int(self: T, name: str, value: int) -> T:
        self.ints[name] = value
        return self

    def add_string(self: T, name: str, value: str) -> T:
        self.strings[name] = value
        return self

    def add_double(self: T, name: str, value: float) -> T:
        self.doubles[name] = value
        return self

    def add_bool(self: T, name: str, value: bool) -> T:
        self.ints[name] = int(value)
        return self

    def get_json(self: T) -> str:
        data = {}
        data["int"] = self.ints
        data["normal"] = self.strings
        if self.doubles:
            data["double"] = self.doubles
        return json.dumps(data)


def get_session_id() -> int:
    global _session_id
    sid = _session_id
    if sid is None:
        sid = random.randrange(2 ** 32)
        _session_id = sid
    return sid


def get_user() -> str:
    return getpass.getuser()


def get_host() -> str:
    return socket.gethostname()


def get_os_and_ver() -> Tuple[str, str]:
    os = platform.system()
    if os == "Darwin":
        os = "macOS"
    if os == "":
        os = "unknown"

    ver = platform.release()
    if ver == "":
        ver = "unknown"

    return os, ver


def get_eden_ver() -> str:
    # TODO: use generated information from  __manifest__.py for this
    # instead of querying the RPM database to determine the version
    edenver = version.get_installed_eden_rpm_version()
    if edenver is None:
        edenver = ""
    return edenver


def build_base_sample(log_type: str) -> TelemetryPayload:
    sample = TelemetryPayload()
    sample.add_string("type", log_type)

    try:
        session_id = get_session_id()
        sample.add_int("session_id", session_id)

        user = get_user()
        sample.add_string("user", user)

        host = get_host()
        sample.add_string("host", host)

        os, os_ver = get_os_and_ver()
        sample.add_string("os", os)
        sample.add_string("osver", os_ver)

        edenver = get_eden_ver()
        sample.add_string("edenver", edenver)

        return sample
    except Exception as ex:
        log.warning(f"unable to build base log sample due to {ex}")
        return sample
