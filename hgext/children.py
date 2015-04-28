# Mercurial extension to provide the 'hg children' command
#
# Copyright 2007 by Intevation GmbH <intevation@intevation.de>
#
# Author(s):
# Thomas Arendsen Hein <thomas@intevation.de>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''command to display child changesets (DEPRECATED)

This extension is deprecated. You should use :hg:`log -r
"children(REV)"` instead.
'''

from mercurial import cmdutil
from mercurial.commands import templateopts
from mercurial.i18n import _

cmdtable = {}
command = cmdutil.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'

@command('children',
    [('r', 'rev', '',
     _('show children of the specified revision'), _('REV')),
    ] + templateopts,
    _('hg children [-r REV] [FILE]'),
    inferrepo=True)
def children(ui, repo, file_=None, **opts):
    """show the children of the given or working directory revision

    Print the children of the working directory's revisions. If a
    revision is given via -r/--rev, the children of that revision will
    be printed. If a file argument is given, revision in which the
    file was last changed (after the working directory revision or the
    argument to --rev if given) is printed.
    """
    rev = opts.get('rev')
    if file_:
        fctx = repo.filectx(file_, changeid=rev)
        childctxs = [fcctx.changectx() for fcctx in fctx.children()]
    else:
        ctx = repo[rev]
        childctxs = ctx.children()

    displayer = cmdutil.show_changeset(ui, repo, opts)
    for cctx in childctxs:
        displayer.show(cctx)
    displayer.close()
