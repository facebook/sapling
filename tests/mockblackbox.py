from mercurial import util

def makedate():
    return 0, 0
def getuser():
    return 'bob'

# mock the date and user apis so the output is always the same
def uisetup(ui):
    util.makedate = makedate
    util.getuser = getuser
