#!/usr/bin/env python3
# Copyright (c) 2019-present, Facebook, Inc.
# All Rights Reserved.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os


def find_buck_out(manifest_path):
    dir = manifest_path
    while dir:
        dir = os.path.dirname(dir)
        if os.path.exists(os.path.join(dir, "project_root")):
            return dir
    m = "%s does not appear to be in a buck-out directory" % manifest_path
    raise ValueError(m)
