# Mercurial extension to provide the 'hg children' command
#
# Copyright 2007 by Intevation GmbH <intevation@intevation.de>
# Author(s):
# Thomas Arendsen Hein <thomas@intevation.de>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial import cmdutil, util
from mercurial.i18n import _
from mercurial.node import nullid


def children(ui, repo, file_=None, **opts):
    """show the children of the given or working dir revision

    Print the children of the working directory's revisions.
    If a revision is given via --rev, the children of that revision
    will be printed. If a file argument is given, revision in
    which the file was last changed (after the working directory
    revision or the argument to --rev if given) is printed.
    """
    rev = opts.get('rev')
    if file_:
        ctx = repo.filectx(file_, changeid=rev)
    else:
        ctx = repo.changectx(rev)
    if ctx.node() == nullid:
        raise util.Abort(_("All non-merge changesets are children of "
                           "the null revision!"))

    displayer = cmdutil.show_changeset(ui, repo, opts)
    for node in [cp.node() for cp in ctx.children()]:
        displayer.show(changenode=node)


cmdtable = {
    "children":
        (children,
         [('r', 'rev', '', _('show children of the specified rev')),
          ('', 'style', '', _('display using template map file')),
          ('', 'template', '', _('display with template'))],
         _('hg children [-r REV] [FILE]')),
}
