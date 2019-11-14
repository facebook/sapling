# An extension to make identifiers from util.makerandomidentifier into a stable
# incrementing sequence.
import os

from edenscm.hgext import extutil
from edenscm.mercurial import extensions, util


def makestableidentifier(orig, length=16):
    stableidentifierfile = os.path.join(os.environ["TESTTMP"], "stableidentifier")
    with extutil.flock(stableidentifierfile, "stableidentifier"):
        try:
            coid = int(open(stableidentifierfile).read().strip())
        except Exception:
            coid = 0
        with open(stableidentifierfile, "w") as f:
            f.write("%s\n" % (coid + 1))
    return "%0*d" % (length, coid)


def uisetup(ui):
    extensions.wrapfunction(util, "makerandomidentifier", makestableidentifier)
