#!/usr/bin/env python2
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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
