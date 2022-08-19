#!/usr/bin/env python
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# like ls -l, but do not print date, user, or non-common mode bit, to avoid
# using globs in tests.
from __future__ import absolute_import, print_function

import os
import stat
import sys


def modestr(st):
    mode = st.st_mode
    result = ""
    if mode & stat.S_IFDIR:
        result += "d"
    else:
        result += "-"
    for owner in ["USR", "GRP", "OTH"]:
        for action in ["R", "W", "X"]:
            if mode & getattr(stat, "S_I%s%s" % (action, owner)):
                result += action.lower()
            else:
                result += "-"
    return result


def sizestr(st):
    if st.st_mode & stat.S_IFREG:
        return "%7d" % st.st_size
    else:
        # do not show size for non regular files
        return " " * 7


paths = sys.argv[1:]
if not paths:
    paths = ["."]


def print_lf(s):
    sys.stdout.buffer.write(s.encode() + b"\n")


for path in paths:
    if os.path.isdir(path):
        os.chdir(path)

        for name in sorted(os.listdir(".")):
            st = os.stat(name)
            print_lf("%s %s %s" % (modestr(st), sizestr(st), name))
    else:
        st = os.stat(path)
        print_lf("%s %s %s" % (modestr(st), sizestr(st), os.path.abspath(path)))

sys.stdout.flush()
