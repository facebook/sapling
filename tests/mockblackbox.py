from __future__ import absolute_import
from mercurial import (
    util,
)

# XXX: we should probably offer a devel option to do this in blackbox directly
def getuser():
    return 'bob'
def getpid():
    return 5000

# mock the date and user apis so the output is always the same
def uisetup(ui):
    util.getuser = getuser
    util.getpid = getpid
