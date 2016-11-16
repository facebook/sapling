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
encountering non-linearity (equivalent to the --newest flag), enabled
the following config option::

    [fbamend]
    alwaysnewest = true

"""

from mercurial import (
    bookmarks,
    cmdutil,
    commands,
    error,
    extensions,
    merge,
    obsolete,
    phases,
    repair,
)
from mercurial.node import hex, nullrev, short
from mercurial import lock as lockmod
from mercurial.i18n import _
from collections import defaultdict, deque
from contextlib import nested
from itertools import count

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

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

    command('^unamend', [])(unamend)

    def has_automv(loaded):
        if not loaded:
            return
        automv = extensions.find('automv')
        entry = extensions.wrapcommand(cmdtable, 'amend', automv.mvcheck)
        entry[1].append(
            ('', 'no-move-detection', None,
             _('disable automatic file move detection')))
    extensions.afterloaded('automv', has_automv)

    # If the evolve extension is enabled, wrap the `next` command to
    # add the --rebase flag.
    def evolveloaded(loaded):
        if not loaded:
            return

        global inhibitmod
        try:
            inhibitmod = extensions.find('inhibit')
        except KeyError:
            pass

        evolvemod = extensions.find('evolve')

        # Wrap `hg previous`.
        preventry = extensions.wrapcommand(
            evolvemod.cmdtable,
            'previous',
            wrapprevious,
            synopsis=" [NUM_STEPS]"
        )
        _hideopts(preventry, set(['no-topic', 'dry-run']))
        preventry[1].extend([
            ('', 'newest', False,
                _('always pick the newest parent when a changeset has '
                  'multiple parents')
            ),
            ('', 'bottom', False,
                _('update to the lowest non-public ancestor of the '
                  'current changeset')
            ),
            ('', 'bookmark', False,
                _('update to the first ancestor with a bookmark')
            ),
            ('', 'no-activate-bookmark', False,
                _('do not activate the bookmark on the destination changeset')
            ),
        ])

        # Wrap `hg next`.
        nextentry = extensions.wrapcommand(
            evolvemod.cmdtable,
            'next',
            wrapnext,
            synopsis=" [NUM_STEPS]",
        )
        _hideopts(nextentry, set(['evolve', 'no-topic', 'dry-run']))
        nextentry[1].extend([
            ('', 'newest', False,
                _('always pick the newest child when a changeset has '
                  'multiple children')
            ),
            ('', 'rebase', False,
                _('rebase each changeset if necessary')
            ),
            ('', 'top', False,
                _('update to the head of the current stack')
            ),
            ('', 'bookmark', False,
                _('update to the first changeset with a bookmark')
            ),
            ('', 'no-activate-bookmark', False,
                _('do not activate the bookmark on the destination changeset')
            ),
        ])
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

def unamend(ui, repo, **opts):
    """undo the amend operation on a current changeset

    This command will roll back to the previous version of a changeset,
    leaving working directory in state in which it was before running
    `hg amend` (e.g. files modified as part of an amend will be
    marked as modified `hg status`)"""
    try:
        inhibitmod = extensions.find('inhibit')
    except KeyError:
        hint = _("please add inhibit to the list of enabled extensions")
        e = _("unamend requires inhibit extension to be enabled")
        raise error.Abort(e, hint=hint)

    unfi = repo.unfiltered()

    # identify the commit from which to unamend
    curctx = repo['.']

    # identify the commit to which to unamend
    markers = list(obsolete.precursormarkers(curctx))
    if len(markers) != 1:
        e = _("changeset must have one precursor, found %i precursors")
        raise error.Abort(e % len(markers))

    precnode = markers[0].precnode()
    precctx = unfi[precnode]

    if curctx.children():
        raise error.Abort(_("cannot unamend in the middle of a stack"))

    with nested(repo.wlock(), repo.lock()):
        repobookmarks = repo._bookmarks
        ctxbookmarks = curctx.bookmarks()
        # we want to inhibit markers that mark precnode obsolete
        inhibitmod._inhibitmarkers(unfi, [precnode])
        changedfiles = []
        wctx = repo[None]
        wm = wctx.manifest()
        cm = precctx.manifest()
        dirstate = repo.dirstate
        diff = cm.diff(wm)
        changedfiles.extend(diff.iterkeys())

        tr = repo.transaction('unamend')
        dirstate.beginparentchange()
        dirstate.rebuild(precnode, cm, changedfiles)
        # we want added and removed files to be shown
        # properly, not with ? and ! prefixes
        for filename, data in diff.iteritems():
            if data[0][0] is None:
                dirstate.add(filename)
            if data[1][0] is None:
                dirstate.remove(filename)
        dirstate.endparentchange()
        for book in ctxbookmarks:
            repobookmarks[book] = precnode
        repobookmarks.recordchange(tr)
        tr.close()
        # we want to mark the changeset from which we were unamending
        # as obsolete
        obsolete.createmarkers(repo, [(curctx, ())])

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
               '- add a `fbamend=!%s` line in the `[extensions]` section\n')
        msg %= ui.config('extensions', 'fbamend')
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
                ui.status(_("(use 'hg amend --fixup' to rebase them)\n"))

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
                    _restackonce(ui, repo, current.rev())
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

def wrapprevious(orig, ui, repo, *args, **opts):
    """Replacement for `hg previous` from the evolve extension."""
    _moverelative(ui, repo, args, opts, reverse=True)

def wrapnext(orig, ui, repo, *args, **opts):
    """Replacement for `hg next` from the evolve extension."""
    _moverelative(ui, repo, args, opts, reverse=False)

def _moverelative(ui, repo, args, opts, reverse=False):
    """Update to a changeset relative to the current changeset.
       Implements both `hg previous` and `hg next`.

       Takes in a list of positional arguments and a dict of command line
       options. (See help for `hg previous` and `hg next` to see which
       arguments and flags are supported.)

       Moves forward through history by default -- the behavior of `hg next`.
       Setting reverse=True will change the behavior to that of `hg previous`.
    """
    # Parse positional argument.
    try:
        n = int(args[0]) if args else 1
    except ValueError:
        raise error.Abort(_("argument must be an integer"))
    if n <= 0:
        return

    if ui.configbool('fbamend', 'alwaysnewest'):
        opts['newest'] = True

    # Check that the given combination of arguments is valid.
    if args:
        if opts.get('bookmark', False):
            raise error.Abort(_("cannot use both number and --bookmark"))
        if opts.get('top', False):
            raise error.Abort(_("cannot use both number and --top"))
        if opts.get('bottom', False):
            raise error.Abort(_("cannot use both number and --bottom"))
    if opts.get('bookmark', False):
        if opts.get('top', False):
            raise error.Abort(_("cannot use both --top and --bookmark"))
        if opts.get('bottom', False):
            raise error.Abort(_("cannot use both --bottom and --bookmark"))

    # Check if there is an outstanding operation or uncommited changes.
    cmdutil.checkunfinished(repo)
    if not opts.get('merge', False):
        try:
            cmdutil.bailifchanged(repo)
        except error.Abort as e:
            e.hint = _("use --merge to bring along uncommitted changes")
            raise
    elif opts.get('rebase', False):
        raise error.Abort(_("cannot use both --merge and --rebase"))

    with nested(repo.wlock(), repo.lock()):
        # Record the active bookmark, if any.
        bookmark = bmactive(repo)
        noactivate = opts.get('no_activate_bookmark', False)
        movebookmark = opts.get('move_bookmark', False)

        with repo.transaction('moverelative') as tr:
            # Find the desired changeset. May potentially perform rebase.
            try:
                target = _findtarget(ui, repo, n, opts, reverse)
            except error.InterventionRequired:
                # Rebase failed. Need to manually close transaction to allow
                # `hg rebase --continue` to work correctly.
                tr.close()
                raise

            # Move the active bookmark if neccesary. Needs to happen before
            # we update to avoid getting a 'leaving bookmark X' message.
            if movebookmark and bookmark is not None:
                _setbookmark(repo, tr, bookmark, target)

            # Update to the target changeset.
            commands.update(ui, repo, rev=target)

            # Print out the changeset we landed on.
            _showchangesets(ui, repo, revs=[target])

            # Activate the bookmark on the new changeset.
            if not noactivate and not movebookmark:
                _activate(ui, repo, target)

            # Clear cached 'visible' set so that the post-transaction hook
            # set by the inhibit extension will see a correct view of
            # the repository. The cached contents of the visible set are
            # after a rebase operation show the old stack as visible,
            # which will cause the inhibit extension to always inhibit
            # the stack even if it is entirely obsolete and hidden.
            repo.invalidatevolatilesets()

def _findtarget(ui, repo, n, opts, reverse):
    """Find the appropriate target changeset for `hg previous` and
       `hg next` based on the provided options. May rebase the traversed
       changesets if the rebase option is given in the opts dict.
    """
    newest = opts.get('newest', False)
    bookmark = opts.get('bookmark', False)
    rebase = opts.get('rebase', False)
    top = opts.get('top', False)
    bottom = opts.get('bottom', False)

    if top and not rebase:
        # If we're not rebasing, jump directly to the top instead of
        # walking up the stack.
        return _findstacktop(ui, repo, newest)
    elif bottom:
        return _findstackbottom(ui, repo)
    elif reverse:
        return _findprevtarget(ui, repo, n, bookmark, newest)
    else:
        return _findnexttarget(ui, repo, n, bookmark, newest, rebase, top)

def _findprevtarget(ui, repo, n=None, bookmark=False, newest=False):
    """Get the revision n levels down the stack from the current revision.
       If newest is True, if a changeset has multiple parents the newest
       will always be chosen. Otherwise, throws an exception.
    """
    ctx = repo['.']

    # The caller must specify a stopping condition -- either a number
    # of steps to walk or a bookmark to search for.
    if not n and not bookmark:
        raise error.Abort(_("no stop condition specified"))

    for i in count(0):
        # Loop until we're gone the desired number of steps, or we reach a
        # node with a bookmark if the bookmark option was specified.
        if bookmark:
            if i > 0 and ctx.bookmarks():
                break
        elif i >= n:
            break

        parents = ctx.parents()

        # Is this the root of the current branch?
        if not parents or parents[0].rev() == nullrev:
            if ctx.rev() == repo['.'].rev():
                raise error.Abort(_("current changeset has no parents"))
            ui.status(_('reached root changeset\n'))
            break

        # Are there multiple parents?
        if len(parents) > 1 and not newest:
            ui.status(_("changeset %s has multiple parents, namely:\n")
                      % short(ctx.node()))
            _showchangesets(ui, repo, contexts=parents)
            raise error.Abort(_("ambiguous previous changeset"),
                              hint=_("use the --newest flag to always "
                                     "pick the newest parent at each step"))

        # Get the parent with the highest revision number.
        ctx = max(parents, key=lambda x: x.rev())

    return ctx.rev()

def _findnexttarget(ui, repo, n=None, bookmark=False, newest=False,
                    rebase=False, top=False):
    """Get the revision n levels up the stack from the current revision.
       If newest is True, if a changeset has multiple children the newest
       will always be chosen. Otherwise, throws an exception. If the rebase
       option is specified, potentially rebase unstable children as we
       walk up the stack.
    """
    rev = repo['.'].rev()

    # The caller must specify a stopping condition -- either a number
    # of steps to walk, a bookmark to search for, or --top.
    if not n and not bookmark and not top:
        raise error.Abort(_("no stop condition specified"))

    # Precompute child relationships to avoid expensive ctx.children() calls.
    if not rebase:
        childrenof = _getchildrelationships(repo, [rev])

    for i in count(0):
        # Loop until we're gone the desired number of steps, or we reach a
        # node with a bookmark if the bookmark option was specified.
        # If top is specified, loop until we reach a head.
        if bookmark:
            if i > 0 and repo[rev].bookmarks():
                break
        elif i >= n and not top:
            break

        # If the rebase flag is present, rebase any unstable children.
        # This means we can't rely on precomputed child relationships.
        if rebase:
            _restackonce(ui, repo, rev, childrenonly=True)
            children = [c.rev() for c in repo[rev].children()]
        else:
            children = childrenof[rev]

        # Have we reached a head?
        if not children:
            if rev == repo['.'].rev():
                raise error.Abort(_("current changeset has no children"))
            if not top:
                ui.status(_('reached head changeset\n'))
            break

        # Are there multiple children?
        if len(children) > 1 and not newest:
            ui.status(_("changeset %s has multiple children, namely:\n")
                      % short(repo[rev].node()))
            _showchangesets(ui, repo, revs=children)
            raise error.Abort(_("ambiguous next changeset"),
                              hint=_("use the --newest flag to always "
                                     "pick the newest child at each step"))

        # Get the child with the highest revision number.
        rev = max(children)

    return rev

def _findstacktop(ui, repo, newest=False):
    """Find the head of the current stack."""
    heads = repo.revs('heads(.::)')
    if len(heads) > 1:
        if newest:
            # We can't simply return heads.max() since this might give
            # a different answer from walking up the stack as in
            # _findnexttarget(), which picks the child with the greatest
            # revision number at each step. This may be confusing, since it
            # means that `hg next --top` and `hg next --top --rebase` would
            # result in a different destination changeset, for example.
            return _findnexttarget(ui, repo, newest=True, top=True)
        ui.warn(_("current stack has multiple heads, namely:\n"))
        _showchangesets(ui, repo, revs=heads)
        raise error.Abort(_("ambiguous next changeset"),
                          hint=_("use the --newest flag to always "
                                 "pick the newest child at each step"))
    return heads.first()

def _findstackbottom(ui, repo):
    """Find the lowest non-public ancestor of the current changeset."""
    if repo['.'].phase() == phases.public:
        raise error.Abort(_("current changeset is public"))
    return repo.revs("::. & draft()").first()

def wraprebase(orig, ui, repo, **opts):
    """Wrapper around `hg rebase` adding the `--restack` option, which rebases
       all "unstable" descendants of an obsolete changeset onto the latest
       version of that changeset. This is similar to (and intended as a
       replacement for) the `hg evolve --all` command.
    """
    if opts['restack']:
        if opts['rev']:
            raise error.Abort(_("cannot use both --rev and --restack"))
        if opts['dest']:
            raise error.Abort(_("cannot use both --dest and --restack"))
        if opts['source']:
            raise error.Abort(_("cannot use both --source and --restack"))
        if opts['base']:
            raise error.Abort(_("cannot use both --base and --restack"))
        if opts['abort']:
            raise error.Abort(_("cannot use both --abort and --restack"))
        if opts['continue']:
            raise error.Abort(_("cannot use both --continue and --restack"))
        return restack(ui, repo, opts)

    # If the --continue flag is passed, we need to create a transaction
    # to ensure that the inhibit extension's post-transaction hook is called
    # after the rebase is finished. This hook is responsible for inhibiting
    # visible obsolete changesets, which may be created if we're continuing a
    # `hg next --rebase` operation. To be less invasive, create a short
    # transaction after the rebase call instead of wrapping the call itself
    # in a transaction.
    if opts['continue']:
        with nested(repo.wlock(), repo.lock()):
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

def restack(ui, repo, rebaseopts=None):
    """Repair a situation in which one or more changesets in a stack
       have been obsoleted (thereby leaving their descendants in the stack
       unstable) by finding any such changesets and rebasing their descendants
       onto the latest version of each respective changeset.
    """
    if rebaseopts is None:
        rebaseopts = {}

    with nested(repo.wlock(), repo.lock()):
        cmdutil.checkunfinished(repo)
        cmdutil.bailifchanged(repo)

        # Find the latest version of the changeset at the botom of the
        # current stack. If the current changeset is public, simply start
        # restacking from the current changeset (under the assumption)
        # that there are non-public changesets higher up.
        base = repo.revs('::. & draft()').first()
        latest = _latest(repo, base) if base is not None else repo['.'].rev()
        targets = _findrestacktargets(repo, latest)

        with repo.transaction('restack') as tr:
            # Attempt to stabilize all changesets that are or will be (after
            # rebasing) descendants of base.
            for rev in targets:
                try:
                    _restackonce(ui, repo, rev, rebaseopts)
                except error.InterventionRequired:
                    tr.close()
                    raise

            # Ensure that we always end up on the latest version of the
            # current changeset. Usually, this will be taken care of
            # by the rebase operation. However, in some cases (such as
            # if we are on the precursor of the base changeset) the
            # rebase will not update to the latest version, so we need
            # to do this manually.
            successor = repo.revs('allsuccessors(.)').last()
            if successor is not None:
                commands.update(ui, repo, rev=successor)

            # Clear cached 'visible' set so that the post-transaction
            # hook in the inhibit extension will see a correct view of
            # the repository. The cached contents of the visible set are
            # after a rebase operation show the old stack as visible,
            # which will cause the inhibit extension to always inhibit
            # the stack even if it is entirely obsolete.
            repo.invalidatevolatilesets()

def _restackonce(ui, repo, rev, rebaseopts=None, childrenonly=False):
    """Rebase all descendants of precursors of rev onto rev, thereby
       stabilzing any non-obsolete descendants of those precursors.
       Takes in an optional dict of options for the rebase command.
       If childrenonly is True, only rebases direct children of precursors
       of rev rather than all descendants of those precursors.
    """
    # Get visible descendants of precusors of rev.
    allprecursors = repo.revs('allprecursors(%d)', rev)
    fmt = '%s(%%ld) - %%ld' % ('children' if childrenonly else 'descendants')
    descendants = repo.revs(fmt, allprecursors, allprecursors)

    # Nothing to do if there are no descendants.
    if not descendants:
        return

    # Overwrite source and destination, leave all other options.
    if rebaseopts is None:
        rebaseopts = {}
    rebaseopts['rev'] = descendants
    rebaseopts['dest'] = rev

    # We're potentially going to make a few temporary configuration
    # changes, so back up the old configs to restore afterwards.
    backupconfigs = []

    # If we're only rebasing children, we need to set the configuration
    # to allow instability. We can't use the --keep flag as this will
    # suppress the creation of obsmarkers on the precursor nodes,
    # and it is difficult to manually create the correct markers on the
    # new changesets after a rebase operation if several changesets
    # were rebased.
    if childrenonly:
        backupconfigs.append(ui.backupconfig('experimental', 'evolution'))
        oldconfig = repo.ui.configlist('experimental', 'evolution')
        newconfig = oldconfig + [obsolete.allowunstableopt]
        repo.ui.setconfig('experimental', 'evolution', newconfig)

    # Overwrite the the global configuration value set by the tweakdefaults
    # extension to store the current top-level operation name. tweakdefaults
    # wraps obsolete.createmarkers() to use this value to set the metadata
    # on newly created obsmarkers. We need this to be set to 'rebase' in
    # order for obsolete changesets to have a "rebased as X" labels
    # in `hg sl`.
    try:
        tweakdefaults = extensions.find('tweakdefaults')
    except KeyError:
        # No tweakdefaults extension -- skip this since there is no wrapper
        # to set the metadata.
        pass
    else:
        backupconfigs.append(ui.backupconfig(
            tweakdefaults.globaldata,
            tweakdefaults.createmarkersoperation
        ))
        repo.ui.setconfig(
            tweakdefaults.globaldata,
            tweakdefaults.createmarkersoperation,
            'rebase'
        )

    try:
        rebasemod.rebase(ui, repo, **rebaseopts)
    finally:
        # Reset the configuration to what it was before.
        for backup in backupconfigs:
            ui.restoreconfig(backup)

    # Remove any preamend bookmarks on precursors.
    _clearpreamend(repo, allprecursors)

    # Deinhibit the precursors so that they will be correctly shown as
    # obsolete. Also deinhibit their ancestors to handle the situation
    # where _restackonce() is being used across several transactions
    # (such as calls to `hg next --rebase`), because each transaction
    # close will result in the ancestors being re-inhibited if they have
    # unrebased (and therefore unstable) descendants. As such, the final
    # call to _restackonce() at the top of the stack should deinhibit the
    # entire stack.
    ancestors = repo.set('%ld %% %d', allprecursors, rev)
    _deinhibit(repo, ancestors)

def _findrestacktargets(repo, base):
    """Starting from the given base revision, do a BFS forwards through
       history, looking for changesets with unstable descendants on their
       precursors. Returns a list of any such changesets, in a top-down
       ordering that will allow all of the descendants of their precursors
       to be correctly rebased.
    """
    childrenof = _getchildrelationships(repo,
        repo.revs('%d + allprecursors(%d)', base, base))

    # Perform BFS starting from base.
    queue = deque([base])
    targets = []
    processed = set()
    while queue:
        rev = queue.popleft()

        # Merges may result in the same revision being added to the queue
        # multiple times. Filter those cases out.
        if rev in processed:
            continue

        processed.add(rev)

        # Children need to be added in sorted order so that newer
        # children (as determined by rev number) will have their
        # descendants of their precursors rebased before older children.
        # This ensures that unstable changesets will always be rebased
        # onto the latest visible successor of their parent changeset.
        queue.extend(sorted(childrenof[rev]))

        # Look for visible precursors (which are probably visible because
        # they have unstable descendants) and successors (for which the latest
        # non-obsolete version should be visible).
        precursors = repo.revs('allprecursors(%d)', rev)
        successors = repo.revs('allsuccessors(%d)', rev)

        # If this changeset has precursors but no successor, then
        # if its precursors have children those children need to be
        # rebased onto the changeset.
        if precursors and not successors:
            children = []
            for p in precursors:
                children.extend(childrenof[p])
            if children:
                queue.extend(children)
                targets.append(rev)

    # We need to perform the rebases in reverse-BFS order so that
    # obsolescence information at lower levels is not modified by rebases
    # at higher levels.
    return reversed(targets)

def _getchildrelationships(repo, revs):
    """Build a defaultdict of child relationships between all descendants of
       revs. This information will prevent us from having to repeatedly
       perform children that reconstruct these relationships each time.
    """
    cl = repo.changelog
    children = defaultdict(set)
    for rev in repo.revs('(%ld)::', revs):
        for parent in cl.parentrevs(rev):
            if parent != nullrev:
                children[parent].add(rev)
    return children

def _latest(repo, rev):
    """Find the "latest version" of the given revision -- either the
       latest visible successor, or the revision itself if it has no
       visible successors.
    """
    latest = repo.revs('allsuccessors(%d)', rev).last()
    return latest if latest is not None else rev

def _clearpreamend(repo, revs):
    """Remove any preamend bookmarks on the given revisions."""
    cl = repo.changelog
    for rev in revs:
        for bookmark in repo.nodebookmarks(cl.node(rev)):
            if bookmark.endswith('.preamend'):
                repo._bookmarks.pop(bookmark, None)

def _deinhibit(repo, contexts):
    """Remove any inhibit markers on the given change contexts."""
    if inhibitmod:
        inhibitmod._deinhibitmarkers(repo, (ctx.node() for ctx in contexts))

def _hideopts(entry, opts):
    """Remove the given set of options from the given command entry.
       Destructively modifies the entry.
    """
    # Each command entry is a tuple, and thus immutable. As such we need
    # to delete each option from the original list, rather than building
    # a new, filtered list. Iterate backwards to prevent indicies from changing
    # as we delete entries.
    for i, opt in reversed(list(enumerate(entry[1]))):
        if opt[1] in opts:
            del entry[1][i]

def _activate(ui, repo, rev):
    """Activate the bookmark on the given revision if it
       only has one bookmark.
    """
    ctx = repo[rev]
    bookmarks = repo.nodebookmarks(ctx.node())
    if len(bookmarks) == 1:
        ui.status(_("(activating bookmark %s)\n") % bookmarks[0])
        bmactivate(repo, bookmarks[0])

def _showchangesets(ui, repo, contexts=None, revs=None):
    """Pretty print a list of changesets. Can take either a list of
       change contexts or a list of revision numbers.
    """
    if contexts is None:
        contexts = []
    if revs is not None:
        contexts.extend(repo[r] for r in revs)
    showopts = {
        'template': '[{shortest(node, 6)}] {if(bookmarks, "({bookmarks}) ")}'
                    '{desc|firstline}\n'
    }
    displayer = cmdutil.show_changeset(ui, repo, showopts)
    for ctx in contexts:
        displayer.show(ctx)

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

def _setbookmark(repo, tr, bookmark, rev):
    """Make the given bookmark point to the given revision."""
    node = repo.changelog.node(rev)
    repo._bookmarks[bookmark] = node
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
