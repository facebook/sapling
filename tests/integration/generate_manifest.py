#!/usr/bin/env python3
# Copyright (c) 2019-present, Facebook, Inc.
# All Rights Reserved.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os
import sys
import json


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

    # ... and we provide locations through the environment
    manifest = {k: map_name(k) for k in sorted(names)}

    # INSTALL_DIR is also provided by Buck's custom_rule
    out = os.path.join(os.environ["INSTALL_DIR"], out)
    with open(out, "w") as f:
        json.dump(manifest, f, indent=2)


if __name__ == "__main__":
    main()
