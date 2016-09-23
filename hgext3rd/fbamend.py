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
"""

from mercurial import util, cmdutil, phases, commands, bookmarks, repair
from mercurial import merge, extensions, error, scmutil, hg, util
from mercurial.node import hex, nullid
from mercurial import obsolete
from mercurial import lock as lockmod
from mercurial.i18n import _
import errno, os, re
from contextlib import nested

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

rebasemod = None

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
    def wrapnext(loaded):
        if not loaded:
            return
        evolvemod = extensions.find('evolve')
        entry = extensions.wrapcommand(evolvemod.cmdtable, 'next', nextrebase)

        # Remove `hg next --evolve` and add `hg next --rebase`.
        # Can't use a list comprehension since the list is in a tuple.
        for i, opt in enumerate(entry[1]):
            if opt[1] == 'evolve':
                del entry[1][i]
                break
        entry[1].append((
            '', 'rebase', False, _('rebase the changeset if necessary')
        ))

    extensions.afterloaded('evolve', wrapnext)

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
    currev = curctx.rev()

    # identify the commit to which to unamend
    markers = list(obsolete.precursormarkers(curctx))
    if len(markers) != 1:
        e = _("changeset must have one precursor, found %i precursors")
        raise error.Abort(e % len(markers))

    precnode = markers[0].precnode()
    precctx = unfi[precnode]
    precrev = precctx.rev()

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

        if not _histediting(repo):
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

def nextrebase(orig, ui, repo, **opts):
    """Wrapper around the evolve extension's next command, adding the
       --rebase option, which detects whether the current changeset has
       any children on an obsolete precursor, and if so, rebases those
       children onto the current version.
    """
    # Just perform `hg next` if no --rebase option.
    if not opts['rebase']:
        return orig(ui, repo, **opts)

    # Abort if there is an unfinished operation or changes to the
    # working copy, to be consistent with the behavior of `hg next`.
    cmdutil.checkunfinished(repo)
    cmdutil.bailifchanged(repo)

    # Find any child changesets on the changeset's precursor, if one exists.
    children = []
    for p in repo.set('allprecursors(.)'):
        children.extend(d.hex() for d in p.descendants())

    # If doing a dry run, just print out the corresponding commands.
    if opts['dry_run']:
        if children:
            rev = '+'.join(children)
            dest = repo['.'].hex()
            ui.write(('hg rebase -r %s -d %s -k\n' % (rev, dest)))
        # Since we don't know what the new hashes will be until we actually
        # perform the rebase, the dry run output can't explicitly say
        # `hg update %s`. This is different from the normal output
        # of `hg next --dry-run`.
        ui.write(('hg next\n'))
        return 0

    # Rebase any children of the obsolete changesets.
    if children:
        rebaseopts = {
            'rev': children,
            'dest': repo['.'].hex(),
            'keep': True,
        }
        try:
            rebasemod.rebase(ui, repo, **rebaseopts)
        except error.InterventionRequired:
            ui.status(_(
                "please resolve any conflicts, run 'hg rebase --continue', "
                "and then run 'hg next'\n"
            ))
            raise

    # Only call `hg next` if there were no conflicts.
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
