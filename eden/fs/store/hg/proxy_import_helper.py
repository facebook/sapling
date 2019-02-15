#!/usr/bin/env python2
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import sys


# This script only exists to help transition old running edenfs servers.
# Newer versions of edenfs will directly run `hg debugedenimporthelper`
# rather than this script.
# The script uses hg.real because that is what we use in the integration
# tests; they do not pass without it.  hg.real is part of the mercurial
# packaging and should outlive this compatibility script.

hg_real = os.environ.get("HG_REAL_BIN", "hg.real")

env = dict(os.environ)
env["HGPLAIN"] = "1"
env["CHGDISABLE"] = "1"

args = [hg_real, "debugedenimporthelper"] + sys.argv[1:]
os.execvpe(args[0], args, env)
