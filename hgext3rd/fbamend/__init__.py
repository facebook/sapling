# fbamend.py - improved amend functionality
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""extends the existing commit amend functionality

Adds an hg amend command that amends the current parent changeset with the
changes in the working copy.  Similiar to the existing hg commit --amend
except it doesn't prompt for the commit message unless --edit is provided.

Allows amending changesets that have children and can automatically rebase
the children onto the new version of the changeset.

This extension is incompatible with changeset evolution. The command will
automatically disable itself if changeset evolution is enabled.

To disable the creation of preamend bookmarks and use obsolescence
markers instead to fix up amends, enable the userestack option::

    [fbamend]
    userestack = true

To make `hg previous` and `hg next` always pick the newest commit at
each step of walking up or down the stack instead of aborting when
encountering non-linearity (equivalent to the --newest flag), enable
the following config option::

    [fbamend]
    alwaysnewest = true

To automatically update the commit date, enable the following config option::

    [fbamend]
    date = implicitupdate

Note that if --date is specified on the command line, it takes precedence.

"""

from __future__ import absolute_import

from mercurial import (
    bookmarks,
    cmdutil,
    commands,
    error,
    extensions,
    merge,
    obsolete,
    phases,
    registrar,
    repair,
)
from mercurial.node import hex
from mercurial import lock as lockmod
from mercurial.i18n import _

from hgext import (
    histedit,
    rebase as rebasemod,
)

from . import (
    common,
    fold,
    hiddenoverride,
    hide,
    metaedit,
    movement,
    prune,
    restack,
    revsets,
    split,
    unamend,
)

import tempfile

revsetpredicate = revsets.revsetpredicate

cmdtable = {}
command = registrar.command(cmdtable)

cmdtable.update(fold.cmdtable)
cmdtable.update(hide.cmdtable)
cmdtable.update(metaedit.cmdtable)
cmdtable.update(movement.cmdtable)
cmdtable.update(prune.cmdtable)
cmdtable.update(split.cmdtable)
cmdtable.update(unamend.cmdtable)

testedwith = 'ships-with-fb-hgext'

amendopts = [
    ('', 'rebase', None, _('rebases children after the amend')),
    ('', 'fixup', None, _('rebase children from a previous amend')),
    ('', 'to', '', _('amend to a specific commit in the current stack')),
]

def uisetup(ui):
    hiddenoverride.uisetup(ui)
    prune.uisetup(ui)
    entry = extensions.wrapcommand(commands.table, 'commit', commit)
    for opt in amendopts:
        opt = (opt[0], opt[1], opt[2], "(with --amend) " + opt[3])
        entry[1].append(opt)

    # manual call of the decorator
    command('^amend', [
            ('A', 'addremove', None,
             _('mark new/missing files as added/removed before committing')),
           ('e', 'edit', None, _('prompt to edit the commit message')),
           ('i', 'interactive', None, _('use interactive mode')),
       ] + amendopts + commands.walkopts + commands.commitopts
        + commands.commitopts2,
       _('hg amend [OPTION]...'))(amend)

    def has_automv(loaded):
        if not loaded:
            return
        automv = extensions.find('automv')
        entry = extensions.wrapcommand(cmdtable, 'amend', automv.mvcheck)
        entry[1].append(
            ('', 'no-move-detection', None,
             _('disable automatic file move detection')))
    extensions.afterloaded('automv', has_automv)

    def evolveloaded(loaded):
        if not loaded:
            return

        evolvemod = extensions.find('evolve')

        # Remove conflicted commands from evolve.
        table = evolvemod.cmdtable
        for name in ['prev', 'next', 'split', 'fold', 'metaedit', 'prune']:
            todelete = [k for k in table if name in k]
            for k in todelete:
                oldentry = table[k]
                table['debugevolve%s' % name] = oldentry
                del table[k]

    extensions.afterloaded('evolve', evolveloaded)

    def rebaseloaded(loaded):
        if not loaded:
            return
        entry = extensions.wrapcommand(rebasemod.cmdtable, 'rebase',
                                       wraprebase)
        entry[1].append((
            '', 'restack', False, _('rebase all changesets in the current '
                                    'stack onto the latest version of their '
                                    'respective parents')
        ))
    extensions.afterloaded('rebase', rebaseloaded)

def commit(orig, ui, repo, *pats, **opts):
    if opts.get("amend"):
        # commit --amend default behavior is to prompt for edit
        opts['noeditmessage'] = True
        return amend(ui, repo, *pats, **opts)
    else:
        badflags = [flag for flag in
                ['rebase', 'fixup'] if opts.get(flag, None)]
        if badflags:
            raise error.Abort(_('--%s must be called with --amend') %
                    badflags[0])

        return orig(ui, repo, *pats, **opts)

def amend(ui, repo, *pats, **opts):
    '''amend the current changeset with more changes
    '''
    rebase = opts.get('rebase')
    to = opts.get('to')

    if rebase and _histediting(repo):
        # if a histedit is in flight, it's dangerous to remove old commits
        hint = _('during histedit, use amend without --rebase')
        raise error.Abort('histedit in progress', hint=hint)

    badflags = [flag for flag in
            ['rebase', 'fixup'] if opts.get(flag, None)]
    if opts.get('interactive') and badflags:
        raise error.Abort(_('--interactive and --%s are mutually exclusive') %
                badflags[0])

    fixup = opts.get('fixup')

    badtoflags = [
        'rebase', 'fixup', 'addremove', 'edit', 'interactive', 'include',
        'exclude', 'message', 'logfile', 'date', 'user',
        'no-move-detection', 'stack'
    ]

    if to and any(opts.get(flag, None) for flag in badtoflags):
        raise error.Abort(_('--to cannot be used with any other options'))

    if fixup:
        fixupamend(ui, repo)
        return

    if to:
        amendtocommit(ui, repo, to)
        return

    old = repo['.']
    if old.phase() == phases.public:
        raise error.Abort(_('cannot amend public changesets'))
    if len(repo[None].parents()) > 1:
        raise error.Abort(_('cannot amend while merging'))

    haschildren = len(old.children()) > 0

    opts['message'] = cmdutil.logmessage(ui, opts)
    # Avoid further processing of any logfile. If such a file existed, its
    # contents have been copied into opts['message'] by logmessage
    opts['logfile'] = ''

    if not opts.get('noeditmessage') and not opts.get('message'):
        opts['message'] = old.description()

    tempnode = []
    commitdate = opts.get('date')
    if not commitdate:
        if ui.config('fbamend', 'date') == 'implicitupdate':
            commitdate = 'now'
        else:
            commitdate = old.date()
    commituser = old.user() if not opts.get('user') else opts.get('user')
    def commitfunc(ui, repo, message, match, opts):
        e = cmdutil.commiteditor
        noderesult = repo.commit(message,
                           commituser,
                           commitdate,
                           match,
                           editor=e,
                           extra={})

        # the temporary commit is the very first commit
        if not tempnode:
            tempnode.append(noderesult)

        return noderesult

    active = bmactive(repo)
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

    tr = None
    wlock = None
    lock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()

        if opts.get('interactive'):
            # Strip the interactive flag to avoid infinite recursive loop
            opts.pop('interactive')
            cmdutil.dorecord(ui, repo, amend, None, False,
                    cmdutil.recordfilter, *pats, **opts)
            return

        else:
            node = cmdutil.amend(ui, repo, commitfunc, old, {}, pats, opts)

        if node == old.node():
            ui.status(_("nothing changed\n"))
            return 1

        if haschildren and not rebase:
            msg = _("warning: the changeset's children were left behind\n")
            if _histediting(repo):
                ui.warn(msg)
                ui.status(_('(this is okay since a histedit is in progress)\n'))
            else:
                _usereducation(ui)
                ui.warn(msg)
                ui.status(_("(use 'hg restack' to rebase them)\n"))

        changes = []
        # move old bookmarks to new node
        for bm in oldbookmarks:
            changes.append((bm, node))

        userestack = ui.configbool('fbamend', 'userestack')
        if not _histediting(repo) and not userestack:
            preamendname = _preamendname(repo, node)
            if haschildren:
                changes.append((preamendname, old.node()))
            elif not active:
                # update bookmark if it isn't based on the active bookmark name
                oldname = _preamendname(repo, old.node())
                if oldname in repo._bookmarks:
                    changes.append((preamendname, repo._bookmarks[oldname]))
                    changes.append((oldname, None)) # delete the old name

        tr = repo.transaction('fixupamend')
        repo._bookmarks.applychanges(repo, tr, changes)
        tr.close()

        if rebase and haschildren:
            fixupamend(ui, repo)
    finally:
        lockmod.release(wlock, lock, tr)

def fixupamend(ui, repo):
    """rebases any children found on the preamend changset and strips the
    preamend changset
    """
    wlock = None
    lock = None
    tr = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()
        current = repo['.']

        # Use obsolescence information to fix up the amend instead of relying
        # on the preamend bookmark if the user enables this feature.
        if ui.configbool('fbamend', 'userestack'):
            with repo.transaction('fixupamend') as tr:
                try:
                    common.restackonce(ui, repo, current.rev())
                except error.InterventionRequired:
                    tr.close()
                    raise
            return

        preamendname = _preamendname(repo, current.node())
        if not preamendname in repo._bookmarks:
            raise error.Abort(_('no bookmark %s') % preamendname,
                             hint=_('check if your bookmark is active'))

        old = repo[preamendname]
        if old == current:
            hint = _('please examine smartlog and rebase your changsets '
                     'manually')
            err = _('cannot automatically determine what to rebase '
                    'because bookmark "%s" points to the current changset') % \
                   preamendname
            raise error.Abort(err, hint=hint)
        oldbookmarks = old.bookmarks()

        ui.status(_("rebasing the children of %s\n") % (preamendname))

        active = bmactive(repo)
        opts = {
            'rev' : [str(c.rev()) for c in old.descendants()],
            'dest' : current.rev()
        }

        if opts['rev'] and opts['rev'][0]:
            rebasemod.rebase(ui, repo, **opts)

        changes = []
        for bookmark in oldbookmarks:
            changes.append((bookmark, None)) # delete the bookmark
        tr = repo.transaction('fixupamend')
        repo._bookmarks.applychanges(repo, tr, changes)

        if obsolete.isenabled(repo, obsolete.createmarkersopt):
            tr.close()
        else:
            tr.close()
            repair.strip(ui, repo, old.node(), topic='preamend-backup')

        merge.update(repo, current.node(), False, True, False)
        if active:
            bmactivate(repo, active)
    finally:
        lockmod.release(wlock, lock, tr)

def amendtocommit(ui, repo, commit):
    """amend to a specific commit
    """
    with repo.wlock(), repo.lock():
        originalcommits = list(repo.set("::. - public()"))
        tempcommit = repo.commit(text="tempCommit")

        if not tempcommit:
            error.Abort(_('no pending changes to amend'))

        tempcommithex = hex(tempcommit)

        fp = tempfile.NamedTemporaryFile()
        try:
            found = False
            for line in originalcommits:
                if str(line) == commit:
                    fp.write("pick " + str(line) + "\n")
                    fp.write("roll " + tempcommithex[:12] + "\n")
                    found = True
                else:
                    fp.write("pick " + str(line) + "\n")
            if not found:
                error.Abort(_('the commit hash provided cannot be found'))
            fp.flush()
            try:
                histedit.histedit(ui, repo, commands=fp.name)
            except error.InterventionRequired:
                ui.warn(_('amend --to encountered an issue - '
                        'use hg histedit to continue or abort'))
                raise
        finally:
            fp.close()

def wraprebase(orig, ui, repo, **opts):
    """Wrapper around `hg rebase` adding the `--restack` option, which rebases
       all "unstable" descendants of an obsolete changeset onto the latest
       version of that changeset. This is similar to (and intended as a
       replacement for) the `hg evolve --all` command.
    """
    if opts['restack']:
        # We can't abort if --dest is passed because some extensions
        # (namely remotenames) will automatically add this flag.
        # So just silently drop it instead.
        opts.pop('dest', None)

        if opts['rev']:
            raise error.Abort(_("cannot use both --rev and --restack"))
        if opts['source']:
            raise error.Abort(_("cannot use both --source and --restack"))
        if opts['base']:
            raise error.Abort(_("cannot use both --base and --restack"))
        if opts['abort']:
            raise error.Abort(_("cannot use both --abort and --restack"))
        if opts['continue']:
            raise error.Abort(_("cannot use both --continue and --restack"))

        # The --hidden option is handled at a higher level, so instead of
        # checking for it directly we have to check whether the repo
        # is unfiltered.
        if repo == repo.unfiltered():
            raise error.Abort(_("cannot use both --hidden and --restack"))

        return restack.restack(ui, repo, opts)

    return orig(ui, repo, **opts)

def _preamendname(repo, node):
    suffix = '.preamend'
    name = bmactive(repo)
    if not name:
        name = hex(node)[:12]
    return name + suffix

def _histediting(repo):
    return repo.vfs.exists('histedit-state')

def _usereducation(ui):
    """
    You can print out a message to the user here
    """
    education = ui.config('fbamend', 'education')
    if education:
        ui.warn(education + "\n")

def _fixbookmarks(repo, revs):
    """Make any bookmarks pointing to the given revisions point to the
       latest version of each respective revision.
    """
    repo = repo.unfiltered()
    cl = repo.changelog
    with repo.wlock(), repo.lock(), repo.transaction('movebookmarks') as tr:
        changes = []
        for rev in revs:
            latest = cl.node(common.latest(repo, rev))
            for bm in repo.nodebookmarks(cl.node(rev)):
                changes.append((bm, latest))
        repo._bookmarks.applychanges(repo, tr, changes)

### bookmarks api compatibility layer ###
def bmactivate(repo, mark):
    try:
        return bookmarks.activate(repo, mark)
    except AttributeError:
        return bookmarks.setcurrent(repo, mark)

def bmactive(repo):
    try:
        return repo._activebookmark
    except AttributeError:
        return repo._bookmarkcurrent
