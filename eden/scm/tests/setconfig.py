from __future__ import absolute_import

import os
import sys


if os.path.exists(".hg"):
    hgrcpath = ".hg/hgrc"
else:
    hgrcpath = os.getenv("HGRCPATH")

content = ""

for config in sys.argv[1:]:
    section, namevalue = config.split(".", 1)
    content += "[%s]\n%s\n" % (section, namevalue)

with open(hgrcpath, "a") as f:
    f.write(content)
