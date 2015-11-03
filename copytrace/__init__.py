from mercurial.extensions import wrapfunction, wrapcommand
from mercurial import extensions, commands, copies, cmdutil
from hgext import rebase
import filldb
import copytrace

def extsetup(ui):
    wrapfunction(cmdutil, 'commit', filldb.commit)
    wrapfunction(cmdutil, 'amend', filldb.amend)
    wrapfunction(rebase, 'concludenode', filldb.concludenode)

    wrapfunction(copies, 'mergecopies', copytrace.mergecopieswithdb)
    wrapfunction(copies, 'pathcopies', copytrace.pathcopieswithdb)


