# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
from pathlib import Path


def checkpath(path, dllnames):
    content = open(path).read()
    for name in dllnames:
        for matched in sorted(set(re.findall(r"%s\.[^(.]*\(" % name, content))):
            # remove tailing "("
            funcname = matched[:-1]
            for expected in ["%s.argtypes" % funcname, "%s.restype" % funcname]:
                if expected not in content:
                    print("%s needs explicit argtypes and restype" % funcname)
                    break


checkpath(
    f"{Path(__file__).parent}/../sapling/win32.py",
    ["_kernel32", "_advapi32", "_user32", "_crypt32"],
)
