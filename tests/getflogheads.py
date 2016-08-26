from mercurial import cmdutil, hg
from mercurial.i18n import _

cmdtable = {}
command = cmdutil.command(cmdtable)

@command('getflogheads',
         [],
         'path')
def getflogheads(ui, repo, path):
    """
    Extension printing a remotefilelog's heads

    Used for testing purpose
    """

    dest = repo.ui.expandpath('default')
    peer = hg.peer(repo, {}, dest)

    flogheads = peer.getflogheads(path)

    if flogheads:
        for head in flogheads:
            ui.write(head + '\n')
    else:
        ui.write(_('EMPTY\n'))
