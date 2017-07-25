# rebase.py - rebasing feature for mercurial
#
# Copyright 2008 Stefano Tortarolo <stefano.tortarolo at gmail dot com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''command to move sets of revisions to a different ancestor

This extension lets you rebase changesets in an existing Mercurial
repository.

For more information:
https://mercurial-scm.org/wiki/RebaseExtension
'''

from __future__ import absolute_import

import errno
import os

from mercurial.i18n import _
from mercurial.node import (
    hex,
    nullid,
    nullrev,
    short,
)
from mercurial import (
    bookmarks,
    cmdutil,
    commands,
    copies,
    destutil,
    dirstateguard,
    error,
    extensions,
    hg,
    lock,
    merge as mergemod,
    mergeutil,
    obsolete,
    obsutil,
    patch,
    phases,
    registrar,
    repair,
    repoview,
    revset,
    scmutil,
    smartset,
    util,
)

release = lock.release
templateopts = cmdutil.templateopts

# The following constants are used throughout the rebase module. The ordering of
# their values must be maintained.

# Indicates that a revision needs to be rebased
revtodo = -1
nullmerge = -2
revignored = -3
# successor in rebase destination
revprecursor = -4
# plain prune (no successor)
revpruned = -5
revskipped = (revignored, revprecursor, revpruned)

cmdtable = {}
command = registrar.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

def _nothingtorebase():
    return 1

def _savegraft(ctx, extra):
    s = ctx.extra().get('source', None)
    if s is not None:
        extra['source'] = s
    s = ctx.extra().get('intermediate-source', None)
    if s is not None:
        extra['intermediate-source'] = s

def _savebranch(ctx, extra):
    extra['branch'] = ctx.branch()

def _makeextrafn(copiers):
    """make an extrafn out of the given copy-functions.

    A copy function takes a context and an extra dict, and mutates the
    extra dict as needed based on the given context.
    """
    def extrafn(ctx, extra):
        for c in copiers:
            c(ctx, extra)
    return extrafn

def _destrebase(repo, sourceset, destspace=None):
    """small wrapper around destmerge to pass the right extra args

    Please wrap destutil.destmerge instead."""
    return destutil.destmerge(repo, action='rebase', sourceset=sourceset,
                              onheadcheck=False, destspace=destspace)

revsetpredicate = registrar.revsetpredicate()

@revsetpredicate('_destrebase')
def _revsetdestrebase(repo, subset, x):
    # ``_rebasedefaultdest()``

    # default destination for rebase.
    # # XXX: Currently private because I expect the signature to change.
    # # XXX: - bailing out in case of ambiguity vs returning all data.
    # i18n: "_rebasedefaultdest" is a keyword
    sourceset = None
    if x is not None:
        sourceset = revset.getset(repo, smartset.fullreposet(repo), x)
    return subset & smartset.baseset([_destrebase(repo, sourceset)])

class rebaseruntime(object):
    """This class is a container for rebase runtime state"""
    def __init__(self, repo, ui, opts=None):
        if opts is None:
            opts = {}

        self.repo = repo
        self.ui = ui
        self.opts = opts
        self.originalwd = None
        self.external = nullrev
        # Mapping between the old revision id and either what is the new rebased
        # revision or what needs to be done with the old revision. The state
        # dict will be what contains most of the rebase progress state.
        self.state = {}
        self.activebookmark = None
        self.dest = None
        self.skipped = set()
        self.destancestors = set()

        self.collapsef = opts.get('collapse', False)
        self.collapsemsg = cmdutil.logmessage(ui, opts)
        self.date = opts.get('date', None)

        e = opts.get('extrafn') # internal, used by e.g. hgsubversion
        self.extrafns = [_savegraft]
        if e:
            self.extrafns = [e]

        self.keepf = opts.get('keep', False)
        self.keepbranchesf = opts.get('keepbranches', False)
        # keepopen is not meant for use on the command line, but by
        # other extensions
        self.keepopen = opts.get('keepopen', False)
        self.obsoletenotrebased = {}

    def storestatus(self, tr=None):
        """Store the current status to allow recovery"""
        if tr:
            tr.addfilegenerator('rebasestate', ('rebasestate',),
                                self._writestatus, location='plain')
        else:
            with self.repo.vfs("rebasestate", "w") as f:
                self._writestatus(f)

    def _writestatus(self, f):
        repo = self.repo.unfiltered()
        f.write(repo[self.originalwd].hex() + '\n')
        f.write(repo[self.dest].hex() + '\n')
        f.write(repo[self.external].hex() + '\n')
        f.write('%d\n' % int(self.collapsef))
        f.write('%d\n' % int(self.keepf))
        f.write('%d\n' % int(self.keepbranchesf))
        f.write('%s\n' % (self.activebookmark or ''))
        for d, v in self.state.iteritems():
            oldrev = repo[d].hex()
            if v >= 0:
                newrev = repo[v].hex()
            elif v == revtodo:
                # To maintain format compatibility, we have to use nullid.
                # Please do remove this special case when upgrading the format.
                newrev = hex(nullid)
            else:
                newrev = v
            f.write("%s:%s\n" % (oldrev, newrev))
        repo.ui.debug('rebase status stored\n')

    def restorestatus(self):
        """Restore a previously stored status"""
        repo = self.repo
        keepbranches = None
        dest = None
        collapse = False
        external = nullrev
        activebookmark = None
        state = {}

        try:
            f = repo.vfs("rebasestate")
            for i, l in enumerate(f.read().splitlines()):
                if i == 0:
                    originalwd = repo[l].rev()
                elif i == 1:
                    dest = repo[l].rev()
                elif i == 2:
                    external = repo[l].rev()
                elif i == 3:
                    collapse = bool(int(l))
                elif i == 4:
                    keep = bool(int(l))
                elif i == 5:
                    keepbranches = bool(int(l))
                elif i == 6 and not (len(l) == 81 and ':' in l):
                    # line 6 is a recent addition, so for backwards
                    # compatibility check that the line doesn't look like the
                    # oldrev:newrev lines
                    activebookmark = l
                else:
                    oldrev, newrev = l.split(':')
                    if newrev in (str(nullmerge), str(revignored),
                                  str(revprecursor), str(revpruned)):
                        state[repo[oldrev].rev()] = int(newrev)
                    elif newrev == nullid:
                        state[repo[oldrev].rev()] = revtodo
                        # Legacy compat special case
                    else:
                        state[repo[oldrev].rev()] = repo[newrev].rev()

        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            cmdutil.wrongtooltocontinue(repo, _('rebase'))

        if keepbranches is None:
            raise error.Abort(_('.hg/rebasestate is incomplete'))

        skipped = set()
        # recompute the set of skipped revs
        if not collapse:
            seen = {dest}
            for old, new in sorted(state.items()):
                if new != revtodo and new in seen:
                    skipped.add(old)
                seen.add(new)
        repo.ui.debug('computed skipped revs: %s\n' %
                        (' '.join(str(r) for r in sorted(skipped)) or None))
        repo.ui.debug('rebase status resumed\n')
        _setrebasesetvisibility(repo, set(state.keys()) | {originalwd})

        self.originalwd = originalwd
        self.dest = dest
        self.state = state
        self.skipped = skipped
        self.collapsef = collapse
        self.keepf = keep
        self.keepbranchesf = keepbranches
        self.external = external
        self.activebookmark = activebookmark

    def _handleskippingobsolete(self, rebaserevs, obsoleterevs, dest):
        """Compute structures necessary for skipping obsolete revisions

        rebaserevs:     iterable of all revisions that are to be rebased
        obsoleterevs:   iterable of all obsolete revisions in rebaseset
        dest:           a destination revision for the rebase operation
        """
        self.obsoletenotrebased = {}
        if not self.ui.configbool('experimental', 'rebaseskipobsolete',
                                  default=True):
            return
        rebaseset = set(rebaserevs)
        obsoleteset = set(obsoleterevs)
        self.obsoletenotrebased = _computeobsoletenotrebased(self.repo,
                                    obsoleteset, dest)
        skippedset = set(self.obsoletenotrebased)
        _checkobsrebase(self.repo, self.ui, obsoleteset, rebaseset, skippedset)

    def _prepareabortorcontinue(self, isabort):
        try:
            self.restorestatus()
            self.collapsemsg = restorecollapsemsg(self.repo, isabort)
        except error.RepoLookupError:
            if isabort:
                clearstatus(self.repo)
                clearcollapsemsg(self.repo)
                self.repo.ui.warn(_('rebase aborted (no revision is removed,'
                                    ' only broken state is cleared)\n'))
                return 0
            else:
                msg = _('cannot continue inconsistent rebase')
                hint = _('use "hg rebase --abort" to clear broken state')
                raise error.Abort(msg, hint=hint)
        if isabort:
            return abort(self.repo, self.originalwd, self.dest,
                         self.state, activebookmark=self.activebookmark)

        obsrevs = (r for r, st in self.state.items() if st == revprecursor)
        self._handleskippingobsolete(self.state.keys(), obsrevs, self.dest)

    def _preparenewrebase(self, dest, rebaseset):
        if dest is None:
            return _nothingtorebase()

        allowunstable = obsolete.isenabled(self.repo, obsolete.allowunstableopt)
        if (not (self.keepf or allowunstable)
              and self.repo.revs('first(children(%ld) - %ld)',
                                 rebaseset, rebaseset)):
            raise error.Abort(
                _("can't remove original changesets with"
                  " unrebased descendants"),
                hint=_('use --keep to keep original changesets'))

        obsrevs = _filterobsoleterevs(self.repo, set(rebaseset))
        self._handleskippingobsolete(rebaseset, obsrevs, dest.rev())

        result = buildstate(self.repo, dest, rebaseset, self.collapsef,
                            self.obsoletenotrebased)

        if not result:
            # Empty state built, nothing to rebase
            self.ui.status(_('nothing to rebase\n'))
            return _nothingtorebase()

        for root in self.repo.set('roots(%ld)', rebaseset):
            if not self.keepf and not root.mutable():
                raise error.Abort(_("can't rebase public changeset %s")
                                  % root,
                                  hint=_("see 'hg help phases' for details"))

        (self.originalwd, self.dest, self.state) = result
        if self.collapsef:
            self.destancestors = self.repo.changelog.ancestors(
                                        [self.dest],
                                        inclusive=True)
            self.external = externalparent(self.repo, self.state,
                                              self.destancestors)

        if dest.closesbranch() and not self.keepbranchesf:
            self.ui.status(_('reopening closed branch head %s\n') % dest)

    def _performrebase(self, tr):
        repo, ui, opts = self.repo, self.ui, self.opts
        if self.keepbranchesf:
            # insert _savebranch at the start of extrafns so if
            # there's a user-provided extrafn it can clobber branch if
            # desired
            self.extrafns.insert(0, _savebranch)
            if self.collapsef:
                branches = set()
                for rev in self.state:
                    branches.add(repo[rev].branch())
                    if len(branches) > 1:
                        raise error.Abort(_('cannot collapse multiple named '
                            'branches'))

        # Rebase
        if not self.destancestors:
            self.destancestors = repo.changelog.ancestors([self.dest],
                                                          inclusive=True)

        # Keep track of the active bookmarks in order to reset them later
        self.activebookmark = self.activebookmark or repo._activebookmark
        if self.activebookmark:
            bookmarks.deactivate(repo)

        # Store the state before we begin so users can run 'hg rebase --abort'
        # if we fail before the transaction closes.
        self.storestatus()

        sortedrevs = repo.revs('sort(%ld, -topo)', self.state)
        cands = [k for k, v in self.state.iteritems() if v == revtodo]
        total = len(cands)
        pos = 0
        for rev in sortedrevs:
            ctx = repo[rev]
            desc = '%d:%s "%s"' % (ctx.rev(), ctx,
                                   ctx.description().split('\n', 1)[0])
            names = repo.nodetags(ctx.node()) + repo.nodebookmarks(ctx.node())
            if names:
                desc += ' (%s)' % ' '.join(names)
            if self.state[rev] == rev:
                ui.status(_('already rebased %s\n') % desc)
            elif self.state[rev] == revtodo:
                pos += 1
                ui.status(_('rebasing %s\n') % desc)
                ui.progress(_("rebasing"), pos, ("%d:%s" % (rev, ctx)),
                            _('changesets'), total)
                p1, p2, base = defineparents(repo, rev, self.dest,
                                             self.state,
                                             self.destancestors,
                                             self.obsoletenotrebased)
                self.storestatus(tr=tr)
                storecollapsemsg(repo, self.collapsemsg)
                if len(repo[None].parents()) == 2:
                    repo.ui.debug('resuming interrupted rebase\n')
                else:
                    try:
                        ui.setconfig('ui', 'forcemerge', opts.get('tool', ''),
                                     'rebase')
                        stats = rebasenode(repo, rev, p1, base, self.state,
                                           self.collapsef, self.dest)
                        if stats and stats[3] > 0:
                            raise error.InterventionRequired(
                                _('unresolved conflicts (see hg '
                                  'resolve, then hg rebase --continue)'))
                    finally:
                        ui.setconfig('ui', 'forcemerge', '', 'rebase')
                if not self.collapsef:
                    merging = p2 != nullrev
                    editform = cmdutil.mergeeditform(merging, 'rebase')
                    editor = cmdutil.getcommiteditor(editform=editform, **opts)
                    newnode = concludenode(repo, rev, p1, p2,
                                           extrafn=_makeextrafn(self.extrafns),
                                           editor=editor,
                                           keepbranches=self.keepbranchesf,
                                           date=self.date)
                    if newnode is None:
                        # If it ended up being a no-op commit, then the normal
                        # merge state clean-up path doesn't happen, so do it
                        # here. Fix issue5494
                        mergemod.mergestate.clean(repo)
                else:
                    # Skip commit if we are collapsing
                    repo.setparents(repo[p1].node())
                    newnode = None
                # Update the state
                if newnode is not None:
                    self.state[rev] = repo[newnode].rev()
                    ui.debug('rebased as %s\n' % short(newnode))
                else:
                    if not self.collapsef:
                        ui.warn(_('note: rebase of %d:%s created no changes '
                                  'to commit\n') % (rev, ctx))
                        self.skipped.add(rev)
                    self.state[rev] = p1
                    ui.debug('next revision set to %s\n' % p1)
            elif self.state[rev] == nullmerge:
                ui.debug('ignoring null merge rebase of %s\n' % rev)
            elif self.state[rev] == revignored:
                ui.status(_('not rebasing ignored %s\n') % desc)
            elif self.state[rev] == revprecursor:
                destctx = repo[self.obsoletenotrebased[rev]]
                descdest = '%d:%s "%s"' % (destctx.rev(), destctx,
                           destctx.description().split('\n', 1)[0])
                msg = _('note: not rebasing %s, already in destination as %s\n')
                ui.status(msg % (desc, descdest))
            elif self.state[rev] == revpruned:
                msg = _('note: not rebasing %s, it has no successor\n')
                ui.status(msg % desc)
            else:
                ui.status(_('already rebased %s as %s\n') %
                          (desc, repo[self.state[rev]]))

        ui.progress(_('rebasing'), None)
        ui.note(_('rebase merging completed\n'))

    def _finishrebase(self):
        repo, ui, opts = self.repo, self.ui, self.opts
        if self.collapsef and not self.keepopen:
            p1, p2, _base = defineparents(repo, min(self.state),
                                          self.dest, self.state,
                                          self.destancestors,
                                          self.obsoletenotrebased)
            editopt = opts.get('edit')
            editform = 'rebase.collapse'
            if self.collapsemsg:
                commitmsg = self.collapsemsg
            else:
                commitmsg = 'Collapsed revision'
                for rebased in self.state:
                    if rebased not in self.skipped and\
                       self.state[rebased] > nullmerge:
                        commitmsg += '\n* %s' % repo[rebased].description()
                editopt = True
            editor = cmdutil.getcommiteditor(edit=editopt, editform=editform)
            revtoreuse = max(self.state)
            newnode = concludenode(repo, revtoreuse, p1, self.external,
                                   commitmsg=commitmsg,
                                   extrafn=_makeextrafn(self.extrafns),
                                   editor=editor,
                                   keepbranches=self.keepbranchesf,
                                   date=self.date)
            if newnode is None:
                newrev = self.dest
            else:
                newrev = repo[newnode].rev()
            for oldrev in self.state.iterkeys():
                if self.state[oldrev] > nullmerge:
                    self.state[oldrev] = newrev

        if 'qtip' in repo.tags():
            updatemq(repo, self.state, self.skipped, **opts)

        # restore original working directory
        # (we do this before stripping)
        newwd = self.state.get(self.originalwd, self.originalwd)
        if newwd == revprecursor:
            newwd = self.obsoletenotrebased[self.originalwd]
        elif newwd < 0:
            # original directory is a parent of rebase set root or ignored
            newwd = self.originalwd
        if newwd not in [c.rev() for c in repo[None].parents()]:
            ui.note(_("update back to initial working directory parent\n"))
            hg.updaterepo(repo, newwd, False)

        if not self.keepf:
            collapsedas = None
            if self.collapsef:
                collapsedas = newnode
            clearrebased(ui, repo, self.dest, self.state, self.skipped,
                         collapsedas)

        clearstatus(repo)
        clearcollapsemsg(repo)

        ui.note(_("rebase completed\n"))
        util.unlinkpath(repo.sjoin('undo'), ignoremissing=True)
        if self.skipped:
            skippedlen = len(self.skipped)
            ui.note(_("%d revisions have been skipped\n") % skippedlen)

        if (self.activebookmark and self.activebookmark in repo._bookmarks and
            repo['.'].node() == repo._bookmarks[self.activebookmark]):
                bookmarks.activate(repo, self.activebookmark)

@command('rebase',
    [('s', 'source', '',
     _('rebase the specified changeset and descendants'), _('REV')),
    ('b', 'base', '',
     _('rebase everything from branching point of specified changeset'),
     _('REV')),
    ('r', 'rev', [],
     _('rebase these revisions'),
     _('REV')),
    ('d', 'dest', '',
     _('rebase onto the specified changeset'), _('REV')),
    ('', 'collapse', False, _('collapse the rebased changesets')),
    ('m', 'message', '',
     _('use text as collapse commit message'), _('TEXT')),
    ('e', 'edit', False, _('invoke editor on commit messages')),
    ('l', 'logfile', '',
     _('read collapse commit message from file'), _('FILE')),
    ('k', 'keep', False, _('keep original changesets')),
    ('', 'keepbranches', False, _('keep original branch names')),
    ('D', 'detach', False, _('(DEPRECATED)')),
    ('i', 'interactive', False, _('(DEPRECATED)')),
    ('t', 'tool', '', _('specify merge tool')),
    ('c', 'continue', False, _('continue an interrupted rebase')),
    ('a', 'abort', False, _('abort an interrupted rebase'))] +
     templateopts,
    _('[-s REV | -b REV] [-d REV] [OPTION]'))
def rebase(ui, repo, **opts):
    """move changeset (and descendants) to a different branch

    Rebase uses repeated merging to graft changesets from one part of
    history (the source) onto another (the destination). This can be
    useful for linearizing *local* changes relative to a master
    development tree.

    Published commits cannot be rebased (see :hg:`help phases`).
    To copy commits, see :hg:`help graft`.

    If you don't specify a destination changeset (``-d/--dest``), rebase
    will use the same logic as :hg:`merge` to pick a destination.  if
    the current branch contains exactly one other head, the other head
    is merged with by default.  Otherwise, an explicit revision with
    which to merge with must be provided.  (destination changeset is not
    modified by rebasing, but new changesets are added as its
    descendants.)

    Here are the ways to select changesets:

      1. Explicitly select them using ``--rev``.

      2. Use ``--source`` to select a root changeset and include all of its
         descendants.

      3. Use ``--base`` to select a changeset; rebase will find ancestors
         and their descendants which are not also ancestors of the destination.

      4. If you do not specify any of ``--rev``, ``source``, or ``--base``,
         rebase will use ``--base .`` as above.

    Rebase will destroy original changesets unless you use ``--keep``.
    It will also move your bookmarks (even if you do).

    Some changesets may be dropped if they do not contribute changes
    (e.g. merges from the destination branch).

    Unlike ``merge``, rebase will do nothing if you are at the branch tip of
    a named branch with two heads. You will need to explicitly specify source
    and/or destination.

    If you need to use a tool to automate merge/conflict decisions, you
    can specify one with ``--tool``, see :hg:`help merge-tools`.
    As a caveat: the tool will not be used to mediate when a file was
    deleted, there is no hook presently available for this.

    If a rebase is interrupted to manually resolve a conflict, it can be
    continued with --continue/-c or aborted with --abort/-a.

    .. container:: verbose

      Examples:

      - move "local changes" (current commit back to branching point)
        to the current branch tip after a pull::

          hg rebase

      - move a single changeset to the stable branch::

          hg rebase -r 5f493448 -d stable

      - splice a commit and all its descendants onto another part of history::

          hg rebase --source c0c3 --dest 4cf9

      - rebase everything on a branch marked by a bookmark onto the
        default branch::

          hg rebase --base myfeature --dest default

      - collapse a sequence of changes into a single commit::

          hg rebase --collapse -r 1520:1525 -d .

      - move a named branch while preserving its name::

          hg rebase -r "branch(featureX)" -d 1.3 --keepbranches

    Configuration Options:

    You can make rebase require a destination if you set the following config
    option::

      [commands]
      rebase.requiredest = True

    By default, rebase will close the transaction after each commit. For
    performance purposes, you can configure rebase to use a single transaction
    across the entire rebase. WARNING: This setting introduces a significant
    risk of losing the work you've done in a rebase if the rebase aborts
    unexpectedly::

      [rebase]
      singletransaction = True

    Return Values:

    Returns 0 on success, 1 if nothing to rebase or there are
    unresolved conflicts.

    """
    rbsrt = rebaseruntime(repo, ui, opts)

    with repo.wlock(), repo.lock():
        # Validate input and define rebasing points
        destf = opts.get('dest', None)
        srcf = opts.get('source', None)
        basef = opts.get('base', None)
        revf = opts.get('rev', [])
        # search default destination in this space
        # used in the 'hg pull --rebase' case, see issue 5214.
        destspace = opts.get('_destspace')
        contf = opts.get('continue')
        abortf = opts.get('abort')
        if opts.get('interactive'):
            try:
                if extensions.find('histedit'):
                    enablehistedit = ''
            except KeyError:
                enablehistedit = " --config extensions.histedit="
            help = "hg%s help -e histedit" % enablehistedit
            msg = _("interactive history editing is supported by the "
                    "'histedit' extension (see \"%s\")") % help
            raise error.Abort(msg)

        if rbsrt.collapsemsg and not rbsrt.collapsef:
            raise error.Abort(
                _('message can only be specified with collapse'))

        if contf or abortf:
            if contf and abortf:
                raise error.Abort(_('cannot use both abort and continue'))
            if rbsrt.collapsef:
                raise error.Abort(
                    _('cannot use collapse with continue or abort'))
            if srcf or basef or destf:
                raise error.Abort(
                    _('abort and continue do not allow specifying revisions'))
            if abortf and opts.get('tool', False):
                ui.warn(_('tool option will be ignored\n'))
            if contf:
                ms = mergemod.mergestate.read(repo)
                mergeutil.checkunresolved(ms)

            retcode = rbsrt._prepareabortorcontinue(abortf)
            if retcode is not None:
                return retcode
        else:
            dest, rebaseset = _definesets(ui, repo, destf, srcf, basef, revf,
                                          destspace=destspace)
            retcode = rbsrt._preparenewrebase(dest, rebaseset)
            if retcode is not None:
                return retcode

        tr = None
        if ui.configbool('rebase', 'singletransaction'):
            tr = repo.transaction('rebase')
        with util.acceptintervention(tr):
            rbsrt._performrebase(tr)

        rbsrt._finishrebase()

def _definesets(ui, repo, destf=None, srcf=None, basef=None, revf=None,
                destspace=None):
    """use revisions argument to define destination and rebase set
    """
    if revf is None:
        revf = []

    # destspace is here to work around issues with `hg pull --rebase` see
    # issue5214 for details
    if srcf and basef:
        raise error.Abort(_('cannot specify both a source and a base'))
    if revf and basef:
        raise error.Abort(_('cannot specify both a revision and a base'))
    if revf and srcf:
        raise error.Abort(_('cannot specify both a revision and a source'))

    cmdutil.checkunfinished(repo)
    cmdutil.bailifchanged(repo)

    if ui.configbool('commands', 'rebase.requiredest') and not destf:
        raise error.Abort(_('you must specify a destination'),
                          hint=_('use: hg rebase -d REV'))

    if destf:
        dest = scmutil.revsingle(repo, destf)

    if revf:
        rebaseset = scmutil.revrange(repo, revf)
        if not rebaseset:
            ui.status(_('empty "rev" revision set - nothing to rebase\n'))
            return None, None
    elif srcf:
        src = scmutil.revrange(repo, [srcf])
        if not src:
            ui.status(_('empty "source" revision set - nothing to rebase\n'))
            return None, None
        rebaseset = repo.revs('(%ld)::', src)
        assert rebaseset
    else:
        base = scmutil.revrange(repo, [basef or '.'])
        if not base:
            ui.status(_('empty "base" revision set - '
                        "can't compute rebase set\n"))
            return None, None
        if not destf:
            dest = repo[_destrebase(repo, base, destspace=destspace)]
            destf = str(dest)

        roots = [] # selected children of branching points
        bpbase = {} # {branchingpoint: [origbase]}
        for b in base: # group bases by branching points
            bp = repo.revs('ancestor(%d, %d)', b, dest).first()
            bpbase[bp] = bpbase.get(bp, []) + [b]
        if None in bpbase:
            # emulate the old behavior, showing "nothing to rebase" (a better
            # behavior may be abort with "cannot find branching point" error)
            bpbase.clear()
        for bp, bs in bpbase.iteritems(): # calculate roots
            roots += list(repo.revs('children(%d) & ancestors(%ld)', bp, bs))

        rebaseset = repo.revs('%ld::', roots)

        if not rebaseset:
            # transform to list because smartsets are not comparable to
            # lists. This should be improved to honor laziness of
            # smartset.
            if list(base) == [dest.rev()]:
                if basef:
                    ui.status(_('nothing to rebase - %s is both "base"'
                                ' and destination\n') % dest)
                else:
                    ui.status(_('nothing to rebase - working directory '
                                'parent is also destination\n'))
            elif not repo.revs('%ld - ::%d', base, dest):
                if basef:
                    ui.status(_('nothing to rebase - "base" %s is '
                                'already an ancestor of destination '
                                '%s\n') %
                              ('+'.join(str(repo[r]) for r in base),
                               dest))
                else:
                    ui.status(_('nothing to rebase - working '
                                'directory parent is already an '
                                'ancestor of destination %s\n') % dest)
            else: # can it happen?
                ui.status(_('nothing to rebase from %s to %s\n') %
                          ('+'.join(str(repo[r]) for r in base), dest))
            return None, None

    if not destf:
        dest = repo[_destrebase(repo, rebaseset, destspace=destspace)]
        destf = str(dest)

    return dest, rebaseset

def externalparent(repo, state, destancestors):
    """Return the revision that should be used as the second parent
    when the revisions in state is collapsed on top of destancestors.
    Abort if there is more than one parent.
    """
    parents = set()
    source = min(state)
    for rev in state:
        if rev == source:
            continue
        for p in repo[rev].parents():
            if (p.rev() not in state
                        and p.rev() not in destancestors):
                parents.add(p.rev())
    if not parents:
        return nullrev
    if len(parents) == 1:
        return parents.pop()
    raise error.Abort(_('unable to collapse on top of %s, there is more '
                       'than one external parent: %s') %
                     (max(destancestors),
                      ', '.join(str(p) for p in sorted(parents))))

def concludenode(repo, rev, p1, p2, commitmsg=None, editor=None, extrafn=None,
                 keepbranches=False, date=None):
    '''Commit the wd changes with parents p1 and p2. Reuse commit info from rev
    but also store useful information in extra.
    Return node of committed revision.'''
    dsguard = dirstateguard.dirstateguard(repo, 'rebase')
    try:
        repo.setparents(repo[p1].node(), repo[p2].node())
        ctx = repo[rev]
        if commitmsg is None:
            commitmsg = ctx.description()
        keepbranch = keepbranches and repo[p1].branch() != ctx.branch()
        extra = {'rebase_source': ctx.hex()}
        if extrafn:
            extrafn(ctx, extra)

        destphase = max(ctx.phase(), phases.draft)
        overrides = {('phases', 'new-commit'): destphase}
        with repo.ui.configoverride(overrides, 'rebase'):
            if keepbranch:
                repo.ui.setconfig('ui', 'allowemptycommit', True)
            # Commit might fail if unresolved files exist
            if date is None:
                date = ctx.date()
            newnode = repo.commit(text=commitmsg, user=ctx.user(),
                                  date=date, extra=extra, editor=editor)

        repo.dirstate.setbranch(repo[newnode].branch())
        dsguard.close()
        return newnode
    finally:
        release(dsguard)

def rebasenode(repo, rev, p1, base, state, collapse, dest):
    'Rebase a single revision rev on top of p1 using base as merge ancestor'
    # Merge phase
    # Update to destination and merge it with local
    if repo['.'].rev() != p1:
        repo.ui.debug(" update to %d:%s\n" % (p1, repo[p1]))
        mergemod.update(repo, p1, False, True)
    else:
        repo.ui.debug(" already in destination\n")
    repo.dirstate.write(repo.currenttransaction())
    repo.ui.debug(" merge against %d:%s\n" % (rev, repo[rev]))
    if base is not None:
        repo.ui.debug("   detach base %d:%s\n" % (base, repo[base]))
    # When collapsing in-place, the parent is the common ancestor, we
    # have to allow merging with it.
    stats = mergemod.update(repo, rev, True, True, base, collapse,
                            labels=['dest', 'source'])
    if collapse:
        copies.duplicatecopies(repo, rev, dest)
    else:
        # If we're not using --collapse, we need to
        # duplicate copies between the revision we're
        # rebasing and its first parent, but *not*
        # duplicate any copies that have already been
        # performed in the destination.
        p1rev = repo[rev].p1().rev()
        copies.duplicatecopies(repo, rev, p1rev, skiprev=dest)
    return stats

def adjustdest(repo, rev, dest, state):
    """adjust rebase destination given the current rebase state

    rev is what is being rebased. Return a list of two revs, which are the
    adjusted destinations for rev's p1 and p2, respectively. If a parent is
    nullrev, return dest without adjustment for it.

    For example, when doing rebase -r B+E -d F, rebase will first move B to B1,
    and E's destination will be adjusted from F to B1.

        B1 <- written during rebasing B
        |
        F <- original destination of B, E
        |
        | E <- rev, which is being rebased
        | |
        | D <- prev, one parent of rev being checked
        | |
        | x <- skipped, ex. no successor or successor in (::dest)
        | |
        | C
        | |
        | B <- rebased as B1
        |/
        A

    Another example about merge changeset, rebase -r C+G+H -d K, rebase will
    first move C to C1, G to G1, and when it's checking H, the adjusted
    destinations will be [C1, G1].

            H       C1 G1
           /|       | /
          F G       |/
        K | |  ->   K
        | C D       |
        | |/        |
        | B         | ...
        |/          |/
        A           A
    """
    result = []
    for prev in repo.changelog.parentrevs(rev):
        adjusted = dest
        if prev != nullrev:
            # pick already rebased revs from state
            source = [s for s, d in state.items() if d > 0]
            candidate = repo.revs('max(%ld and (::%d))', source, prev).first()
            if candidate is not None:
                adjusted = state[candidate]
        result.append(adjusted)
    return result

def nearestrebased(repo, rev, state):
    """return the nearest ancestors of rev in the rebase result"""
    rebased = [r for r in state if state[r] > nullmerge]
    candidates = repo.revs('max(%ld  and (::%d))', rebased, rev)
    if candidates:
        return state[candidates.first()]
    else:
        return None

def _checkobsrebase(repo, ui, rebaseobsrevs, rebasesetrevs, rebaseobsskipped):
    """
    Abort if rebase will create divergence or rebase is noop because of markers

    `rebaseobsrevs`: set of obsolete revision in source
    `rebasesetrevs`: set of revisions to be rebased from source
    `rebaseobsskipped`: set of revisions from source skipped because they have
    successors in destination
    """
    # Obsolete node with successors not in dest leads to divergence
    divergenceok = ui.configbool('experimental',
                                 'allowdivergence')
    divergencebasecandidates = rebaseobsrevs - rebaseobsskipped

    if divergencebasecandidates and not divergenceok:
        divhashes = (str(repo[r])
                     for r in divergencebasecandidates)
        msg = _("this rebase will cause "
                "divergences from: %s")
        h = _("to force the rebase please set "
              "experimental.allowdivergence=True")
        raise error.Abort(msg % (",".join(divhashes),), hint=h)

def defineparents(repo, rev, dest, state, destancestors,
                  obsoletenotrebased):
    'Return the new parent relationship of the revision that will be rebased'
    parents = repo[rev].parents()
    p1 = p2 = nullrev
    rp1 = None

    p1n = parents[0].rev()
    if p1n in destancestors:
        p1 = dest
    elif p1n in state:
        if state[p1n] == nullmerge:
            p1 = dest
        elif state[p1n] in revskipped:
            p1 = nearestrebased(repo, p1n, state)
            if p1 is None:
                p1 = dest
        else:
            p1 = state[p1n]
    else: # p1n external
        p1 = dest
        p2 = p1n

    if len(parents) == 2 and parents[1].rev() not in destancestors:
        p2n = parents[1].rev()
        # interesting second parent
        if p2n in state:
            if p1 == dest: # p1n in destancestors or external
                p1 = state[p2n]
                if p1 == revprecursor:
                    rp1 = obsoletenotrebased[p2n]
            elif state[p2n] in revskipped:
                p2 = nearestrebased(repo, p2n, state)
                if p2 is None:
                    # no ancestors rebased yet, detach
                    p2 = dest
            else:
                p2 = state[p2n]
        else: # p2n external
            if p2 != nullrev: # p1n external too => rev is a merged revision
                raise error.Abort(_('cannot use revision %d as base, result '
                        'would have 3 parents') % rev)
            p2 = p2n
    repo.ui.debug(" future parents are %d and %d\n" %
                            (repo[rp1 or p1].rev(), repo[p2].rev()))

    if not any(p.rev() in state for p in parents):
        # Case (1) root changeset of a non-detaching rebase set.
        # Let the merge mechanism find the base itself.
        base = None
    elif not repo[rev].p2():
        # Case (2) detaching the node with a single parent, use this parent
        base = repo[rev].p1().rev()
    else:
        # Assuming there is a p1, this is the case where there also is a p2.
        # We are thus rebasing a merge and need to pick the right merge base.
        #
        # Imagine we have:
        # - M: current rebase revision in this step
        # - A: one parent of M
        # - B: other parent of M
        # - D: destination of this merge step (p1 var)
        #
        # Consider the case where D is a descendant of A or B and the other is
        # 'outside'. In this case, the right merge base is the D ancestor.
        #
        # An informal proof, assuming A is 'outside' and B is the D ancestor:
        #
        # If we pick B as the base, the merge involves:
        # - changes from B to M (actual changeset payload)
        # - changes from B to D (induced by rebase) as D is a rebased
        #   version of B)
        # Which exactly represent the rebase operation.
        #
        # If we pick A as the base, the merge involves:
        # - changes from A to M (actual changeset payload)
        # - changes from A to D (with include changes between unrelated A and B
        #   plus changes induced by rebase)
        # Which does not represent anything sensible and creates a lot of
        # conflicts. A is thus not the right choice - B is.
        #
        # Note: The base found in this 'proof' is only correct in the specified
        # case. This base does not make sense if is not D a descendant of A or B
        # or if the other is not parent 'outside' (especially not if the other
        # parent has been rebased). The current implementation does not
        # make it feasible to consider different cases separately. In these
        # other cases we currently just leave it to the user to correctly
        # resolve an impossible merge using a wrong ancestor.
        #
        # xx, p1 could be -4, and both parents could probably be -4...
        for p in repo[rev].parents():
            if state.get(p.rev()) == p1:
                base = p.rev()
                break
        else: # fallback when base not found
            base = None

            # Raise because this function is called wrong (see issue 4106)
            raise AssertionError('no base found to rebase on '
                                 '(defineparents called wrong)')
    return rp1 or p1, p2, base

def isagitpatch(repo, patchname):
    'Return true if the given patch is in git format'
    mqpatch = os.path.join(repo.mq.path, patchname)
    for line in patch.linereader(file(mqpatch, 'rb')):
        if line.startswith('diff --git'):
            return True
    return False

def updatemq(repo, state, skipped, **opts):
    'Update rebased mq patches - finalize and then import them'
    mqrebase = {}
    mq = repo.mq
    original_series = mq.fullseries[:]
    skippedpatches = set()

    for p in mq.applied:
        rev = repo[p.node].rev()
        if rev in state:
            repo.ui.debug('revision %d is an mq patch (%s), finalize it.\n' %
                                        (rev, p.name))
            mqrebase[rev] = (p.name, isagitpatch(repo, p.name))
        else:
            # Applied but not rebased, not sure this should happen
            skippedpatches.add(p.name)

    if mqrebase:
        mq.finish(repo, mqrebase.keys())

        # We must start import from the newest revision
        for rev in sorted(mqrebase, reverse=True):
            if rev not in skipped:
                name, isgit = mqrebase[rev]
                repo.ui.note(_('updating mq patch %s to %s:%s\n') %
                             (name, state[rev], repo[state[rev]]))
                mq.qimport(repo, (), patchname=name, git=isgit,
                                rev=[str(state[rev])])
            else:
                # Rebased and skipped
                skippedpatches.add(mqrebase[rev][0])

        # Patches were either applied and rebased and imported in
        # order, applied and removed or unapplied. Discard the removed
        # ones while preserving the original series order and guards.
        newseries = [s for s in original_series
                     if mq.guard_re.split(s, 1)[0] not in skippedpatches]
        mq.fullseries[:] = newseries
        mq.seriesdirty = True
        mq.savedirty()

def storecollapsemsg(repo, collapsemsg):
    'Store the collapse message to allow recovery'
    collapsemsg = collapsemsg or ''
    f = repo.vfs("last-message.txt", "w")
    f.write("%s\n" % collapsemsg)
    f.close()

def clearcollapsemsg(repo):
    'Remove collapse message file'
    repo.vfs.unlinkpath("last-message.txt", ignoremissing=True)

def restorecollapsemsg(repo, isabort):
    'Restore previously stored collapse message'
    try:
        f = repo.vfs("last-message.txt")
        collapsemsg = f.readline().strip()
        f.close()
    except IOError as err:
        if err.errno != errno.ENOENT:
            raise
        if isabort:
            # Oh well, just abort like normal
            collapsemsg = ''
        else:
            raise error.Abort(_('missing .hg/last-message.txt for rebase'))
    return collapsemsg

def clearstatus(repo):
    'Remove the status files'
    _clearrebasesetvisibiliy(repo)
    # Make sure the active transaction won't write the state file
    tr = repo.currenttransaction()
    if tr:
        tr.removefilegenerator('rebasestate')
    repo.vfs.unlinkpath("rebasestate", ignoremissing=True)

def needupdate(repo, state):
    '''check whether we should `update --clean` away from a merge, or if
    somehow the working dir got forcibly updated, e.g. by older hg'''
    parents = [p.rev() for p in repo[None].parents()]

    # Are we in a merge state at all?
    if len(parents) < 2:
        return False

    # We should be standing on the first as-of-yet unrebased commit.
    firstunrebased = min([old for old, new in state.iteritems()
                          if new == nullrev])
    if firstunrebased in parents:
        return True

    return False

def abort(repo, originalwd, dest, state, activebookmark=None):
    '''Restore the repository to its original state.  Additional args:

    activebookmark: the name of the bookmark that should be active after the
        restore'''

    try:
        # If the first commits in the rebased set get skipped during the rebase,
        # their values within the state mapping will be the dest rev id. The
        # dstates list must must not contain the dest rev (issue4896)
        dstates = [s for s in state.values() if s >= 0 and s != dest]
        immutable = [d for d in dstates if not repo[d].mutable()]
        cleanup = True
        if immutable:
            repo.ui.warn(_("warning: can't clean up public changesets %s\n")
                        % ', '.join(str(repo[r]) for r in immutable),
                        hint=_("see 'hg help phases' for details"))
            cleanup = False

        descendants = set()
        if dstates:
            descendants = set(repo.changelog.descendants(dstates))
        if descendants - set(dstates):
            repo.ui.warn(_("warning: new changesets detected on destination "
                           "branch, can't strip\n"))
            cleanup = False

        if cleanup:
            shouldupdate = False
            rebased = filter(lambda x: x >= 0 and x != dest, state.values())
            if rebased:
                strippoints = [
                        c.node() for c in repo.set('roots(%ld)', rebased)]

            updateifonnodes = set(rebased)
            updateifonnodes.add(dest)
            updateifonnodes.add(originalwd)
            shouldupdate = repo['.'].rev() in updateifonnodes

            # Update away from the rebase if necessary
            if shouldupdate or needupdate(repo, state):
                mergemod.update(repo, originalwd, False, True)

            # Strip from the first rebased revision
            if rebased:
                # no backup of rebased cset versions needed
                repair.strip(repo.ui, repo, strippoints)

        if activebookmark and activebookmark in repo._bookmarks:
            bookmarks.activate(repo, activebookmark)

    finally:
        clearstatus(repo)
        clearcollapsemsg(repo)
        repo.ui.warn(_('rebase aborted\n'))
    return 0

def buildstate(repo, dest, rebaseset, collapse, obsoletenotrebased):
    '''Define which revisions are going to be rebased and where

    repo: repo
    dest: context
    rebaseset: set of rev
    '''
    originalwd = repo['.'].rev()
    _setrebasesetvisibility(repo, set(rebaseset) | {originalwd})

    # This check isn't strictly necessary, since mq detects commits over an
    # applied patch. But it prevents messing up the working directory when
    # a partially completed rebase is blocked by mq.
    if 'qtip' in repo.tags() and (dest.node() in
                            [s.node for s in repo.mq.applied]):
        raise error.Abort(_('cannot rebase onto an applied mq patch'))

    roots = list(repo.set('roots(%ld)', rebaseset))
    if not roots:
        raise error.Abort(_('no matching revisions'))
    roots.sort()
    state = dict.fromkeys(rebaseset, revtodo)
    detachset = set()
    emptyrebase = True
    for root in roots:
        commonbase = root.ancestor(dest)
        if commonbase == root:
            raise error.Abort(_('source is ancestor of destination'))
        if commonbase == dest:
            wctx = repo[None]
            if dest == wctx.p1():
                # when rebasing to '.', it will use the current wd branch name
                samebranch = root.branch() == wctx.branch()
            else:
                samebranch = root.branch() == dest.branch()
            if not collapse and samebranch and dest in root.parents():
                # mark the revision as done by setting its new revision
                # equal to its old (current) revisions
                state[root.rev()] = root.rev()
                repo.ui.debug('source is a child of destination\n')
                continue

        emptyrebase = False
        repo.ui.debug('rebase onto %s starting from %s\n' % (dest, root))
        # Rebase tries to turn <dest> into a parent of <root> while
        # preserving the number of parents of rebased changesets:
        #
        # - A changeset with a single parent will always be rebased as a
        #   changeset with a single parent.
        #
        # - A merge will be rebased as merge unless its parents are both
        #   ancestors of <dest> or are themselves in the rebased set and
        #   pruned while rebased.
        #
        # If one parent of <root> is an ancestor of <dest>, the rebased
        # version of this parent will be <dest>. This is always true with
        # --base option.
        #
        # Otherwise, we need to *replace* the original parents with
        # <dest>. This "detaches" the rebased set from its former location
        # and rebases it onto <dest>. Changes introduced by ancestors of
        # <root> not common with <dest> (the detachset, marked as
        # nullmerge) are "removed" from the rebased changesets.
        #
        # - If <root> has a single parent, set it to <dest>.
        #
        # - If <root> is a merge, we cannot decide which parent to
        #   replace, the rebase operation is not clearly defined.
        #
        # The table below sums up this behavior:
        #
        # +------------------+----------------------+-------------------------+
        # |                  |     one parent       |  merge                  |
        # +------------------+----------------------+-------------------------+
        # | parent in        | new parent is <dest> | parents in ::<dest> are |
        # | ::<dest>         |                      | remapped to <dest>      |
        # +------------------+----------------------+-------------------------+
        # | unrelated source | new parent is <dest> | ambiguous, abort        |
        # +------------------+----------------------+-------------------------+
        #
        # The actual abort is handled by `defineparents`
        if len(root.parents()) <= 1:
            # ancestors of <root> not ancestors of <dest>
            detachset.update(repo.changelog.findmissingrevs([commonbase.rev()],
                                                            [root.rev()]))
    if emptyrebase:
        return None
    for rev in sorted(state):
        parents = [p for p in repo.changelog.parentrevs(rev) if p != nullrev]
        # if all parents of this revision are done, then so is this revision
        if parents and all((state.get(p) == p for p in parents)):
            state[rev] = rev
    for r in detachset:
        if r not in state:
            state[r] = nullmerge
    if len(roots) > 1:
        # If we have multiple roots, we may have "hole" in the rebase set.
        # Rebase roots that descend from those "hole" should not be detached as
        # other root are. We use the special `revignored` to inform rebase that
        # the revision should be ignored but that `defineparents` should search
        # a rebase destination that make sense regarding rebased topology.
        rebasedomain = set(repo.revs('%ld::%ld', rebaseset, rebaseset))
        for ignored in set(rebasedomain) - set(rebaseset):
            state[ignored] = revignored
    for r in obsoletenotrebased:
        if obsoletenotrebased[r] is None:
            state[r] = revpruned
        else:
            state[r] = revprecursor
    return originalwd, dest.rev(), state

def clearrebased(ui, repo, dest, state, skipped, collapsedas=None):
    """dispose of rebased revision at the end of the rebase

    If `collapsedas` is not None, the rebase was a collapse whose result if the
    `collapsedas` node."""
    tonode = repo.changelog.node
    # Move bookmark of skipped nodes to destination. This cannot be handled
    # by scmutil.cleanupnodes since it will treat rev as removed (no successor)
    # and move bookmark backwards.
    bmchanges = [(name, tonode(max(adjustdest(repo, rev, dest, state))))
                 for rev in skipped
                 for name in repo.nodebookmarks(tonode(rev))]
    if bmchanges:
        with repo.transaction('rebase') as tr:
            repo._bookmarks.applychanges(repo, tr, bmchanges)
    mapping = {}
    for rev, newrev in sorted(state.items()):
        if newrev >= 0 and newrev != rev:
            if rev in skipped:
                succs = ()
            elif collapsedas is not None:
                succs = (collapsedas,)
            else:
                succs = (tonode(newrev),)
            mapping[tonode(rev)] = succs
    scmutil.cleanupnodes(repo, mapping, 'rebase')

def pullrebase(orig, ui, repo, *args, **opts):
    'Call rebase after pull if the latter has been invoked with --rebase'
    ret = None
    if opts.get('rebase'):
        if ui.configbool('commands', 'rebase.requiredest'):
            msg = _('rebase destination required by configuration')
            hint = _('use hg pull followed by hg rebase -d DEST')
            raise error.Abort(msg, hint=hint)

        with repo.wlock(), repo.lock():
            if opts.get('update'):
                del opts['update']
                ui.debug('--update and --rebase are not compatible, ignoring '
                         'the update flag\n')

            cmdutil.checkunfinished(repo)
            cmdutil.bailifchanged(repo, hint=_('cannot pull with rebase: '
                'please commit or shelve your changes first'))

            revsprepull = len(repo)
            origpostincoming = commands.postincoming
            def _dummy(*args, **kwargs):
                pass
            commands.postincoming = _dummy
            try:
                ret = orig(ui, repo, *args, **opts)
            finally:
                commands.postincoming = origpostincoming
            revspostpull = len(repo)
            if revspostpull > revsprepull:
                # --rev option from pull conflict with rebase own --rev
                # dropping it
                if 'rev' in opts:
                    del opts['rev']
                # positional argument from pull conflicts with rebase's own
                # --source.
                if 'source' in opts:
                    del opts['source']
                # revsprepull is the len of the repo, not revnum of tip.
                destspace = list(repo.changelog.revs(start=revsprepull))
                opts['_destspace'] = destspace
                try:
                    rebase(ui, repo, **opts)
                except error.NoMergeDestAbort:
                    # we can maybe update instead
                    rev, _a, _b = destutil.destupdate(repo)
                    if rev == repo['.'].rev():
                        ui.status(_('nothing to rebase\n'))
                    else:
                        ui.status(_('nothing to rebase - updating instead\n'))
                        # not passing argument to get the bare update behavior
                        # with warning and trumpets
                        commands.update(ui, repo)
    else:
        if opts.get('tool'):
            raise error.Abort(_('--tool can only be used with --rebase'))
        ret = orig(ui, repo, *args, **opts)

    return ret

def _setrebasesetvisibility(repo, revs):
    """store the currently rebased set on the repo object

    This is used by another function to prevent rebased revision to because
    hidden (see issue4504)"""
    repo = repo.unfiltered()
    repo._rebaseset = revs
    # invalidate cache if visibility changes
    hiddens = repo.filteredrevcache.get('visible', set())
    if revs & hiddens:
        repo.invalidatevolatilesets()

def _clearrebasesetvisibiliy(repo):
    """remove rebaseset data from the repo"""
    repo = repo.unfiltered()
    if '_rebaseset' in vars(repo):
        del repo._rebaseset

def _rebasedvisible(orig, repo):
    """ensure rebased revs stay visible (see issue4504)"""
    blockers = orig(repo)
    blockers.update(getattr(repo, '_rebaseset', ()))
    return blockers

def _filterobsoleterevs(repo, revs):
    """returns a set of the obsolete revisions in revs"""
    return set(r for r in revs if repo[r].obsolete())

def _computeobsoletenotrebased(repo, rebaseobsrevs, dest):
    """return a mapping obsolete => successor for all obsolete nodes to be
    rebased that have a successors in the destination

    obsolete => None entries in the mapping indicate nodes with no successor"""
    obsoletenotrebased = {}

    # Build a mapping successor => obsolete nodes for the obsolete
    # nodes to be rebased
    allsuccessors = {}
    cl = repo.changelog
    for r in rebaseobsrevs:
        node = cl.node(r)
        for s in obsutil.allsuccessors(repo.obsstore, [node]):
            try:
                allsuccessors[cl.rev(s)] = cl.rev(node)
            except LookupError:
                pass

    if allsuccessors:
        # Look for successors of obsolete nodes to be rebased among
        # the ancestors of dest
        ancs = cl.ancestors([dest],
                            stoprev=min(allsuccessors),
                            inclusive=True)
        for s in allsuccessors:
            if s in ancs:
                obsoletenotrebased[allsuccessors[s]] = s
            elif (s == allsuccessors[s] and
                  allsuccessors.values().count(s) == 1):
                # plain prune
                obsoletenotrebased[s] = None

    return obsoletenotrebased

def summaryhook(ui, repo):
    if not repo.vfs.exists('rebasestate'):
        return
    try:
        rbsrt = rebaseruntime(repo, ui, {})
        rbsrt.restorestatus()
        state = rbsrt.state
    except error.RepoLookupError:
        # i18n: column positioning for "hg summary"
        msg = _('rebase: (use "hg rebase --abort" to clear broken state)\n')
        ui.write(msg)
        return
    numrebased = len([i for i in state.itervalues() if i >= 0])
    # i18n: column positioning for "hg summary"
    ui.write(_('rebase: %s, %s (rebase --continue)\n') %
             (ui.label(_('%d rebased'), 'rebase.rebased') % numrebased,
              ui.label(_('%d remaining'), 'rebase.remaining') %
              (len(state) - numrebased)))

def uisetup(ui):
    #Replace pull with a decorator to provide --rebase option
    entry = extensions.wrapcommand(commands.table, 'pull', pullrebase)
    entry[1].append(('', 'rebase', None,
                     _("rebase working directory to branch head")))
    entry[1].append(('t', 'tool', '',
                     _("specify merge tool for rebase")))
    cmdutil.summaryhooks.add('rebase', summaryhook)
    cmdutil.unfinishedstates.append(
        ['rebasestate', False, False, _('rebase in progress'),
         _("use 'hg rebase --continue' or 'hg rebase --abort'")])
    cmdutil.afterresolvedstates.append(
        ['rebasestate', _('hg rebase --continue')])
    # ensure rebased rev are not hidden
    extensions.wrapfunction(repoview, 'pinnedrevs', _rebasedvisible)
