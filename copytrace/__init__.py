from mercurial.extensions import wrapfunction, wrapcommand
from mercurial import extensions, commands, copies, cmdutil, exchange
from hgext import rebase
import filldb
import copytrace
import bundle2


def extsetup(ui):
    if ui.configbool("copytrace", "enablefilldb", False):
        wrapfunction(cmdutil, 'commit', filldb.commit)
        wrapfunction(cmdutil, 'amend', filldb.amend)
        wrapfunction(rebase, 'concludenode', filldb.concludenode)

    if ui.configbool("copytrace", "enablecopytracing", False):
        wrapfunction(copies, 'mergecopies', copytrace.mergecopieswithdb)
        wrapfunction(copies, 'pathcopies', copytrace.pathcopieswithdb)
        wrapfunction(rebase, 'buildstate', copytrace.buildstate)

    if ui.configbool("copytrace", "enablebundle2", False):
        wrapfunction(exchange, '_pullbundle2extraprepare',
                    bundle2._pullbundle2extraprepare)
