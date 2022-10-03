# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import os
import sys


if os.path.exists(".hg"):
    hgrcpath = ".hg/hgrc"
else:
    hgrcpath = os.getenv("HGRCPATH")
    if os.pathsep in hgrcpath:
        hgrcpath = hgrcpath.split(os.pathsep)[-1]

content = ""

for config in sys.argv[1:]:
    section, namevalue = config.split(".", 1)
    content += "[%s]\n%s\n" % (section, namevalue)

with open(hgrcpath, "a") as f:
    f.write(content)
