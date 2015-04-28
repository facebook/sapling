# histedit.py - interactive history editing for mercurial
#
# Copyright 2009 Augie Fackler <raf@durin42.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""interactive history editing

With this extension installed, Mercurial gains one new command: histedit. Usage
is as follows, assuming the following history::

 @  3[tip]   7c2fd3b9020c   2009-04-27 18:04 -0500   durin42
 |    Add delta
 |
 o  2   030b686bedc4   2009-04-27 18:04 -0500   durin42
 |    Add gamma
 |
 o  1   c561b4e977df   2009-04-27 18:04 -0500   durin42
 |    Add beta
 |
 o  0   d8d2fcd0e319   2009-04-27 18:04 -0500   durin42
      Add alpha

If you were to run ``hg histedit c561b4e977df``, you would see the following
file open in your editor::

 pick c561b4e977df Add beta
 pick 030b686bedc4 Add gamma
 pick 7c2fd3b9020c Add delta

 # Edit history between c561b4e977df and 7c2fd3b9020c
 #
 # Commits are listed from least to most recent
 #
 # Commands:
 #  p, pick = use commit
 #  e, edit = use commit, but stop for amending
 #  f, fold = use commit, but combine it with the one above
 #  r, roll = like fold, but discard this commit's description
 #  d, drop = remove commit from history
 #  m, mess = edit message without changing commit content
 #

In this file, lines beginning with ``#`` are ignored. You must specify a rule
for each revision in your history. For example, if you had meant to add gamma
before beta, and then wanted to add delta in the same revision as beta, you
would reorganize the file to look like this::

 pick 030b686bedc4 Add gamma
 pick c561b4e977df Add beta
 fold 7c2fd3b9020c Add delta

 # Edit history between c561b4e977df and 7c2fd3b9020c
 #
 # Commits are listed from least to most recent
 #
 # Commands:
 #  p, pick = use commit
 #  e, edit = use commit, but stop for amending
 #  f, fold = use commit, but combine it with the one above
 #  r, roll = like fold, but discard this commit's description
 #  d, drop = remove commit from history
 #  m, mess = edit message without changing commit content
 #

At which point you close the editor and ``histedit`` starts working. When you
specify a ``fold`` operation, ``histedit`` will open an editor when it folds
those revisions together, offering you a chance to clean up the commit message::

 Add beta
 ***
 Add delta

Edit the commit message to your liking, then close the editor. For
this example, let's assume that the commit message was changed to
``Add beta and delta.`` After histedit has run and had a chance to
remove any old or temporary revisions it needed, the history looks
like this::

 @  2[tip]   989b4d060121   2009-04-27 18:04 -0500   durin42
 |    Add beta and delta.
 |
 o  1   081603921c3f   2009-04-27 18:04 -0500   durin42
 |    Add gamma
 |
 o  0   d8d2fcd0e319   2009-04-27 18:04 -0500   durin42
      Add alpha

Note that ``histedit`` does *not* remove any revisions (even its own temporary
ones) until after it has completed all the editing operations, so it will
probably perform several strip operations when it's done. For the above example,
it had to run strip twice. Strip can be slow depending on a variety of factors,
so you might need to be a little patient. You can choose to keep the original
revisions by passing the ``--keep`` flag.

The ``edit`` operation will drop you back to a command prompt,
allowing you to edit files freely, or even use ``hg record`` to commit
some changes as a separate commit. When you're done, any remaining
uncommitted changes will be committed as well. When done, run ``hg
histedit --continue`` to finish this step. You'll be prompted for a
new commit message, but the default commit message will be the
original message for the ``edit`` ed revision.

The ``message`` operation will give you a chance to revise a commit
message without changing the contents. It's a shortcut for doing
``edit`` immediately followed by `hg histedit --continue``.

If ``histedit`` encounters a conflict when moving a revision (while
handling ``pick`` or ``fold``), it'll stop in a similar manner to
``edit`` with the difference that it won't prompt you for a commit
message when done. If you decide at this point that you don't like how
much work it will be to rearrange history, or that you made a mistake,
you can use ``hg histedit --abort`` to abandon the new changes you
have made and return to the state before you attempted to edit your
history.

If we clone the histedit-ed example repository above and add four more
changes, such that we have the following history::

   @  6[tip]   038383181893   2009-04-27 18:04 -0500   stefan
   |    Add theta
   |
   o  5   140988835471   2009-04-27 18:04 -0500   stefan
   |    Add eta
   |
   o  4   122930637314   2009-04-27 18:04 -0500   stefan
   |    Add zeta
   |
   o  3   836302820282   2009-04-27 18:04 -0500   stefan
   |    Add epsilon
   |
   o  2   989b4d060121   2009-04-27 18:04 -0500   durin42
   |    Add beta and delta.
   |
   o  1   081603921c3f   2009-04-27 18:04 -0500   durin42
   |    Add gamma
   |
   o  0   d8d2fcd0e319   2009-04-27 18:04 -0500   durin42
        Add alpha

If you run ``hg histedit --outgoing`` on the clone then it is the same
as running ``hg histedit 836302820282``. If you need plan to push to a
repository that Mercurial does not detect to be related to the source
repo, you can add a ``--force`` option.

Histedit rule lines are truncated to 80 characters by default. You
can customise this behaviour by setting a different length in your
configuration file::

  [histedit]
  linelen = 120      # truncate rule lines at 120 characters
"""

try:
    import cPickle as pickle
    pickle.dump # import now
except ImportError:
    import pickle
import errno
import os
import sys

from mercurial import cmdutil
from mercurial import discovery
from mercurial import error
from mercurial import changegroup
from mercurial import copies
from mercurial import context
from mercurial import exchange
from mercurial import extensions
from mercurial import hg
from mercurial import node
from mercurial import repair
from mercurial import scmutil
from mercurial import util
from mercurial import obsolete
from mercurial import merge as mergemod
from mercurial.lock import release
from mercurial.i18n import _

cmdtable = {}
command = cmdutil.command(cmdtable)

# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'

# i18n: command names and abbreviations must remain untranslated
editcomment = _("""# Edit history between %s and %s
#
# Commits are listed from least to most recent
#
# Commands:
#  p, pick = use commit
#  e, edit = use commit, but stop for amending
#  f, fold = use commit, but combine it with the one above
#  r, roll = like fold, but discard this commit's description
#  d, drop = remove commit from history
#  m, mess = edit message without changing commit content
#
""")

class histeditstate(object):
    def __init__(self, repo, parentctxnode=None, rules=None, keep=None,
            topmost=None, replacements=None, lock=None, wlock=None):
        self.repo = repo
        self.rules = rules
        self.keep = keep
        self.topmost = topmost
        self.parentctxnode = parentctxnode
        self.lock = lock
        self.wlock = wlock
        self.backupfile = None
        if replacements is None:
            self.replacements = []
        else:
            self.replacements = replacements

    def read(self):
        """Load histedit state from disk and set fields appropriately."""
        try:
            fp = self.repo.vfs('histedit-state', 'r')
        except IOError, err:
            if err.errno != errno.ENOENT:
                raise
            raise util.Abort(_('no histedit in progress'))

        try:
            data = pickle.load(fp)
            parentctxnode, rules, keep, topmost, replacements = data
            backupfile = None
        except pickle.UnpicklingError:
            data = self._load()
            parentctxnode, rules, keep, topmost, replacements, backupfile = data

        self.parentctxnode = parentctxnode
        self.rules = rules
        self.keep = keep
        self.topmost = topmost
        self.replacements = replacements
        self.backupfile = backupfile

    def write(self):
        fp = self.repo.vfs('histedit-state', 'w')
        fp.write('v1\n')
        fp.write('%s\n' % node.hex(self.parentctxnode))
        fp.write('%s\n' % node.hex(self.topmost))
        fp.write('%s\n' % self.keep)
        fp.write('%d\n' % len(self.rules))
        for rule in self.rules:
            fp.write('%s\n' % rule[0]) # action
            fp.write('%s\n' % rule[1]) # remainder
        fp.write('%d\n' % len(self.replacements))
        for replacement in self.replacements:
            fp.write('%s%s\n' % (node.hex(replacement[0]), ''.join(node.hex(r)
                for r in replacement[1])))
        backupfile = self.backupfile
        if not backupfile:
            backupfile = ''
        fp.write('%s\n' % backupfile)
        fp.close()

    def _load(self):
        fp = self.repo.vfs('histedit-state', 'r')
        lines = [l[:-1] for l in fp.readlines()]

        index = 0
        lines[index] # version number
        index += 1

        parentctxnode = node.bin(lines[index])
        index += 1

        topmost = node.bin(lines[index])
        index += 1

        keep = lines[index] == 'True'
        index += 1

        # Rules
        rules = []
        rulelen = int(lines[index])
        index += 1
        for i in xrange(rulelen):
            ruleaction = lines[index]
            index += 1
            rule = lines[index]
            index += 1
            rules.append((ruleaction, rule))

        # Replacements
        replacements = []
        replacementlen = int(lines[index])
        index += 1
        for i in xrange(replacementlen):
            replacement = lines[index]
            original = node.bin(replacement[:40])
            succ = [node.bin(replacement[i:i + 40]) for i in
                    range(40, len(replacement), 40)]
            replacements.append((original, succ))
            index += 1

        backupfile = lines[index]
        index += 1

        fp.close()

        return parentctxnode, rules, keep, topmost, replacements, backupfile

    def clear(self):
        self.repo.vfs.unlink('histedit-state')

class histeditaction(object):
    def __init__(self, state, node):
        self.state = state
        self.repo = state.repo
        self.node = node

    @classmethod
    def fromrule(cls, state, rule):
        """Parses the given rule, returning an instance of the histeditaction.
        """
        repo = state.repo
        rulehash = rule.strip().split(' ', 1)[0]
        try:
            node = repo[rulehash].node()
        except error.RepoError:
            raise util.Abort(_('unknown changeset %s listed') % rulehash[:12])
        return cls(state, node)

    def run(self):
        """Runs the action. The default behavior is simply apply the action's
        rulectx onto the current parentctx."""
        self.applychange()
        self.continuedirty()
        return self.continueclean()

    def applychange(self):
        """Applies the changes from this action's rulectx onto the current
        parentctx, but does not commit them."""
        repo = self.repo
        rulectx = repo[self.node]
        hg.update(repo, self.state.parentctxnode)
        stats = applychanges(repo.ui, repo, rulectx, {})
        if stats and stats[3] > 0:
            raise error.InterventionRequired(_('Fix up the change and run '
                                            'hg histedit --continue'))

    def continuedirty(self):
        """Continues the action when changes have been applied to the working
        copy. The default behavior is to commit the dirty changes."""
        repo = self.repo
        rulectx = repo[self.node]

        editor = self.commiteditor()
        commit = commitfuncfor(repo, rulectx)

        commit(text=rulectx.description(), user=rulectx.user(),
               date=rulectx.date(), extra=rulectx.extra(), editor=editor)

    def commiteditor(self):
        """The editor to be used to edit the commit message."""
        return False

    def continueclean(self):
        """Continues the action when the working copy is clean. The default
        behavior is to accept the current commit as the new version of the
        rulectx."""
        ctx = self.repo['.']
        if ctx.node() == self.state.parentctxnode:
            self.repo.ui.warn(_('%s: empty changeset\n') %
                              node.short(self.node))
            return ctx, [(self.node, tuple())]
        if ctx.node() == self.node:
            # Nothing changed
            return ctx, []
        return ctx, [(self.node, (ctx.node(),))]

def commitfuncfor(repo, src):
    """Build a commit function for the replacement of <src>

    This function ensure we apply the same treatment to all changesets.

    - Add a 'histedit_source' entry in extra.

    Note that fold have its own separated logic because its handling is a bit
    different and not easily factored out of the fold method.
    """
    phasemin = src.phase()
    def commitfunc(**kwargs):
        phasebackup = repo.ui.backupconfig('phases', 'new-commit')
        try:
            repo.ui.setconfig('phases', 'new-commit', phasemin,
                              'histedit')
            extra = kwargs.get('extra', {}).copy()
            extra['histedit_source'] = src.hex()
            kwargs['extra'] = extra
            return repo.commit(**kwargs)
        finally:
            repo.ui.restoreconfig(phasebackup)
    return commitfunc

def applychanges(ui, repo, ctx, opts):
    """Merge changeset from ctx (only) in the current working directory"""
    wcpar = repo.dirstate.parents()[0]
    if ctx.p1().node() == wcpar:
        # edition ar "in place" we do not need to make any merge,
        # just applies changes on parent for edition
        cmdutil.revert(ui, repo, ctx, (wcpar, node.nullid), all=True)
        stats = None
    else:
        try:
            # ui.forcemerge is an internal variable, do not document
            repo.ui.setconfig('ui', 'forcemerge', opts.get('tool', ''),
                              'histedit')
            stats = mergemod.graft(repo, ctx, ctx.p1(), ['local', 'histedit'])
        finally:
            repo.ui.setconfig('ui', 'forcemerge', '', 'histedit')
    return stats

def collapse(repo, first, last, commitopts, skipprompt=False):
    """collapse the set of revisions from first to last as new one.

    Expected commit options are:
        - message
        - date
        - username
    Commit message is edited in all cases.

    This function works in memory."""
    ctxs = list(repo.set('%d::%d', first, last))
    if not ctxs:
        return None
    base = first.parents()[0]

    # commit a new version of the old changeset, including the update
    # collect all files which might be affected
    files = set()
    for ctx in ctxs:
        files.update(ctx.files())

    # Recompute copies (avoid recording a -> b -> a)
    copied = copies.pathcopies(base, last)

    # prune files which were reverted by the updates
    def samefile(f):
        if f in last.manifest():
            a = last.filectx(f)
            if f in base.manifest():
                b = base.filectx(f)
                return (a.data() == b.data()
                        and a.flags() == b.flags())
            else:
                return False
        else:
            return f not in base.manifest()
    files = [f for f in files if not samefile(f)]
    # commit version of these files as defined by head
    headmf = last.manifest()
    def filectxfn(repo, ctx, path):
        if path in headmf:
            fctx = last[path]
            flags = fctx.flags()
            mctx = context.memfilectx(repo,
                                      fctx.path(), fctx.data(),
                                      islink='l' in flags,
                                      isexec='x' in flags,
                                      copied=copied.get(path))
            return mctx
        return None

    if commitopts.get('message'):
        message = commitopts['message']
    else:
        message = first.description()
    user = commitopts.get('user')
    date = commitopts.get('date')
    extra = commitopts.get('extra')

    parents = (first.p1().node(), first.p2().node())
    editor = None
    if not skipprompt:
        editor = cmdutil.getcommiteditor(edit=True, editform='histedit.fold')
    new = context.memctx(repo,
                         parents=parents,
                         text=message,
                         files=files,
                         filectxfn=filectxfn,
                         user=user,
                         date=date,
                         extra=extra,
                         editor=editor)
    return repo.commitctx(new)

class pick(histeditaction):
    def run(self):
        rulectx = self.repo[self.node]
        if rulectx.parents()[0].node() == self.state.parentctxnode:
            self.repo.ui.debug('node %s unchanged\n' % node.short(self.node))
            return rulectx, []

        return super(pick, self).run()

class edit(histeditaction):
    def run(self):
        repo = self.repo
        rulectx = repo[self.node]
        hg.update(repo, self.state.parentctxnode)
        applychanges(repo.ui, repo, rulectx, {})
        raise error.InterventionRequired(
            _('Make changes as needed, you may commit or record as needed '
              'now.\nWhen you are finished, run hg histedit --continue to '
              'resume.'))

    def commiteditor(self):
        return cmdutil.getcommiteditor(edit=True, editform='histedit.edit')

class fold(histeditaction):
    def continuedirty(self):
        repo = self.repo
        rulectx = repo[self.node]

        commit = commitfuncfor(repo, rulectx)
        commit(text='fold-temp-revision %s' % node.short(self.node),
               user=rulectx.user(), date=rulectx.date(),
               extra=rulectx.extra())

    def continueclean(self):
        repo = self.repo
        ctx = repo['.']
        rulectx = repo[self.node]
        parentctxnode = self.state.parentctxnode
        if ctx.node() == parentctxnode:
            repo.ui.warn(_('%s: empty changeset\n') %
                              node.short(self.node))
            return ctx, [(self.node, (parentctxnode,))]

        parentctx = repo[parentctxnode]
        newcommits = set(c.node() for c in repo.set('(%d::. - %d)', parentctx,
                                                 parentctx))
        if not newcommits:
            repo.ui.warn(_('%s: cannot fold - working copy is not a '
                           'descendant of previous commit %s\n') %
                           (node.short(self.node), node.short(parentctxnode)))
            return ctx, [(self.node, (ctx.node(),))]

        middlecommits = newcommits.copy()
        middlecommits.discard(ctx.node())

        return self.finishfold(repo.ui, repo, parentctx, rulectx, ctx.node(),
                               middlecommits)

    def skipprompt(self):
        return False

    def finishfold(self, ui, repo, ctx, oldctx, newnode, internalchanges):
        parent = ctx.parents()[0].node()
        hg.update(repo, parent)
        ### prepare new commit data
        commitopts = {}
        commitopts['user'] = ctx.user()
        # commit message
        if self.skipprompt():
            newmessage = ctx.description()
        else:
            newmessage = '\n***\n'.join(
                [ctx.description()] +
                [repo[r].description() for r in internalchanges] +
                [oldctx.description()]) + '\n'
        commitopts['message'] = newmessage
        # date
        commitopts['date'] = max(ctx.date(), oldctx.date())
        extra = ctx.extra().copy()
        # histedit_source
        # note: ctx is likely a temporary commit but that the best we can do
        #       here. This is sufficient to solve issue3681 anyway.
        extra['histedit_source'] = '%s,%s' % (ctx.hex(), oldctx.hex())
        commitopts['extra'] = extra
        phasebackup = repo.ui.backupconfig('phases', 'new-commit')
        try:
            phasemin = max(ctx.phase(), oldctx.phase())
            repo.ui.setconfig('phases', 'new-commit', phasemin, 'histedit')
            n = collapse(repo, ctx, repo[newnode], commitopts,
                         skipprompt=self.skipprompt())
        finally:
            repo.ui.restoreconfig(phasebackup)
        if n is None:
            return ctx, []
        hg.update(repo, n)
        replacements = [(oldctx.node(), (newnode,)),
                        (ctx.node(), (n,)),
                        (newnode, (n,)),
                       ]
        for ich in internalchanges:
            replacements.append((ich, (n,)))
        return repo[n], replacements

class rollup(fold):
    def skipprompt(self):
        return True

class drop(histeditaction):
    def run(self):
        parentctx = self.repo[self.state.parentctxnode]
        return parentctx, [(self.node, tuple())]

class message(histeditaction):
    def commiteditor(self):
        return cmdutil.getcommiteditor(edit=True, editform='histedit.mess')

def findoutgoing(ui, repo, remote=None, force=False, opts={}):
    """utility function to find the first outgoing changeset

    Used by initialisation code"""
    dest = ui.expandpath(remote or 'default-push', remote or 'default')
    dest, revs = hg.parseurl(dest, None)[:2]
    ui.status(_('comparing with %s\n') % util.hidepassword(dest))

    revs, checkout = hg.addbranchrevs(repo, repo, revs, None)
    other = hg.peer(repo, opts, dest)

    if revs:
        revs = [repo.lookup(rev) for rev in revs]

    outgoing = discovery.findcommonoutgoing(repo, other, revs, force=force)
    if not outgoing.missing:
        raise util.Abort(_('no outgoing ancestors'))
    roots = list(repo.revs("roots(%ln)", outgoing.missing))
    if 1 < len(roots):
        msg = _('there are ambiguous outgoing revisions')
        hint = _('see "hg help histedit" for more detail')
        raise util.Abort(msg, hint=hint)
    return repo.lookup(roots[0])

actiontable = {'p': pick,
               'pick': pick,
               'e': edit,
               'edit': edit,
               'f': fold,
               'fold': fold,
               'r': rollup,
               'roll': rollup,
               'd': drop,
               'drop': drop,
               'm': message,
               'mess': message,
               }

@command('histedit',
    [('', 'commands', '',
      _('read history edits from the specified file'), _('FILE')),
     ('c', 'continue', False, _('continue an edit already in progress')),
     ('', 'edit-plan', False, _('edit remaining actions list')),
     ('k', 'keep', False,
      _("don't strip old nodes after edit is complete")),
     ('', 'abort', False, _('abort an edit in progress')),
     ('o', 'outgoing', False, _('changesets not found in destination')),
     ('f', 'force', False,
      _('force outgoing even for unrelated repositories')),
     ('r', 'rev', [], _('first revision to be edited'), _('REV'))],
     _("ANCESTOR | --outgoing [URL]"))
def histedit(ui, repo, *freeargs, **opts):
    """interactively edit changeset history

    This command edits changesets between ANCESTOR and the parent of
    the working directory.

    With --outgoing, this edits changesets not found in the
    destination repository. If URL of the destination is omitted, the
    'default-push' (or 'default') path will be used.

    For safety, this command is aborted, also if there are ambiguous
    outgoing revisions which may confuse users: for example, there are
    multiple branches containing outgoing revisions.

    Use "min(outgoing() and ::.)" or similar revset specification
    instead of --outgoing to specify edit target revision exactly in
    such ambiguous situation. See :hg:`help revsets` for detail about
    selecting revisions.

    Returns 0 on success, 1 if user intervention is required (not only
    for intentional "edit" command, but also for resolving unexpected
    conflicts).
    """
    state = histeditstate(repo)
    try:
        state.wlock = repo.wlock()
        state.lock = repo.lock()
        _histedit(ui, repo, state, *freeargs, **opts)
    finally:
        release(state.lock, state.wlock)

def _histedit(ui, repo, state, *freeargs, **opts):
    # TODO only abort if we try and histedit mq patches, not just
    # blanket if mq patches are applied somewhere
    mq = getattr(repo, 'mq', None)
    if mq and mq.applied:
        raise util.Abort(_('source has mq patches applied'))

    # basic argument incompatibility processing
    outg = opts.get('outgoing')
    cont = opts.get('continue')
    editplan = opts.get('edit_plan')
    abort = opts.get('abort')
    force = opts.get('force')
    rules = opts.get('commands', '')
    revs = opts.get('rev', [])
    goal = 'new' # This invocation goal, in new, continue, abort
    if force and not outg:
        raise util.Abort(_('--force only allowed with --outgoing'))
    if cont:
        if any((outg, abort, revs, freeargs, rules, editplan)):
            raise util.Abort(_('no arguments allowed with --continue'))
        goal = 'continue'
    elif abort:
        if any((outg, revs, freeargs, rules, editplan)):
            raise util.Abort(_('no arguments allowed with --abort'))
        goal = 'abort'
    elif editplan:
        if any((outg, revs, freeargs)):
            raise util.Abort(_('only --commands argument allowed with '
                               '--edit-plan'))
        goal = 'edit-plan'
    else:
        if os.path.exists(os.path.join(repo.path, 'histedit-state')):
            raise util.Abort(_('history edit already in progress, try '
                               '--continue or --abort'))
        if outg:
            if revs:
                raise util.Abort(_('no revisions allowed with --outgoing'))
            if len(freeargs) > 1:
                raise util.Abort(
                    _('only one repo argument allowed with --outgoing'))
        else:
            revs.extend(freeargs)
            if len(revs) == 0:
                histeditdefault = ui.config('histedit', 'defaultrev')
                if histeditdefault:
                    revs.append(histeditdefault)
            if len(revs) != 1:
                raise util.Abort(
                    _('histedit requires exactly one ancestor revision'))


    replacements = []
    keep = opts.get('keep', False)

    # rebuild state
    if goal == 'continue':
        state.read()
        state = bootstrapcontinue(ui, state, opts)
    elif goal == 'edit-plan':
        state.read()
        if not rules:
            comment = editcomment % (node.short(state.parentctxnode),
                                     node.short(state.topmost))
            rules = ruleeditor(repo, ui, state.rules, comment)
        else:
            if rules == '-':
                f = sys.stdin
            else:
                f = open(rules)
            rules = f.read()
            f.close()
        rules = [l for l in (r.strip() for r in rules.splitlines())
                 if l and not l.startswith('#')]
        rules = verifyrules(rules, repo, [repo[c] for [_a, c] in state.rules])
        state.rules = rules
        state.write()
        return
    elif goal == 'abort':
        state.read()
        mapping, tmpnodes, leafs, _ntm = processreplacement(state)
        ui.debug('restore wc to old parent %s\n' % node.short(state.topmost))

        # Recover our old commits if necessary
        if not state.topmost in repo and state.backupfile:
            backupfile = repo.join(state.backupfile)
            f = hg.openpath(ui, backupfile)
            gen = exchange.readbundle(ui, f, backupfile)
            changegroup.addchangegroup(repo, gen, 'histedit',
                                       'bundle:' + backupfile)
            os.remove(backupfile)

        # check whether we should update away
        parentnodes = [c.node() for c in repo[None].parents()]
        for n in leafs | set([state.parentctxnode]):
            if n in parentnodes:
                hg.clean(repo, state.topmost)
                break
        else:
            pass
        cleanupnode(ui, repo, 'created', tmpnodes)
        cleanupnode(ui, repo, 'temp', leafs)
        state.clear()
        return
    else:
        cmdutil.checkunfinished(repo)
        cmdutil.bailifchanged(repo)

        topmost, empty = repo.dirstate.parents()
        if outg:
            if freeargs:
                remote = freeargs[0]
            else:
                remote = None
            root = findoutgoing(ui, repo, remote, force, opts)
        else:
            rr = list(repo.set('roots(%ld)', scmutil.revrange(repo, revs)))
            if len(rr) != 1:
                raise util.Abort(_('The specified revisions must have '
                    'exactly one common root'))
            root = rr[0].node()

        revs = between(repo, root, topmost, keep)
        if not revs:
            raise util.Abort(_('%s is not an ancestor of working directory') %
                             node.short(root))

        ctxs = [repo[r] for r in revs]
        if not rules:
            comment = editcomment % (node.short(root), node.short(topmost))
            rules = ruleeditor(repo, ui, [['pick', c] for c in ctxs], comment)
        else:
            if rules == '-':
                f = sys.stdin
            else:
                f = open(rules)
            rules = f.read()
            f.close()
        rules = [l for l in (r.strip() for r in rules.splitlines())
                 if l and not l.startswith('#')]
        rules = verifyrules(rules, repo, ctxs)

        parentctxnode = repo[root].parents()[0].node()

        state.parentctxnode = parentctxnode
        state.rules = rules
        state.keep = keep
        state.topmost = topmost
        state.replacements = replacements

        # Create a backup so we can always abort completely.
        backupfile = None
        if not obsolete.isenabled(repo, obsolete.createmarkersopt):
            backupfile = repair._bundle(repo, [parentctxnode], [topmost], root,
                                        'histedit')
        state.backupfile = backupfile

    while state.rules:
        state.write()
        action, ha = state.rules.pop(0)
        ui.debug('histedit: processing %s %s\n' % (action, ha[:12]))
        actobj = actiontable[action].fromrule(state, ha)
        parentctx, replacement_ = actobj.run()
        state.parentctxnode = parentctx.node()
        state.replacements.extend(replacement_)
    state.write()

    hg.update(repo, state.parentctxnode)

    mapping, tmpnodes, created, ntm = processreplacement(state)
    if mapping:
        for prec, succs in mapping.iteritems():
            if not succs:
                ui.debug('histedit: %s is dropped\n' % node.short(prec))
            else:
                ui.debug('histedit: %s is replaced by %s\n' % (
                    node.short(prec), node.short(succs[0])))
                if len(succs) > 1:
                    m = 'histedit:                            %s'
                    for n in succs[1:]:
                        ui.debug(m % node.short(n))

    if not keep:
        if mapping:
            movebookmarks(ui, repo, mapping, state.topmost, ntm)
            # TODO update mq state
        if obsolete.isenabled(repo, obsolete.createmarkersopt):
            markers = []
            # sort by revision number because it sound "right"
            for prec in sorted(mapping, key=repo.changelog.rev):
                succs = mapping[prec]
                markers.append((repo[prec],
                                tuple(repo[s] for s in succs)))
            if markers:
                obsolete.createmarkers(repo, markers)
        else:
            cleanupnode(ui, repo, 'replaced', mapping)

    cleanupnode(ui, repo, 'temp', tmpnodes)
    state.clear()
    if os.path.exists(repo.sjoin('undo')):
        os.unlink(repo.sjoin('undo'))

def bootstrapcontinue(ui, state, opts):
    repo = state.repo
    if state.rules:
        action, currentnode = state.rules.pop(0)

        actobj = actiontable[action].fromrule(state, currentnode)

        s = repo.status()
        if s.modified or s.added or s.removed or s.deleted:
            actobj.continuedirty()
            s = repo.status()
            if s.modified or s.added or s.removed or s.deleted:
                raise util.Abort(_("working copy still dirty"))

        parentctx, replacements = actobj.continueclean()

        state.parentctxnode = parentctx.node()
        state.replacements.extend(replacements)

    return state

def between(repo, old, new, keep):
    """select and validate the set of revision to edit

    When keep is false, the specified set can't have children."""
    ctxs = list(repo.set('%n::%n', old, new))
    if ctxs and not keep:
        if (not obsolete.isenabled(repo, obsolete.allowunstableopt) and
            repo.revs('(%ld::) - (%ld)', ctxs, ctxs)):
            raise util.Abort(_('cannot edit history that would orphan nodes'))
        if repo.revs('(%ld) and merge()', ctxs):
            raise util.Abort(_('cannot edit history that contains merges'))
        root = ctxs[0] # list is already sorted by repo.set
        if not root.mutable():
            raise util.Abort(_('cannot edit immutable changeset: %s') % root)
    return [c.node() for c in ctxs]

def makedesc(repo, action, rev):
    """build a initial action line for a ctx

    line are in the form:

      <action> <hash> <rev> <summary>
    """
    ctx = repo[rev]
    summary = ''
    if ctx.description():
        summary = ctx.description().splitlines()[0]
    line = '%s %s %d %s' % (action, ctx, ctx.rev(), summary)
    # trim to 80 columns so it's not stupidly wide in my editor
    maxlen = repo.ui.configint('histedit', 'linelen', default=80)
    maxlen = max(maxlen, 22) # avoid truncating hash
    return util.ellipsis(line, maxlen)

def ruleeditor(repo, ui, rules, editcomment=""):
    """open an editor to edit rules

    rules are in the format [ [act, ctx], ...] like in state.rules
    """
    rules = '\n'.join([makedesc(repo, act, rev) for [act, rev] in rules])
    rules += '\n\n'
    rules += editcomment
    rules = ui.edit(rules, ui.username())

    # Save edit rules in .hg/histedit-last-edit.txt in case
    # the user needs to ask for help after something
    # surprising happens.
    f = open(repo.join('histedit-last-edit.txt'), 'w')
    f.write(rules)
    f.close()

    return rules

def verifyrules(rules, repo, ctxs):
    """Verify that there exists exactly one edit rule per given changeset.

    Will abort if there are to many or too few rules, a malformed rule,
    or a rule on a changeset outside of the user-given range.
    """
    parsed = []
    expected = set(c.hex() for c in ctxs)
    seen = set()
    for r in rules:
        if ' ' not in r:
            raise util.Abort(_('malformed line "%s"') % r)
        action, rest = r.split(' ', 1)
        ha = rest.strip().split(' ', 1)[0]
        try:
            ha = repo[ha].hex()
        except error.RepoError:
            raise util.Abort(_('unknown changeset %s listed') % ha[:12])
        if ha not in expected:
            raise util.Abort(
                _('may not use changesets other than the ones listed'))
        if ha in seen:
            raise util.Abort(_('duplicated command for changeset %s') %
                    ha[:12])
        seen.add(ha)
        if action not in actiontable:
            raise util.Abort(_('unknown action "%s"') % action)
        parsed.append([action, ha])
    missing = sorted(expected - seen)  # sort to stabilize output
    if missing:
        raise util.Abort(_('missing rules for changeset %s') %
                missing[0][:12],
                hint=_('do you want to use the drop action?'))
    return parsed

def processreplacement(state):
    """process the list of replacements to return

    1) the final mapping between original and created nodes
    2) the list of temporary node created by histedit
    3) the list of new commit created by histedit"""
    replacements = state.replacements
    allsuccs = set()
    replaced = set()
    fullmapping = {}
    # initialise basic set
    # fullmapping record all operation recorded in replacement
    for rep in replacements:
        allsuccs.update(rep[1])
        replaced.add(rep[0])
        fullmapping.setdefault(rep[0], set()).update(rep[1])
    new = allsuccs - replaced
    tmpnodes = allsuccs & replaced
    # Reduce content fullmapping  into direct relation between original nodes
    # and final node created during history edition
    # Dropped changeset are replaced by an empty list
    toproceed = set(fullmapping)
    final = {}
    while toproceed:
        for x in list(toproceed):
            succs = fullmapping[x]
            for s in list(succs):
                if s in toproceed:
                    # non final node with unknown closure
                    # We can't process this now
                    break
                elif s in final:
                    # non final node, replace with closure
                    succs.remove(s)
                    succs.update(final[s])
            else:
                final[x] = succs
                toproceed.remove(x)
    # remove tmpnodes from final mapping
    for n in tmpnodes:
        del final[n]
    # we expect all changes involved in final to exist in the repo
    # turn `final` into list (topologically sorted)
    nm = state.repo.changelog.nodemap
    for prec, succs in final.items():
        final[prec] = sorted(succs, key=nm.get)

    # computed topmost element (necessary for bookmark)
    if new:
        newtopmost = sorted(new, key=state.repo.changelog.rev)[-1]
    elif not final:
        # Nothing rewritten at all. we won't need `newtopmost`
        # It is the same as `oldtopmost` and `processreplacement` know it
        newtopmost = None
    else:
        # every body died. The newtopmost is the parent of the root.
        r = state.repo.changelog.rev
        newtopmost = state.repo[sorted(final, key=r)[0]].p1().node()

    return final, tmpnodes, new, newtopmost

def movebookmarks(ui, repo, mapping, oldtopmost, newtopmost):
    """Move bookmark from old to newly created node"""
    if not mapping:
        # if nothing got rewritten there is not purpose for this function
        return
    moves = []
    for bk, old in sorted(repo._bookmarks.iteritems()):
        if old == oldtopmost:
            # special case ensure bookmark stay on tip.
            #
            # This is arguably a feature and we may only want that for the
            # active bookmark. But the behavior is kept compatible with the old
            # version for now.
            moves.append((bk, newtopmost))
            continue
        base = old
        new = mapping.get(base, None)
        if new is None:
            continue
        while not new:
            # base is killed, trying with parent
            base = repo[base].p1().node()
            new = mapping.get(base, (base,))
            # nothing to move
        moves.append((bk, new[-1]))
    if moves:
        marks = repo._bookmarks
        for mark, new in moves:
            old = marks[mark]
            ui.note(_('histedit: moving bookmarks %s from %s to %s\n')
                    % (mark, node.short(old), node.short(new)))
            marks[mark] = new
        marks.write()

def cleanupnode(ui, repo, name, nodes):
    """strip a group of nodes from the repository

    The set of node to strip may contains unknown nodes."""
    ui.debug('should strip %s nodes %s\n' %
             (name, ', '.join([node.short(n) for n in nodes])))
    lock = None
    try:
        lock = repo.lock()
        # Find all node that need to be stripped
        # (we hg %lr instead of %ln to silently ignore unknown item
        nm = repo.changelog.nodemap
        nodes = sorted(n for n in nodes if n in nm)
        roots = [c.node() for c in repo.set("roots(%ln)", nodes)]
        for c in roots:
            # We should process node in reverse order to strip tip most first.
            # but this trigger a bug in changegroup hook.
            # This would reduce bundle overhead
            repair.strip(ui, repo, c)
    finally:
        release(lock)

def stripwrapper(orig, ui, repo, nodelist, *args, **kwargs):
    if isinstance(nodelist, str):
        nodelist = [nodelist]
    if os.path.exists(os.path.join(repo.path, 'histedit-state')):
        state = histeditstate(repo)
        state.read()
        histedit_nodes = set([repo[rulehash].node() for (action, rulehash)
                             in state.rules if rulehash in repo])
        strip_nodes = set([repo[n].node() for n in nodelist])
        common_nodes = histedit_nodes & strip_nodes
        if common_nodes:
            raise util.Abort(_("histedit in progress, can't strip %s")
                             % ', '.join(node.short(x) for x in common_nodes))
    return orig(ui, repo, nodelist, *args, **kwargs)

extensions.wrapfunction(repair, 'strip', stripwrapper)

def summaryhook(ui, repo):
    if not os.path.exists(repo.join('histedit-state')):
        return
    state = histeditstate(repo)
    state.read()
    if state.rules:
        # i18n: column positioning for "hg summary"
        ui.write(_('hist:   %s (histedit --continue)\n') %
                 (ui.label(_('%d remaining'), 'histedit.remaining') %
                  len(state.rules)))

def extsetup(ui):
    cmdutil.summaryhooks.add('histedit', summaryhook)
    cmdutil.unfinishedstates.append(
        ['histedit-state', False, True, _('histedit in progress'),
         _("use 'hg histedit --continue' or 'hg histedit --abort'")])
