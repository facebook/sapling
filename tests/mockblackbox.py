from __future__ import absolute_import
from mercurial import (
    util,
)

def makedate():
    return 0, 0
def getuser():
    return 'bob'
def getpid():
    return 5000

# mock the date and user apis so the output is always the same
def uisetup(ui):
    util.makedate = makedate
    util.getuser = getuser
    util.getpid = getpid
