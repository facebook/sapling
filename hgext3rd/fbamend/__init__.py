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
    scmutil,
    util,
)
from mercurial.node import hex
from mercurial import lock as lockmod
from mercurial.i18n import _

from . import (
    common,
    movement,
    restack,
    revsets,
    split,
    unamend,
)

revsetpredicate = revsets.revsetpredicate

cmdtable = {}
command = registrar.command(cmdtable)

cmdtable.update(movement.cmdtable)
cmdtable.update(split.cmdtable)
cmdtable.update(unamend.cmdtable)

testedwith = 'ships-with-fb-hgext'

rebasemod = None
inhibitmod = None

amendopts = [
    ('', 'rebase', None, _('rebases children after the amend')),
    ('', 'fixup', None, _('rebase children from a previous amend')),
]

def uisetup(ui):
    global rebasemod
    try:
        rebasemod = extensions.find('rebase')
    except KeyError:
        ui.warn(_("no rebase extension detected - disabling fbamend"))
        return
    common.detectinhibit()

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
       ] + amendopts + commands.walkopts + commands.commitopts,
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

        global inhibitmod
        try:
            inhibitmod = extensions.find('inhibit')
        except KeyError:
            pass

        evolvemod = extensions.find('evolve')

        # Remove conflicted commands from evolve.
        table = evolvemod.cmdtable
        for name in ['prev', 'next', 'split']:
            todelete = [k for k in table if name in k]
            for k in todelete:
                oldentry = table[k]
                table['debugevolve%s' % name] = oldentry
                del table[k]

        # Wrap `hg fold`.
        foldentry = extensions.wrapcommand(
            evolvemod.cmdtable,
            'fold',
            wrapfold,
        )
        foldentry[1].append(
            ('', 'norebase', False, _("don't rebase children after fold"))
        )

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
    if obsolete.isenabled(repo, 'allnewcommands'):
        msg = ('fbamend and evolve extension are incompatible, '
               'fbamend deactivated.\n'
               'You can either disable it globally:\n'
               '- type `hg config --edit`\n'
               '- drop the `fbamend=` line from the `[extensions]` section\n'
               'or disable it for a specific repo:\n'
               '- type `hg config --local --edit`\n'
               '- add a `fbamend=!` line in the `[extensions]` section\n')
        ui.write_err(msg)
    rebase = opts.get('rebase')

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
    if fixup:
        fixupamend(ui, repo)
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
    commitdate = old.date() if not opts.get('date') else opts.get('date')
    def commitfunc(ui, repo, message, match, opts):
        e = cmdutil.commiteditor
        noderesult = repo.commit(message,
                           old.user(),
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
            return 0

        if haschildren and not rebase:
            msg = _("warning: the changeset's children were left behind\n")
            if _histediting(repo):
                ui.warn(msg)
                ui.status(_('(this is okay since a histedit is in progress)\n'))
            else:
                _usereducation(ui)
                ui.warn(msg)
                ui.status(_("(use 'hg rebase --restack' (alias: 'hg restack') "
                            "to rebase them)\n"))

        newbookmarks = repo._bookmarks

        # move old bookmarks to new node
        for bm in oldbookmarks:
            newbookmarks[bm] = node

        userestack = ui.configbool('fbamend', 'userestack')
        if not _histediting(repo) and not userestack:
            preamendname = _preamendname(repo, node)
            if haschildren:
                newbookmarks[preamendname] = old.node()
            elif not active:
                # update bookmark if it isn't based on the active bookmark name
                oldname = _preamendname(repo, old.node())
                if oldname in repo._bookmarks:
                    newbookmarks[preamendname] = repo._bookmarks[oldname]
                    del newbookmarks[oldname]

        tr = repo.transaction('fixupamend')
        newbookmarks.recordchange(tr)
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
            # There's a subtly to rebase transaction close where the rebasestate
            # file will be written to disk, even if it had already been unlinked
            # by the rebase logic (because the file generator was already on the
            # transaction). Until we fix it in core, let's manually unlink the
            # rebasestate so the rebase isn't left pending.
            util.unlinkpath(repo.vfs.join("rebasestate"), ignoremissing=True)
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

        for bookmark in oldbookmarks:
            repo._bookmarks.pop(bookmark)

        tr = repo.transaction('fixupamend')
        repo._bookmarks.recordchange(tr)

        if obsolete.isenabled(repo, obsolete.createmarkersopt):
            # clean up the original node if inhibit kept it alive
            if not old.obsolete():
                obsolete.createmarkers(repo, [(old,())])
            tr.close()
        else:
            tr.close()
            repair.strip(ui, repo, old.node(), topic='preamend-backup')

        merge.update(repo, current.node(), False, True, False)
        if active:
            bmactivate(repo, active)
    finally:
        lockmod.release(wlock, lock, tr)

def wrapfold(orig, ui, repo, *args, **opts):
    """Automatically rebase unstable descendants after fold."""
    # Find the rev numbers of the changesets that will be folded. This needs
    # to happen before folding in case the input revset is relative to the
    # working copy parent, since `hg fold` may update to a new changeset.
    revs = list(args) + opts.get('rev', [])
    revs = scmutil.revrange(repo, revs)
    if not opts['exact']:
        revs = repo.revs('(%ld::.) or (.::%ld)', revs, revs)
    torebase = repo.revs('descendants(%ld) - (%ld)', revs, revs)

    # Perform fold.
    ret = orig(ui, repo, *args, **opts)

    # Return early if fold failed.
    if ret:
        return ret

    # Fix up stack.
    with repo.wlock():
        with repo.lock():
            with repo.transaction('foldrebase'):
                if not opts['norebase'] and torebase:
                    folded = repo.revs('allsuccessors(%ld)', revs).last()
                    common.restackonce(ui, repo, folded)
                else:
                    # If there's nothing to rebase, deinhibit the folded
                    # changesets so that they get correctly marked as
                    # hidden if needed. For some reason inhibit's
                    # post-transaction hook misses this changeset.
                    visible = repo.unfiltered().revs('(%ld) - hidden()', revs)
                    common.deinhibit(repo, (repo[r] for r in visible))
            # The rebasestate file is incorrectly left behind, so cleanup.
            # See the earlier comment on util.unlinkpath for more details.
            util.unlinkpath(repo.vfs.join("rebasestate"), ignoremissing=True)

    # Fix up bookmarks, if any.
    _fixbookmarks(repo, revs)

    return ret

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

    # We need to create a transaction to ensure that the inhibit extension's
    # post-transaction hook is called after the rebase is finished. This hook
    # is responsible for inhibiting visible obsolete (suspended) changesets,
    # which may be created if the rebased commits have descendants that were
    # not rebased. To be less invasive, create a short transaction after the
    # rebase call instead of wrapping the call itself in a transaction.
    with repo.wlock():
        with repo.lock():
            ret = orig(ui, repo, **opts)
            with repo.transaction('rebase'):
                # The rebase command will cause the rebased commits to still be
                # cached as 'visible', even if the entire stack has been
                # rebased and everything is obsolete. We need to manaully clear
                # the cached values to that the post-transaction callback will
                # work correctly.
                repo.invalidatevolatilesets()
            return ret

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
    with repo.wlock():
        with repo.lock():
            with repo.transaction('movebookmarks') as tr:
                for rev in revs:
                    latest = cl.node(common.latest(repo, rev))
                    for bm in repo.nodebookmarks(cl.node(rev)):
                        repo._bookmarks[bm] = latest
                repo._bookmarks.recordchange(tr)

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
