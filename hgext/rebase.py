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
    revset,
    revsetlang,
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
revtodostr = '-1'

# legacy revstates no longer needed in current code
# -2: nullmerge, -3: revignored, -4: revprecursor, -5: revpruned
legacystates = {'-2', '-3', '-4', '-5'}

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

def _ctxdesc(ctx):
    """short description for a context"""
    desc = '%d:%s "%s"' % (ctx.rev(), ctx,
                           ctx.description().split('\n', 1)[0])
    repo = ctx.repo()
    names = []
    for nsname, ns in repo.names.iteritems():
        if nsname == 'branches':
            continue
        names.extend(ns.names(repo, ctx.node()))
    if names:
        desc += ' (%s)' % ' '.join(names)
    return desc

class rebaseruntime(object):
    """This class is a container for rebase runtime state"""
    def __init__(self, repo, ui, opts=None):
        if opts is None:
            opts = {}

        # prepared: whether we have rebasestate prepared or not. Currently it
        # decides whether "self.repo" is unfiltered or not.
        # The rebasestate has explicit hash to hash instructions not depending
        # on visibility. If rebasestate exists (in-memory or on-disk), use
        # unfiltered repo to avoid visibility issues.
        # Before knowing rebasestate (i.e. when starting a new rebase (not
        # --continue or --abort)), the original repo should be used so
        # visibility-dependent revsets are correct.
        self.prepared = False
        self._repo = repo

        self.ui = ui
        self.opts = opts
        self.originalwd = None
        self.external = nullrev
        # Mapping between the old revision id and either what is the new rebased
        # revision or what needs to be done with the old revision. The state
        # dict will be what contains most of the rebase progress state.
        self.state = {}
        self.activebookmark = None
        self.destmap = {}
        self.skipped = set()

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

    @property
    def repo(self):
        if self.prepared:
            return self._repo.unfiltered()
        else:
            return self._repo

    def storestatus(self, tr=None):
        """Store the current status to allow recovery"""
        if tr:
            tr.addfilegenerator('rebasestate', ('rebasestate',),
                                self._writestatus, location='plain')
        else:
            with self.repo.vfs("rebasestate", "w") as f:
                self._writestatus(f)

    def _writestatus(self, f):
        repo = self.repo
        assert repo.filtername is None
        f.write(repo[self.originalwd].hex() + '\n')
        # was "dest". we now write dest per src root below.
        f.write('\n')
        f.write(repo[self.external].hex() + '\n')
        f.write('%d\n' % int(self.collapsef))
        f.write('%d\n' % int(self.keepf))
        f.write('%d\n' % int(self.keepbranchesf))
        f.write('%s\n' % (self.activebookmark or ''))
        destmap = self.destmap
        for d, v in self.state.iteritems():
            oldrev = repo[d].hex()
            if v >= 0:
                newrev = repo[v].hex()
            else:
                newrev = v
            destnode = repo[destmap[d]].hex()
            f.write("%s:%s:%s\n" % (oldrev, newrev, destnode))
        repo.ui.debug('rebase status stored\n')

    def restorestatus(self):
        """Restore a previously stored status"""
        self.prepared = True
        repo = self.repo
        assert repo.filtername is None
        keepbranches = None
        legacydest = None
        collapse = False
        external = nullrev
        activebookmark = None
        state = {}
        destmap = {}

        try:
            f = repo.vfs("rebasestate")
            for i, l in enumerate(f.read().splitlines()):
                if i == 0:
                    originalwd = repo[l].rev()
                elif i == 1:
                    # this line should be empty in newer version. but legacy
                    # clients may still use it
                    if l:
                        legacydest = repo[l].rev()
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
                    args = l.split(':')
                    oldrev = args[0]
                    newrev = args[1]
                    if newrev in legacystates:
                        continue
                    if len(args) > 2:
                        destnode = args[2]
                    else:
                        destnode = legacydest
                    destmap[repo[oldrev].rev()] = repo[destnode].rev()
                    if newrev in (nullid, revtodostr):
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
            seen = set(destmap.values())
            for old, new in sorted(state.items()):
                if new != revtodo and new in seen:
                    skipped.add(old)
                seen.add(new)
        repo.ui.debug('computed skipped revs: %s\n' %
                        (' '.join(str(r) for r in sorted(skipped)) or None))
        repo.ui.debug('rebase status resumed\n')

        self.originalwd = originalwd
        self.destmap = destmap
        self.state = state
        self.skipped = skipped
        self.collapsef = collapse
        self.keepf = keep
        self.keepbranchesf = keepbranches
        self.external = external
        self.activebookmark = activebookmark

    def _handleskippingobsolete(self, obsoleterevs, destmap):
        """Compute structures necessary for skipping obsolete revisions

        obsoleterevs:   iterable of all obsolete revisions in rebaseset
        destmap:        {srcrev: destrev} destination revisions
        """
        self.obsoletenotrebased = {}
        if not self.ui.configbool('experimental', 'rebaseskipobsolete'):
            return
        obsoleteset = set(obsoleterevs)
        self.obsoletenotrebased = _computeobsoletenotrebased(self.repo,
                                    obsoleteset, destmap)
        skippedset = set(self.obsoletenotrebased)
        _checkobsrebase(self.repo, self.ui, obsoleteset, skippedset)

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
            return abort(self.repo, self.originalwd, self.destmap,
                         self.state, activebookmark=self.activebookmark)

    def _preparenewrebase(self, destmap):
        if not destmap:
            return _nothingtorebase()

        rebaseset = destmap.keys()
        allowunstable = obsolete.isenabled(self.repo, obsolete.allowunstableopt)
        if (not (self.keepf or allowunstable)
              and self.repo.revs('first(children(%ld) - %ld)',
                                 rebaseset, rebaseset)):
            raise error.Abort(
                _("can't remove original changesets with"
                  " unrebased descendants"),
                hint=_('use --keep to keep original changesets'))

        result = buildstate(self.repo, destmap, self.collapsef)

        if not result:
            # Empty state built, nothing to rebase
            self.ui.status(_('nothing to rebase\n'))
            return _nothingtorebase()

        for root in self.repo.set('roots(%ld)', rebaseset):
            if not self.keepf and not root.mutable():
                raise error.Abort(_("can't rebase public changeset %s")
                                  % root,
                                  hint=_("see 'hg help phases' for details"))

        (self.originalwd, self.destmap, self.state) = result
        if self.collapsef:
            dests = set(self.destmap.values())
            if len(dests) != 1:
                raise error.Abort(
                    _('--collapse does not work with multiple destinations'))
            destrev = next(iter(dests))
            destancestors = self.repo.changelog.ancestors([destrev],
                                                          inclusive=True)
            self.external = externalparent(self.repo, self.state, destancestors)

        for destrev in sorted(set(destmap.values())):
            dest = self.repo[destrev]
            if dest.closesbranch() and not self.keepbranchesf:
                self.ui.status(_('reopening closed branch head %s\n') % dest)

        self.prepared = True

    def _performrebase(self, tr):
        repo, ui = self.repo, self.ui
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

        # Calculate self.obsoletenotrebased
        obsrevs = _filterobsoleterevs(self.repo, self.state)
        self._handleskippingobsolete(obsrevs, self.destmap)

        # Keep track of the active bookmarks in order to reset them later
        self.activebookmark = self.activebookmark or repo._activebookmark
        if self.activebookmark:
            bookmarks.deactivate(repo)

        # Store the state before we begin so users can run 'hg rebase --abort'
        # if we fail before the transaction closes.
        self.storestatus()

        cands = [k for k, v in self.state.iteritems() if v == revtodo]
        total = len(cands)
        pos = 0
        for subset in sortsource(self.destmap):
            pos = self._performrebasesubset(tr, subset, pos, total)
        ui.progress(_('rebasing'), None)
        ui.note(_('rebase merging completed\n'))

    def _performrebasesubset(self, tr, subset, pos, total):
        repo, ui, opts = self.repo, self.ui, self.opts
        sortedrevs = repo.revs('sort(%ld, -topo)', subset)
        for rev in sortedrevs:
            dest = self.destmap[rev]
            ctx = repo[rev]
            desc = _ctxdesc(ctx)
            if self.state[rev] == rev:
                ui.status(_('already rebased %s\n') % desc)
            elif rev in self.obsoletenotrebased:
                succ = self.obsoletenotrebased[rev]
                if succ is None:
                    msg = _('note: not rebasing %s, it has no '
                            'successor\n') % desc
                else:
                    succdesc = _ctxdesc(repo[succ])
                    msg = (_('note: not rebasing %s, already in '
                             'destination as %s\n') % (desc, succdesc))
                repo.ui.status(msg)
                # Make clearrebased aware state[rev] is not a true successor
                self.skipped.add(rev)
                # Record rev as moved to its desired destination in self.state.
                # This helps bookmark and working parent movement.
                dest = max(adjustdest(repo, rev, self.destmap, self.state,
                                      self.skipped))
                self.state[rev] = dest
            elif self.state[rev] == revtodo:
                pos += 1
                ui.status(_('rebasing %s\n') % desc)
                ui.progress(_("rebasing"), pos, ("%d:%s" % (rev, ctx)),
                            _('changesets'), total)
                p1, p2, base = defineparents(repo, rev, self.destmap,
                                             self.state, self.skipped,
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
                                           self.collapsef, dest)
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
            else:
                ui.status(_('already rebased %s as %s\n') %
                          (desc, repo[self.state[rev]]))
        return pos

    def _finishrebase(self):
        repo, ui, opts = self.repo, self.ui, self.opts
        fm = ui.formatter('rebase', opts)
        fm.startitem()
        if self.collapsef and not self.keepopen:
            p1, p2, _base = defineparents(repo, min(self.state), self.destmap,
                                          self.state, self.skipped,
                                          self.obsoletenotrebased)
            editopt = opts.get('edit')
            editform = 'rebase.collapse'
            if self.collapsemsg:
                commitmsg = self.collapsemsg
            else:
                commitmsg = 'Collapsed revision'
                for rebased in sorted(self.state):
                    if rebased not in self.skipped:
                        commitmsg += '\n* %s' % repo[rebased].description()
                editopt = True
            editor = cmdutil.getcommiteditor(edit=editopt, editform=editform)
            revtoreuse = max(self.state)

            dsguard = None
            if ui.configbool('rebase', 'singletransaction'):
                dsguard = dirstateguard.dirstateguard(repo, 'rebase')
            with util.acceptintervention(dsguard):
                newnode = concludenode(repo, revtoreuse, p1, self.external,
                                       commitmsg=commitmsg,
                                       extrafn=_makeextrafn(self.extrafns),
                                       editor=editor,
                                       keepbranches=self.keepbranchesf,
                                       date=self.date)
            if newnode is not None:
                newrev = repo[newnode].rev()
                for oldrev in self.state.iterkeys():
                    self.state[oldrev] = newrev

        if 'qtip' in repo.tags():
            updatemq(repo, self.state, self.skipped, **opts)

        # restore original working directory
        # (we do this before stripping)
        newwd = self.state.get(self.originalwd, self.originalwd)
        if newwd < 0:
            # original directory is a parent of rebase set root or ignored
            newwd = self.originalwd
        if newwd not in [c.rev() for c in repo[None].parents()]:
            ui.note(_("update back to initial working directory parent\n"))
            hg.updaterepo(repo, newwd, False)

        collapsedas = None
        if not self.keepf:
            if self.collapsef:
                collapsedas = newnode
        clearrebased(ui, repo, self.destmap, self.state, self.skipped,
                     collapsedas, self.keepf, fm=fm)

        clearstatus(repo)
        clearcollapsemsg(repo)

        ui.note(_("rebase completed\n"))
        util.unlinkpath(repo.sjoin('undo'), ignoremissing=True)
        if self.skipped:
            skippedlen = len(self.skipped)
            ui.note(_("%d revisions have been skipped\n") % skippedlen)
        fm.end()

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
            destmap = _definedestmap(ui, repo, destf, srcf, basef, revf,
                                     destspace=destspace)
            retcode = rbsrt._preparenewrebase(destmap)
            if retcode is not None:
                return retcode

        tr = None
        dsguard = None

        singletr = ui.configbool('rebase', 'singletransaction')
        if singletr:
            tr = repo.transaction('rebase')
        with util.acceptintervention(tr):
            if singletr:
                dsguard = dirstateguard.dirstateguard(repo, 'rebase')
            with util.acceptintervention(dsguard):
                rbsrt._performrebase(tr)

        rbsrt._finishrebase()

def _definedestmap(ui, repo, destf=None, srcf=None, basef=None, revf=None,
                   destspace=None):
    """use revisions argument to define destmap {srcrev: destrev}"""
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

    dest = None

    if revf:
        rebaseset = scmutil.revrange(repo, revf)
        if not rebaseset:
            ui.status(_('empty "rev" revision set - nothing to rebase\n'))
            return None
    elif srcf:
        src = scmutil.revrange(repo, [srcf])
        if not src:
            ui.status(_('empty "source" revision set - nothing to rebase\n'))
            return None
        rebaseset = repo.revs('(%ld)::', src)
        assert rebaseset
    else:
        base = scmutil.revrange(repo, [basef or '.'])
        if not base:
            ui.status(_('empty "base" revision set - '
                        "can't compute rebase set\n"))
            return None
        if destf:
            # --base does not support multiple destinations
            dest = scmutil.revsingle(repo, destf)
        else:
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
            return None

    if not destf:
        dest = repo[_destrebase(repo, rebaseset, destspace=destspace)]
        destf = str(dest)

    allsrc = revsetlang.formatspec('%ld', rebaseset)
    alias = {'ALLSRC': allsrc}

    if dest is None:
        try:
            # fast path: try to resolve dest without SRC alias
            dest = scmutil.revsingle(repo, destf, localalias=alias)
        except error.RepoLookupError:
            if not ui.configbool('experimental', 'rebase.multidest'):
                raise
            # multi-dest path: resolve dest for each SRC separately
            destmap = {}
            for r in rebaseset:
                alias['SRC'] = revsetlang.formatspec('%d', r)
                # use repo.anyrevs instead of scmutil.revsingle because we
                # don't want to abort if destset is empty.
                destset = repo.anyrevs([destf], user=True, localalias=alias)
                size = len(destset)
                if size == 1:
                    destmap[r] = destset.first()
                elif size == 0:
                    ui.note(_('skipping %s - empty destination\n') % repo[r])
                else:
                    raise error.Abort(_('rebase destination for %s is not '
                                        'unique') % repo[r])

    if dest is not None:
        # single-dest case: assign dest to each rev in rebaseset
        destrev = dest.rev()
        destmap = {r: destrev for r in rebaseset} # {srcrev: destrev}

    if not destmap:
        ui.status(_('nothing to rebase - empty destination\n'))
        return None

    return destmap

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
    dsguard = util.nullcontextmanager()
    if not repo.ui.configbool('rebase', 'singletransaction'):
        dsguard = dirstateguard.dirstateguard(repo, 'rebase')
    with dsguard:
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
        return newnode

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
    wctx = repo[None]
    stats = mergemod.update(repo, rev, True, True, base, collapse,
                            labels=['dest', 'source'])
    if collapse:
        copies.duplicatecopies(repo, wctx, rev, dest)
    else:
        # If we're not using --collapse, we need to
        # duplicate copies between the revision we're
        # rebasing and its first parent, but *not*
        # duplicate any copies that have already been
        # performed in the destination.
        p1rev = repo[rev].p1().rev()
        copies.duplicatecopies(repo, wctx, rev, p1rev, skiprev=dest)
    return stats

def adjustdest(repo, rev, destmap, state, skipped):
    """adjust rebase destination given the current rebase state

    rev is what is being rebased. Return a list of two revs, which are the
    adjusted destinations for rev's p1 and p2, respectively. If a parent is
    nullrev, return dest without adjustment for it.

    For example, when doing rebasing B+E to F, C to G, rebase will first move B
    to B1, and E's destination will be adjusted from F to B1.

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
        | C <- rebased as C', different destination
        | |
        | B <- rebased as B1     C'
        |/                       |
        A                        G <- destination of C, different

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

    Besides, adjust dest according to existing rebase information. For example,

      B C D    B needs to be rebased on top of C, C needs to be rebased on top
       \|/     of D. We will rebase C first.
        A

          C'   After rebasing C, when considering B's destination, use C'
          |    instead of the original C.
      B   D
       \ /
        A
    """
    # pick already rebased revs with same dest from state as interesting source
    dest = destmap[rev]
    source = [s for s, d in state.items()
              if d > 0 and destmap[s] == dest and s not in skipped]

    result = []
    for prev in repo.changelog.parentrevs(rev):
        adjusted = dest
        if prev != nullrev:
            candidate = repo.revs('max(%ld and (::%d))', source, prev).first()
            if candidate is not None:
                adjusted = state[candidate]
        if adjusted == dest and dest in state:
            adjusted = state[dest]
            if adjusted == revtodo:
                # sortsource should produce an order that makes this impossible
                raise error.ProgrammingError(
                    'rev %d should be rebased already at this time' % dest)
        result.append(adjusted)
    return result

def _checkobsrebase(repo, ui, rebaseobsrevs, rebaseobsskipped):
    """
    Abort if rebase will create divergence or rebase is noop because of markers

    `rebaseobsrevs`: set of obsolete revision in source
    `rebaseobsskipped`: set of revisions from source skipped because they have
    successors in destination
    """
    # Obsolete node with successors not in dest leads to divergence
    divergenceok = ui.configbool('experimental',
                                 'evolution.allowdivergence')
    divergencebasecandidates = rebaseobsrevs - rebaseobsskipped

    if divergencebasecandidates and not divergenceok:
        divhashes = (str(repo[r])
                     for r in divergencebasecandidates)
        msg = _("this rebase will cause "
                "divergences from: %s")
        h = _("to force the rebase please set "
              "experimental.evolution.allowdivergence=True")
        raise error.Abort(msg % (",".join(divhashes),), hint=h)

def successorrevs(unfi, rev):
    """yield revision numbers for successors of rev"""
    assert unfi.filtername is None
    nodemap = unfi.changelog.nodemap
    for s in obsutil.allsuccessors(unfi.obsstore, [unfi[rev].node()]):
        if s in nodemap:
            yield nodemap[s]

def defineparents(repo, rev, destmap, state, skipped, obsskipped):
    """Return new parents and optionally a merge base for rev being rebased

    The destination specified by "dest" cannot always be used directly because
    previously rebase result could affect destination. For example,

          D E    rebase -r C+D+E -d B
          |/     C will be rebased to C'
        B C      D's new destination will be C' instead of B
        |/       E's new destination will be C' instead of B
        A

    The new parents of a merge is slightly more complicated. See the comment
    block below.
    """
    # use unfiltered changelog since successorrevs may return filtered nodes
    assert repo.filtername is None
    cl = repo.changelog
    def isancestor(a, b):
        # take revision numbers instead of nodes
        if a == b:
            return True
        elif a > b:
            return False
        return cl.isancestor(cl.node(a), cl.node(b))

    dest = destmap[rev]
    oldps = repo.changelog.parentrevs(rev) # old parents
    newps = [nullrev, nullrev] # new parents
    dests = adjustdest(repo, rev, destmap, state, skipped)
    bases = list(oldps) # merge base candidates, initially just old parents

    if all(r == nullrev for r in oldps[1:]):
        # For non-merge changeset, just move p to adjusted dest as requested.
        newps[0] = dests[0]
    else:
        # For merge changeset, if we move p to dests[i] unconditionally, both
        # parents may change and the end result looks like "the merge loses a
        # parent", which is a surprise. This is a limit because "--dest" only
        # accepts one dest per src.
        #
        # Therefore, only move p with reasonable conditions (in this order):
        #   1. use dest, if dest is a descendent of (p or one of p's successors)
        #   2. use p's rebased result, if p is rebased (state[p] > 0)
        #
        # Comparing with adjustdest, the logic here does some additional work:
        #   1. decide which parents will not be moved towards dest
        #   2. if the above decision is "no", should a parent still be moved
        #      because it was rebased?
        #
        # For example:
        #
        #     C    # "rebase -r C -d D" is an error since none of the parents
        #    /|    # can be moved. "rebase -r B+C -d D" will move C's parent
        #   A B D  # B (using rule "2."), since B will be rebased.
        #
        # The loop tries to be not rely on the fact that a Mercurial node has
        # at most 2 parents.
        for i, p in enumerate(oldps):
            np = p # new parent
            if any(isancestor(x, dests[i]) for x in successorrevs(repo, p)):
                np = dests[i]
            elif p in state and state[p] > 0:
                np = state[p]

            # "bases" only record "special" merge bases that cannot be
            # calculated from changelog DAG (i.e. isancestor(p, np) is False).
            # For example:
            #
            #   B'   # rebase -s B -d D, when B was rebased to B'. dest for C
            #   | C  # is B', but merge base for C is B, instead of
            #   D |  # changelog.ancestor(C, B') == A. If changelog DAG and
            #   | B  # "state" edges are merged (so there will be an edge from
            #   |/   # B to B'), the merge base is still ancestor(C, B') in
            #   A    # the merged graph.
            #
            # Also see https://bz.mercurial-scm.org/show_bug.cgi?id=1950#c8
            # which uses "virtual null merge" to explain this situation.
            if isancestor(p, np):
                bases[i] = nullrev

            # If one parent becomes an ancestor of the other, drop the ancestor
            for j, x in enumerate(newps[:i]):
                if x == nullrev:
                    continue
                if isancestor(np, x): # CASE-1
                    np = nullrev
                elif isancestor(x, np): # CASE-2
                    newps[j] = np
                    np = nullrev
                    # New parents forming an ancestor relationship does not
                    # mean the old parents have a similar relationship. Do not
                    # set bases[x] to nullrev.
                    bases[j], bases[i] = bases[i], bases[j]

            newps[i] = np

        # "rebasenode" updates to new p1, and the old p1 will be used as merge
        # base. If only p2 changes, merging using unchanged p1 as merge base is
        # suboptimal. Therefore swap parents to make the merge sane.
        if newps[1] != nullrev and oldps[0] == newps[0]:
            assert len(newps) == 2 and len(oldps) == 2
            newps.reverse()
            bases.reverse()

        # No parent change might be an error because we fail to make rev a
        # descendent of requested dest. This can happen, for example:
        #
        #     C    # rebase -r C -d D
        #    /|    # None of A and B will be changed to D and rebase fails.
        #   A B D
        if set(newps) == set(oldps) and dest not in newps:
            raise error.Abort(_('cannot rebase %d:%s without '
                                'moving at least one of its parents')
                              % (rev, repo[rev]))

    # Source should not be ancestor of dest. The check here guarantees it's
    # impossible. With multi-dest, the initial check does not cover complex
    # cases since we don't have abstractions to dry-run rebase cheaply.
    if any(p != nullrev and isancestor(rev, p) for p in newps):
        raise error.Abort(_('source is ancestor of destination'))

    # "rebasenode" updates to new p1, use the corresponding merge base.
    if bases[0] != nullrev:
        base = bases[0]
    else:
        base = None

    # Check if the merge will contain unwanted changes. That may happen if
    # there are multiple special (non-changelog ancestor) merge bases, which
    # cannot be handled well by the 3-way merge algorithm. For example:
    #
    #     F
    #    /|
    #   D E  # "rebase -r D+E+F -d Z", when rebasing F, if "D" was chosen
    #   | |  # as merge base, the difference between D and F will include
    #   B C  # C, so the rebased F will contain C surprisingly. If "E" was
    #   |/   #  chosen, the rebased F will contain B.
    #   A Z
    #
    # But our merge base candidates (D and E in above case) could still be
    # better than the default (ancestor(F, Z) == null). Therefore still
    # pick one (so choose p1 above).
    if sum(1 for b in bases if b != nullrev) > 1:
        unwanted = [None, None] # unwanted[i]: unwanted revs if choose bases[i]
        for i, base in enumerate(bases):
            if base == nullrev:
                continue
            # Revisions in the side (not chosen as merge base) branch that
            # might contain "surprising" contents
            siderevs = list(repo.revs('((%ld-%d) %% (%d+%d))',
                                      bases, base, base, dest))

            # If those revisions are covered by rebaseset, the result is good.
            # A merge in rebaseset would be considered to cover its ancestors.
            if siderevs:
                rebaseset = [r for r, d in state.items()
                             if d > 0 and r not in obsskipped]
                merges = [r for r in rebaseset
                          if cl.parentrevs(r)[1] != nullrev]
                unwanted[i] = list(repo.revs('%ld - (::%ld) - %ld',
                                             siderevs, merges, rebaseset))

        # Choose a merge base that has a minimal number of unwanted revs.
        l, i = min((len(revs), i)
                   for i, revs in enumerate(unwanted) if revs is not None)
        base = bases[i]

        # newps[0] should match merge base if possible. Currently, if newps[i]
        # is nullrev, the only case is newps[i] and newps[j] (j < i), one is
        # the other's ancestor. In that case, it's fine to not swap newps here.
        # (see CASE-1 and CASE-2 above)
        if i != 0 and newps[i] != nullrev:
            newps[0], newps[i] = newps[i], newps[0]

        # The merge will include unwanted revisions. Abort now. Revisit this if
        # we have a more advanced merge algorithm that handles multiple bases.
        if l > 0:
            unwanteddesc = _(' or ').join(
                (', '.join('%d:%s' % (r, repo[r]) for r in revs)
                 for revs in unwanted if revs is not None))
            raise error.Abort(
                _('rebasing %d:%s will include unwanted changes from %s')
                % (rev, repo[rev], unwanteddesc))

    repo.ui.debug(" future parents are %d and %d\n" % tuple(newps))

    return newps[0], newps[1], base

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

def abort(repo, originalwd, destmap, state, activebookmark=None):
    '''Restore the repository to its original state.  Additional args:

    activebookmark: the name of the bookmark that should be active after the
        restore'''

    try:
        # If the first commits in the rebased set get skipped during the rebase,
        # their values within the state mapping will be the dest rev id. The
        # dstates list must must not contain the dest rev (issue4896)
        dstates = [s for r, s in state.items() if s >= 0 and s != destmap[r]]
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
            rebased = [s for r, s in state.items()
                       if s >= 0 and s != destmap[r]]
            if rebased:
                strippoints = [
                        c.node() for c in repo.set('roots(%ld)', rebased)]

            updateifonnodes = set(rebased)
            updateifonnodes.update(destmap.values())
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

def sortsource(destmap):
    """yield source revisions in an order that we only rebase things once

    If source and destination overlaps, we should filter out revisions
    depending on other revisions which hasn't been rebased yet.

    Yield a sorted list of revisions each time.

    For example, when rebasing A to B, B to C. This function yields [B], then
    [A], indicating B needs to be rebased first.

    Raise if there is a cycle so the rebase is impossible.
    """
    srcset = set(destmap)
    while srcset:
        srclist = sorted(srcset)
        result = []
        for r in srclist:
            if destmap[r] not in srcset:
                result.append(r)
        if not result:
            raise error.Abort(_('source and destination form a cycle'))
        srcset -= set(result)
        yield result

def buildstate(repo, destmap, collapse):
    '''Define which revisions are going to be rebased and where

    repo: repo
    destmap: {srcrev: destrev}
    '''
    rebaseset = destmap.keys()
    originalwd = repo['.'].rev()

    # This check isn't strictly necessary, since mq detects commits over an
    # applied patch. But it prevents messing up the working directory when
    # a partially completed rebase is blocked by mq.
    if 'qtip' in repo.tags():
        mqapplied = set(repo[s.node].rev() for s in repo.mq.applied)
        if set(destmap.values()) & mqapplied:
            raise error.Abort(_('cannot rebase onto an applied mq patch'))

    # Get "cycle" error early by exhausting the generator.
    sortedsrc = list(sortsource(destmap)) # a list of sorted revs
    if not sortedsrc:
        raise error.Abort(_('no matching revisions'))

    # Only check the first batch of revisions to rebase not depending on other
    # rebaseset. This means "source is ancestor of destination" for the second
    # (and following) batches of revisions are not checked here. We rely on
    # "defineparents" to do that check.
    roots = list(repo.set('roots(%ld)', sortedsrc[0]))
    if not roots:
        raise error.Abort(_('no matching revisions'))
    roots.sort()
    state = dict.fromkeys(rebaseset, revtodo)
    emptyrebase = (len(sortedsrc) == 1)
    for root in roots:
        dest = repo[destmap[root.rev()]]
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
    if emptyrebase:
        return None
    for rev in sorted(state):
        parents = [p for p in repo.changelog.parentrevs(rev) if p != nullrev]
        # if all parents of this revision are done, then so is this revision
        if parents and all((state.get(p) == p for p in parents)):
            state[rev] = rev
    return originalwd, destmap, state

def clearrebased(ui, repo, destmap, state, skipped, collapsedas=None,
                 keepf=False, fm=None):
    """dispose of rebased revision at the end of the rebase

    If `collapsedas` is not None, the rebase was a collapse whose result if the
    `collapsedas` node.

    If `keepf` is not True, the rebase has --keep set and no nodes should be
    removed (but bookmarks still need to be moved).
    """
    tonode = repo.changelog.node
    replacements = {}
    moves = {}
    for rev, newrev in sorted(state.items()):
        if newrev >= 0 and newrev != rev:
            oldnode = tonode(rev)
            newnode = collapsedas or tonode(newrev)
            moves[oldnode] = newnode
            if not keepf:
                if rev in skipped:
                    succs = ()
                else:
                    succs = (newnode,)
                replacements[oldnode] = succs
    scmutil.cleanupnodes(repo, replacements, 'rebase', moves)
    if fm:
        nodechanges = {hex(oldn): [hex(n) for n in newn]
                       for oldn, newn in replacements.iteritems()}
        fm.data(nodechanges=nodechanges)

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

def _filterobsoleterevs(repo, revs):
    """returns a set of the obsolete revisions in revs"""
    return set(r for r in revs if repo[r].obsolete())

def _computeobsoletenotrebased(repo, rebaseobsrevs, destmap):
    """return a mapping obsolete => successor for all obsolete nodes to be
    rebased that have a successors in the destination

    obsolete => None entries in the mapping indicate nodes with no successor"""
    obsoletenotrebased = {}

    assert repo.filtername is None
    cl = repo.changelog
    nodemap = cl.nodemap
    for srcrev in rebaseobsrevs:
        srcnode = cl.node(srcrev)
        destnode = cl.node(destmap[srcrev])
        # XXX: more advanced APIs are required to handle split correctly
        successors = list(obsutil.allsuccessors(repo.obsstore, [srcnode]))
        if len(successors) == 1:
            # obsutil.allsuccessors includes node itself. When the list only
            # contains one element, it means there are no successors.
            obsoletenotrebased[srcrev] = None
        else:
            for succnode in successors:
                if succnode == srcnode or succnode not in nodemap:
                    continue
                if cl.isancestor(succnode, destnode):
                    obsoletenotrebased[srcrev] = nodemap[succnode]
                    break

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
