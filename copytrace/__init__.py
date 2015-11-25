from mercurial.extensions import wrapfunction, wrapcommand
from mercurial import extensions, commands, copies, cmdutil, exchange, wireproto
from hgext import rebase
import filldb
import copytrace
import bundle2


def uisetup(ui):
    # Generating this extension in the end so as to have the bundle2 part after
    # the 'pushrebase' one
    order = extensions._order
    order.remove('copytrace')
    order.append('copytrace')
    extensions._order = order


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
        # Adding the options to the ones accepted by bundle2
        wireproto.gboptsmap['movedatareq'] = 'nodes'

        # Generating this part last so as to handle after 'pushrebase' if that
        # extension is loaded
        partorder = exchange.b2partsgenorder
        partorder.insert(len(partorder),
                         partorder.pop(partorder.index('push:movedata')))
