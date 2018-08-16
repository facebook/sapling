#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import argparse
import os

from . import util
from .config import EdenInstance


# Relative to the user's $HOME/%USERPROFILE% directory.
# TODO: This value should be .eden outside of Facebook devservers.
DEFAULT_CONFIG_DIR = "local/.eden"


def find_default_config_dir(home_dir: str) -> str:
    """Returns the path to default Eden config directory.

    Note that the path is not guaranteed to correspond to an existing directory.
    """
    return os.path.join(home_dir, DEFAULT_CONFIG_DIR)


def get_eden_instance(args: argparse.Namespace) -> EdenInstance:
    home_dir = args.home_dir or util.get_home_dir()
    state_dir = args.config_dir or find_default_config_dir(home_dir)
    return EdenInstance(state_dir, args.etc_eden_dir, home_dir)
