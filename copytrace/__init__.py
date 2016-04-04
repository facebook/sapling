from mercurial.extensions import wrapfunction, wrapcommand
from mercurial import extensions, commands, copies, cmdutil, exchange, wireproto
from hgext import rebase
from mercurial.i18n import _
import filldb
import copytrace
import bundle2


cmdtable = {}
command = cmdutil.command(cmdtable)


def uisetup(ui):
    # Generating this extension in the end so as to have the bundle2 part after
    # the 'pushrebase' one
    order = extensions._order
    order.remove('copytrace')
    order.append('copytrace')
    extensions._order = order

    # Creating the 'fillmvdb' command
    command('^fillmvdb', [
        ('', 'stop', '-1', _('stopping rev -- not included')),
        ('', 'start', '.', _('starting rev -- included'))
        ] , '') (filldb.fillmvdb)


def extsetup(ui):
    # - Enablefilldb allows the local database to be filled when using commit,
    # amend or rebase
    # - Enablebundle2 allows to exchange move data with the server during pulls
    # and pushs
    # - Enablecopytracing allows the use of the local database to do
    # copytracing during rebases, 'hg st -C', ...

    # /!\
    # Enablecopytracing should not be used if Enablefilldb and Enablebundle2
    # are not allowed on every client. It would be very slow since it will
    # manually calculate and add to the local database all missing moves not
    # present in the server database

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
        # Adding the 'movedatareq' argument to the ones accepted by bundle2
        wireproto.gboptsmap['movedatareq'] = 'nodes'

        # Generating this part last so as to handle after 'pushrebase' if that
        # extension is loaded
        partorder = exchange.b2partsgenorder
        partorder.insert(len(partorder),
                         partorder.pop(partorder.index('push:movedata')))
