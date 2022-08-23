# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# An extension to make identifiers from util.makerandomidentifier into a stable
# incrementing sequence.
import os

from edenscm.ext import extutil
from edenscm.mercurial import extensions, util


def makestableidentifier(orig, length=16):
    stableidentifierfile = os.path.join(os.environ["TESTTMP"], "stableidentifier")
    with extutil.flock(stableidentifierfile, "stableidentifier"):
        try:
            with open(stableidentifierfile) as f:
                coid = int(f.read().strip())
        except Exception:
            coid = 0
        with open(stableidentifierfile, "w") as f:
            f.write("%s\n" % (coid + 1))
    return "%0*d" % (length, coid)


def reposetup(ui, repo):
    assert ui._correlator.get() is None
    ui._correlator.swap("stableidentifiers:correlator")


def uisetup(ui):
    extensions.wrapfunction(util, "makerandomidentifier", makestableidentifier)
