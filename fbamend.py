# fbamend.py - improved amend functionality
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""FBONLY: extends the existing commit amend functionality

Adds an hg amend command that amends the current parent commit with the
changes in the working copy.  Similiar to the existing hg commit --amend
except it doesn't prompt for the commit message unless --edit is provided.

Allows amending commits that have children and can automatically rebase
the children onto the new version of the commit

This extension is incompatible with changeset evolution. The command will
automatically disable itself if changeset evolution is enabled.
"""

from hgext import rebase
from mercurial import util, cmdutil, phases, commands, bookmarks, repair
from mercurial import merge, extensions
from mercurial.node import hex
from mercurial import obsolete
from mercurial.i18n import _
import errno, os, re

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

amendopts = [('', 'rebase', None, _('rebases children commits after the amend')),
    ('', 'fixup', None, _('rebase children commits from a previous amend')),
]

def uisetup(ui):
    if obsolete._enabled:
        msg = ('fbamend and evolve extension are imcompatible, '
               'fbamend deactivated.\n'
               'You can either disable it globally:\n'
               '- type `hg config --edit`\n'
               '- drop the `fbamend=` line from the `[extensions]` section\n'
               'or disable it for a specific repo:\n'
               '- type `hg config --local --edit`\n'
               '- add a `fbamend=!%s` line in the `[extensions]` section\n')
        msg %= ui.config('extensions', 'fbamend')
        ui.write_err(msg)
        return
    entry = extensions.wrapcommand(commands.table, 'commit', commit)
    for opt in amendopts:
        opt = (opt[0], opt[1], opt[2], "(with --amend) " + opt[3])
        entry[1].append(opt)
    # manual call of the decorator
    command('^amend', [
           ('e', 'edit', None, _('prompt to edit the commit message')),
       ] + amendopts + commands.walkopts + commands.commitopts,
       _('hg amend [OPTION]...'))(amend)


def commit(orig, ui, repo, *pats, **opts):
    if opts.get("amend"):
        # commit --amend default behavior is to prompt for edit
        opts['ignoremessage'] = True
        return amend(ui, repo, *pats, **opts)
    else:
        return orig(ui, repo, *pats, **opts)

def amend(ui, repo, *pats, **opts):
    '''amend the current commit with more changes
    '''
    rebase = opts.get('rebase')
    fixup = opts.get('fixup')

    if fixup:
        fixupamend(ui, repo)
        return

    old = repo['.']
    if old.phase() == phases.public:
        raise util.Abort(_('cannot amend public changesets'))
    if len(repo[None].parents()) > 1:
        raise util.Abort(_('cannot amend while merging'))

    haschildren = len(old.children()) > 0

    ignoremessage = opts.get('ignoremessage')
    if not ignoremessage:
        opts['message'] = old.description()

    tempnode = []
    def commitfunc(ui, repo, message, match, opts):
        e = cmdutil.commiteditor
        noderesult = repo.commit(message,
                           old.user(),
                           old.date(),
                           match,
                           editor=e,
                           extra={})

        # the temporary commit is the very first commit
        if not tempnode:
            tempnode.append(noderesult)

        return noderesult

    current = repo._bookmarkcurrent
    oldbookmarks = old.bookmarks()

    if haschildren:
        def fakestrip(orig, ui, repo, *args, **kwargs):
            if tempnode:
                if tempnode[0]:
                    # don't strip everything, just the temp node
                    # this is very hacky
                    orig(ui, repo, tempnode[0], backup='none')
                tempnode.pop()
            else:
                orig(ui, repo, *args, **kwargs)
        extensions.wrapfunction(repair, 'strip', fakestrip)

    node = cmdutil.amend(ui, repo, commitfunc, old, {}, pats, opts)

    if node == old.node():
        ui.status(_("nothing changed\n"))
        return 1

    if haschildren and not rebase:
        ui.status("warning: the commit's children were left behind " +
                  "(use hg amend --fixup to rebase them)\n")

    # move bookmarks
    newbookmarks = repo._bookmarks
    for bm in oldbookmarks:
        newbookmarks[bm] = node

    # create preamend bookmark
    if current:
        bookmarks.setcurrent(repo, current)
        if haschildren:
            newbookmarks[current + "(preamend)"] = old.node()
    else:
        # no active bookmark
        if haschildren:
            newbookmarks[hex(node)[:12] + "(preamend)"] = old.node()

    newbookmarks.write()

    if rebase and haschildren:
        fixupamend(ui, repo)

def fixupamend(ui, repo):
    """rebases any children found on the preamend commit and strips the
    preamend commit
    """
    current = repo['.']
    preamendname = None
    active = repo._bookmarkcurrent
    if active:
        preamendname = active + "(preamend)"

    if not preamendname:
        preamendname = hex(current.node())[:12] + "(preamend)"

    if not preamendname in repo._bookmarks:
        if active:
            raise util.Abort(_('no %s(preamend) bookmark' % active))
        else:
            raise util.Abort(_('no %s(preamend) bookmark - is your bookmark not active?' %
                               hex(current.node())[:12]))

    ui.status("rebasing the children of %s\n" % (preamendname))

    old = repo[preamendname]
    oldbookmarks = old.bookmarks()

    opts = {
        'rev' : [str(c.rev()) for c in old.descendants()],
        'dest' : active
    }

    if opts['rev'] and opts['rev'][0]:
        rebase.rebase(ui, repo, **opts)

    repair.strip(ui, repo, old.node(), topic='preamend-backup')

    for bookmark in oldbookmarks:
        repo._bookmarks.pop(bookmark)

    repo._bookmarks.write()

    merge.update(repo, current.node(), False, True, False)
    if active:
        bookmarks.setcurrent(repo, active)
