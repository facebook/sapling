#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import subprocess
from typing import Optional, Tuple
from . import config as config_mod


def get_installed_eden_rpm_version() -> str:
    return subprocess.check_output(
        ['rpm', '-q', 'fb-eden', '--queryformat', '%{version}-%{release}']
    ).decode('utf-8')


# returns (runing_version, release) tuple
def get_running_eden_version_parts(
        config: config_mod.Config) -> Tuple[Optional[str], Optional[str]]:
    bi = config.get_server_build_info()
    return (bi.get('build_package_version'), bi.get('build_package_release'))


def format_running_eden_version(
        parts: Tuple[Optional[str], Optional[str]]) -> str:
    running_version, release = parts
    if running_version is None:
        running_version = ''
    if release is None:
        release = ''
    return f'{running_version}-{release}'


def get_running_eden_version(config: config_mod.Config) -> str:
    return format_running_eden_version(get_running_eden_version_parts(config))
