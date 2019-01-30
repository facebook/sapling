from __future__ import absolute_import, print_function

import os
import sys

from edenscm.hgext import patchrmdir
from edenscm.mercurial import util


if not sys.platform.startswith("linux"):
    sys.stderr.write("skipped: linux required\n")
    sys.exit(80)


patchrmdir.uisetup(None)

testtmp = os.environ["TESTTMP"]

d1 = os.path.join(testtmp, "d1")
d2 = os.path.join(testtmp, "d1", "d2")

os.mkdir(d1)
os.mkdir(d2)


def tryfunc(func):
    try:
        func()
    except Exception as ex:
        # normalize the error message across platforms
        if "Non-empty directory" not in str(ex):
            ex = "*"
        else:
            ex = "Non-empty directory"
        print("  error: %s" % ex)
    else:
        print("  success")


print("rmdir d1 - should fail with ENOTEMPTY")
tryfunc(lambda: os.rmdir(d1))

print("rmdir d1/d2 - should succeed")
tryfunc(lambda: os.rmdir(d2))

open(d2, "w").close()

print("rmdir d1 - should fail with ENOTEMPTY")
tryfunc(lambda: os.rmdir(d1))

os.unlink(d2)

print("rmdir d1 - should succeed")
tryfunc(lambda: os.rmdir(d1))

print("rmdir d1 - should fail with ENOENT")
tryfunc(lambda: os.rmdir(d1))

os.mkdir(d1)
os.mkdir(d2)

print("removedirs d2 (and d1) - should succeed")
tryfunc(lambda: util.removedirs(d2))

print("removedirs d1 - should fail with ENOENT")
tryfunc(lambda: util.removedirs(d1))
