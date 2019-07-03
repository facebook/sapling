#!/usr/bin/env python3
# Copyright (c) 2019-present, Facebook, Inc.
# All Rights Reserved.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os
import sys
import json

from mononoke.tests.integration.lib_buck import find_buck_out


def map_name(k):
    v = os.environ[k]

    # Hg is actually within a file there.
    if k == "BINARY_HG":
        return os.path.join(v, "hg")

    # We want our test root to be the directory that actually contains the
    # files.
    if k == "TEST_ROOT_FACEBOOK":
        return os.path.join(v, "facebook")

    return v


def main():
    # We provide the output file and names as argument
    _, out, *names = sys.argv

    # The INSTALL_DIR is provided by Buck's custom_rule.
    out = os.path.join(os.environ["INSTALL_DIR"], out)

    # Locations are provided through the environment (using Buck location
    # macro). The paths we output must be relative to buck_out, since they might
    # have been built on a different host so we must avoid absolute paths.
    buck_out = find_buck_out(out)

    manifest = {
        k: os.path.relpath(map_name(k), buck_out)
        for k in sorted(names)
    }

    with open(out, "w") as f:
        json.dump(manifest, f, indent=2)


if __name__ == "__main__":
    main()
