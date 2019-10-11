#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

import os


def find_buck_out(manifest_path):
    dir = manifest_path
    while dir:
        dir = os.path.dirname(dir)
        if os.path.exists(os.path.join(dir, "project_root")):
            return dir
    m = "%s does not appear to be in a buck-out directory" % manifest_path
    raise ValueError(m)
