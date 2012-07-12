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

 # Edit history between 633536316234 and 7c2fd3b9020c
 #
 # Commands:
 #  p, pick = use commit
 #  e, edit = use commit, but stop for amending
 #  f, fold = use commit, but fold into previous commit
 #  d, drop = remove commit from history
 #  m, mess = edit message without changing commit content
 #
 0 files updated, 0 files merged, 0 files removed, 0 files unresolved

In this file, lines beginning with ``#`` are ignored. You must specify a rule
for each revision in your history. For example, if you had meant to add gamma
before beta, and then wanted to add delta in the same revision as beta, you
would reorganize the file to look like this::

 pick 030b686bedc4 Add gamma
 pick c561b4e977df Add beta
 fold 7c2fd3b9020c Add delta

 # Edit history between 633536316234 and 7c2fd3b9020c
 #
 # Commands:
 #  p, pick = use commit
 #  e, edit = use commit, but stop for amending
 #  f, fold = use commit, but fold into previous commit
 #  d, drop = remove commit from history
 #  m, mess = edit message without changing commit content
 #
 0 files updated, 0 files merged, 0 files removed, 0 files unresolved

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

If we clone the example repository above and add three more changes, such that
we have the following history::

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
"""

try:
    import cPickle as pickle
except ImportError:
    import pickle
import tempfile
import os

from mercurial import bookmarks
from mercurial import cmdutil
from mercurial import discovery
from mercurial import error
from mercurial import hg
from mercurial import node
from mercurial import patch
from mercurial import repair
from mercurial import scmutil
from mercurial import util
from mercurial.i18n import _

testedwith = 'internal'

editcomment = """

# Edit history between %s and %s
#
# Commands:
#  p, pick = use commit
#  e, edit = use commit, but stop for amending
#  f, fold = use commit, but fold into previous commit (combines N and N-1)
#  d, drop = remove commit from history
#  m, mess = edit message without changing commit content
#
"""

def between(repo, old, new, keep):
    revs = [old]
    current = old
    while current != new:
        ctx = repo[current]
        if not keep and len(ctx.children()) > 1:
            raise util.Abort(_('cannot edit history that would orphan nodes'))
        if len(ctx.parents()) != 1 and ctx.parents()[1] != node.nullid:
            raise util.Abort(_("can't edit history with merges"))
        if not ctx.children():
            current = new
        else:
            current = ctx.children()[0].node()
            revs.append(current)
    if len(repo[current].children()) and not keep:
        raise util.Abort(_('cannot edit history that would orphan nodes'))
    return revs


def pick(ui, repo, ctx, ha, opts):
    oldctx = repo[ha]
    if oldctx.parents()[0] == ctx:
        ui.debug('node %s unchanged\n' % ha)
        return oldctx, [], [], []
    hg.update(repo, ctx.node())
    fd, patchfile = tempfile.mkstemp(prefix='hg-histedit-')
    fp = os.fdopen(fd, 'w')
    diffopts = patch.diffopts(ui, opts)
    diffopts.git = True
    diffopts.ignorews = False
    diffopts.ignorewsamount = False
    diffopts.ignoreblanklines = False
    gen = patch.diff(repo, oldctx.parents()[0].node(), ha, opts=diffopts)
    for chunk in gen:
        fp.write(chunk)
    fp.close()
    try:
        files = set()
        try:
            patch.patch(ui, repo, patchfile, files=files, eolmode=None)
            if not files:
                ui.warn(_('%s: empty changeset')
                             % node.hex(ha))
                return ctx, [], [], []
        finally:
            os.unlink(patchfile)
    except Exception:
        raise util.Abort(_('Fix up the change and run '
                           'hg histedit --continue'))
    n = repo.commit(text=oldctx.description(), user=oldctx.user(),
                    date=oldctx.date(), extra=oldctx.extra())
    return repo[n], [n], [oldctx.node()], []


def edit(ui, repo, ctx, ha, opts):
    oldctx = repo[ha]
    hg.update(repo, ctx.node())
    fd, patchfile = tempfile.mkstemp(prefix='hg-histedit-')
    fp = os.fdopen(fd, 'w')
    diffopts = patch.diffopts(ui, opts)
    diffopts.git = True
    diffopts.ignorews = False
    diffopts.ignorewsamount = False
    diffopts.ignoreblanklines = False
    gen = patch.diff(repo, oldctx.parents()[0].node(), ha, opts=diffopts)
    for chunk in gen:
        fp.write(chunk)
    fp.close()
    try:
        files = set()
        try:
            patch.patch(ui, repo, patchfile, files=files, eolmode=None)
        finally:
            os.unlink(patchfile)
    except Exception:
        pass
    raise util.Abort(_('Make changes as needed, you may commit or record as '
                       'needed now.\nWhen you are finished, run hg'
                       ' histedit --continue to resume.'))

def fold(ui, repo, ctx, ha, opts):
    oldctx = repo[ha]
    hg.update(repo, ctx.node())
    fd, patchfile = tempfile.mkstemp(prefix='hg-histedit-')
    fp = os.fdopen(fd, 'w')
    diffopts = patch.diffopts(ui, opts)
    diffopts.git = True
    diffopts.ignorews = False
    diffopts.ignorewsamount = False
    diffopts.ignoreblanklines = False
    gen = patch.diff(repo, oldctx.parents()[0].node(), ha, opts=diffopts)
    for chunk in gen:
        fp.write(chunk)
    fp.close()
    try:
        files = set()
        try:
            patch.patch(ui, repo, patchfile, files=files, eolmode=None)
            if not files:
                ui.warn(_('%s: empty changeset')
                             % node.hex(ha))
                return ctx, [], [], []
        finally:
            os.unlink(patchfile)
    except Exception:
        raise util.Abort(_('Fix up the change and run '
                           'hg histedit --continue'))
    n = repo.commit(text='fold-temp-revision %s' % ha, user=oldctx.user(),
                    date=oldctx.date(), extra=oldctx.extra())
    return finishfold(ui, repo, ctx, oldctx, n, opts, [])

def finishfold(ui, repo, ctx, oldctx, newnode, opts, internalchanges):
    parent = ctx.parents()[0].node()
    hg.update(repo, parent)
    fd, patchfile = tempfile.mkstemp(prefix='hg-histedit-')
    fp = os.fdopen(fd, 'w')
    diffopts = patch.diffopts(ui, opts)
    diffopts.git = True
    diffopts.ignorews = False
    diffopts.ignorewsamount = False
    diffopts.ignoreblanklines = False
    gen = patch.diff(repo, parent, newnode, opts=diffopts)
    for chunk in gen:
        fp.write(chunk)
    fp.close()
    files = set()
    try:
        patch.patch(ui, repo, patchfile, files=files, eolmode=None)
    finally:
        os.unlink(patchfile)
    newmessage = '\n***\n'.join(
        [ctx.description()] +
        [repo[r].description() for r in internalchanges] +
        [oldctx.description()])
    # If the changesets are from the same author, keep it.
    if ctx.user() == oldctx.user():
        username = ctx.user()
    else:
        username = ui.username()
    newmessage = ui.edit(newmessage, username)
    n = repo.commit(text=newmessage, user=username,
                    date=max(ctx.date(), oldctx.date()), extra=oldctx.extra())
    return repo[n], [n], [oldctx.node(), ctx.node()], [newnode]

def drop(ui, repo, ctx, ha, opts):
    return ctx, [], [repo[ha].node()], []


def message(ui, repo, ctx, ha, opts):
    oldctx = repo[ha]
    hg.update(repo, ctx.node())
    fd, patchfile = tempfile.mkstemp(prefix='hg-histedit-')
    fp = os.fdopen(fd, 'w')
    diffopts = patch.diffopts(ui, opts)
    diffopts.git = True
    diffopts.ignorews = False
    diffopts.ignorewsamount = False
    diffopts.ignoreblanklines = False
    gen = patch.diff(repo, oldctx.parents()[0].node(), ha, opts=diffopts)
    for chunk in gen:
        fp.write(chunk)
    fp.close()
    try:
        files = set()
        try:
            patch.patch(ui, repo, patchfile, files=files, eolmode=None)
        finally:
            os.unlink(patchfile)
    except Exception:
        raise util.Abort(_('Fix up the change and run '
                           'hg histedit --continue'))
    message = oldctx.description()
    message = ui.edit(message, ui.username())
    new = repo.commit(text=message, user=oldctx.user(), date=oldctx.date(),
                      extra=oldctx.extra())
    newctx = repo[new]
    if oldctx.node() != newctx.node():
        return newctx, [new], [oldctx.node()], []
    # We didn't make an edit, so just indicate no replaced nodes
    return newctx, [new], [], []


def makedesc(c):
    summary = ''
    if c.description():
        summary = c.description().splitlines()[0]
    line = 'pick %s %d %s' % (c.hex()[:12], c.rev(), summary)
    return line[:80]  # trim to 80 chars so it's not stupidly wide in my editor

actiontable = {'p': pick,
               'pick': pick,
               'e': edit,
               'edit': edit,
               'f': fold,
               'fold': fold,
               'd': drop,
               'drop': drop,
               'm': message,
               'mess': message,
               }
def histedit(ui, repo, *parent, **opts):
    """interactively edit changeset history
    """
    # TODO only abort if we try and histedit mq patches, not just
    # blanket if mq patches are applied somewhere
    mq = getattr(repo, 'mq', None)
    if mq and mq.applied:
        raise util.Abort(_('source has mq patches applied'))

    parent = list(parent) + opts.get('rev', [])
    if opts.get('outgoing'):
        if len(parent) > 1:
            raise util.Abort(
                _('only one repo argument allowed with --outgoing'))
        elif parent:
            parent = parent[0]

        dest = ui.expandpath(parent or 'default-push', parent or 'default')
        dest, revs = hg.parseurl(dest, None)[:2]
        ui.status(_('comparing with %s\n') % util.hidepassword(dest))

        revs, checkout = hg.addbranchrevs(repo, repo, revs, None)
        other = hg.repository(hg.remoteui(repo, opts), dest)

        if revs:
            revs = [repo.lookup(rev) for rev in revs]

        parent = discovery.findcommonoutgoing(
            repo, other, [], force=opts.get('force')).missing[0:1]
    else:
        if opts.get('force'):
            raise util.Abort(_('--force only allowed with --outgoing'))

    if opts.get('continue', False):
        if len(parent) != 0:
            raise util.Abort(_('no arguments allowed with --continue'))
        (parentctxnode, created, replaced,
         tmpnodes, existing, rules, keep, tip, replacemap) = readstate(repo)
        currentparent, wantnull = repo.dirstate.parents()
        parentctx = repo[parentctxnode]
        # discover any nodes the user has added in the interim
        newchildren = [c for c in parentctx.children()
                       if c.node() not in existing]
        action, currentnode = rules.pop(0)
        while newchildren:
            if action in ('f', 'fold'):
                tmpnodes.extend([n.node() for n in newchildren])
            else:
                created.extend([n.node() for n in newchildren])
            filtered = []
            for r in newchildren:
                filtered += [c for c in r.children() if c.node not in existing]
            newchildren = filtered
        m, a, r, d = repo.status()[:4]
        oldctx = repo[currentnode]
        message = oldctx.description()
        if action in ('e', 'edit', 'm', 'mess'):
            message = ui.edit(message, ui.username())
        elif action in ('f', 'fold'):
            message = 'fold-temp-revision %s' % currentnode
        new = None
        if m or a or r or d:
            new = repo.commit(text=message, user=oldctx.user(),
                              date=oldctx.date(), extra=oldctx.extra())

        # If we're resuming a fold and we have new changes, mark the
        # replacements and finish the fold. If not, it's more like a
        # drop of the changesets that disappeared, and we can skip
        # this step.
        if action in ('f', 'fold') and (new or newchildren):
            if new:
                tmpnodes.append(new)
            else:
                new = newchildren[-1]
            (parentctx, created_, replaced_, tmpnodes_) = finishfold(
                ui, repo, parentctx, oldctx, new, opts, newchildren)
            replaced.extend(replaced_)
            created.extend(created_)
            tmpnodes.extend(tmpnodes_)
        elif action not in ('d', 'drop'):
            if new != oldctx.node():
                replaced.append(oldctx.node())
            if new:
                if new != oldctx.node():
                    created.append(new)
                parentctx = repo[new]

    elif opts.get('abort', False):
        if len(parent) != 0:
            raise util.Abort(_('no arguments allowed with --abort'))
        (parentctxnode, created, replaced, tmpnodes,
         existing, rules, keep, tip, replacemap) = readstate(repo)
        ui.debug('restore wc to old tip %s\n' % node.hex(tip))
        hg.clean(repo, tip)
        ui.debug('should strip created nodes %s\n' %
                 ', '.join([node.hex(n)[:12] for n in created]))
        ui.debug('should strip temp nodes %s\n' %
                 ', '.join([node.hex(n)[:12] for n in tmpnodes]))
        for nodes in (created, tmpnodes):
            for n in reversed(nodes):
                try:
                    repair.strip(ui, repo, n)
                except error.LookupError:
                    pass
        os.unlink(os.path.join(repo.path, 'histedit-state'))
        return
    else:
        cmdutil.bailifchanged(repo)
        if os.path.exists(os.path.join(repo.path, 'histedit-state')):
            raise util.Abort(_('history edit already in progress, try '
                               '--continue or --abort'))

        tip, empty = repo.dirstate.parents()


        if len(parent) != 1:
            raise util.Abort(_('histedit requires exactly one parent revision'))
        parent = scmutil.revsingle(repo, parent[0]).node()

        keep = opts.get('keep', False)
        revs = between(repo, parent, tip, keep)

        ctxs = [repo[r] for r in revs]
        existing = [r.node() for r in ctxs]
        rules = opts.get('commands', '')
        if not rules:
            rules = '\n'.join([makedesc(c) for c in ctxs])
            rules += editcomment % (node.hex(parent)[:12], node.hex(tip)[:12])
            rules = ui.edit(rules, ui.username())
            # Save edit rules in .hg/histedit-last-edit.txt in case
            # the user needs to ask for help after something
            # surprising happens.
            f = open(repo.join('histedit-last-edit.txt'), 'w')
            f.write(rules)
            f.close()
        else:
            f = open(rules)
            rules = f.read()
            f.close()
        rules = [l for l in (r.strip() for r in rules.splitlines())
                 if l and not l[0] == '#']
        rules = verifyrules(rules, repo, ctxs)

        parentctx = repo[parent].parents()[0]
        keep = opts.get('keep', False)
        replaced = []
        replacemap = {}
        tmpnodes = []
        created = []


    while rules:
        writestate(repo, parentctx.node(), created, replaced,
                   tmpnodes, existing, rules, keep, tip, replacemap)
        action, ha = rules.pop(0)
        (parentctx, created_, replaced_, tmpnodes_) = actiontable[action](
            ui, repo, parentctx, ha, opts)

        if replaced_:
            clen, rlen = len(created_), len(replaced_)
            if clen == rlen == 1:
                ui.debug('histedit: exact replacement of %s with %s\n' % (
                    node.short(replaced_[0]), node.short(created_[0])))

                replacemap[replaced_[0]] = created_[0]
            elif clen > rlen:
                assert rlen == 1, ('unexpected replacement of '
                                   '%d changes with %d changes' % (rlen, clen))
                # made more changesets than we're replacing
                # TODO synthesize patch names for created patches
                replacemap[replaced_[0]] = created_[-1]
                ui.debug('histedit: created many, assuming %s replaced by %s' %
                         (node.short(replaced_[0]), node.short(created_[-1])))
            elif rlen > clen:
                if not created_:
                    # This must be a drop. Try and put our metadata on
                    # the parent change.
                    assert rlen == 1
                    r = replaced_[0]
                    ui.debug('histedit: %s seems replaced with nothing, '
                            'finding a parent\n' % (node.short(r)))
                    pctx = repo[r].parents()[0]
                    if pctx.node() in replacemap:
                        ui.debug('histedit: parent is already replaced\n')
                        replacemap[r] = replacemap[pctx.node()]
                    else:
                        replacemap[r] = pctx.node()
                    ui.debug('histedit: %s best replaced by %s\n' % (
                        node.short(r), node.short(replacemap[r])))
                else:
                    assert len(created_) == 1
                    for r in replaced_:
                        ui.debug('histedit: %s replaced by %s\n' % (
                            node.short(r), node.short(created_[0])))
                        replacemap[r] = created_[0]
            else:
                assert False, (
                    'Unhandled case in replacement mapping! '
                    'replacing %d changes with %d changes' % (rlen, clen))
        created.extend(created_)
        replaced.extend(replaced_)
        tmpnodes.extend(tmpnodes_)

    hg.update(repo, parentctx.node())

    if not keep:
        if replacemap:
            ui.note(_('histedit: Should update metadata for the following '
                      'changes:\n'))

            def copybms(old, new):
                if old in tmpnodes or old in created:
                    # can't have any metadata we'd want to update
                    return
                while new in replacemap:
                    new = replacemap[new]
                ui.note(_('histedit:  %s to %s\n') % (node.short(old),
                                                      node.short(new)))
                octx = repo[old]
                marks = octx.bookmarks()
                if marks:
                    ui.note(_('histedit:     moving bookmarks %s\n') %
                              ', '.join(marks))
                    for mark in marks:
                        repo._bookmarks[mark] = new
                    bookmarks.write(repo)

            # We assume that bookmarks on the tip should remain
            # tipmost, but bookmarks on non-tip changesets should go
            # to their most reasonable successor. As a result, find
            # the old tip and new tip and copy those bookmarks first,
            # then do the rest of the bookmark copies.
            oldtip = sorted(replacemap.keys(), key=repo.changelog.rev)[-1]
            newtip = sorted(replacemap.values(), key=repo.changelog.rev)[-1]
            copybms(oldtip, newtip)

            for old, new in sorted(replacemap.iteritems()):
                copybms(old, new)
                # TODO update mq state

        ui.debug('should strip replaced nodes %s\n' %
                 ', '.join([node.hex(n)[:12] for n in replaced]))
        for n in sorted(replaced, key=lambda x: repo[x].rev()):
            try:
                repair.strip(ui, repo, n)
            except error.LookupError:
                pass

    ui.debug('should strip temp nodes %s\n' %
             ', '.join([node.hex(n)[:12] for n in tmpnodes]))
    for n in reversed(tmpnodes):
        try:
            repair.strip(ui, repo, n)
        except error.LookupError:
            pass
    os.unlink(os.path.join(repo.path, 'histedit-state'))
    if os.path.exists(repo.sjoin('undo')):
        os.unlink(repo.sjoin('undo'))


def writestate(repo, parentctxnode, created, replaced,
               tmpnodes, existing, rules, keep, oldtip, replacemap):
    fp = open(os.path.join(repo.path, 'histedit-state'), 'w')
    pickle.dump((parentctxnode, created, replaced,
                 tmpnodes, existing, rules, keep, oldtip, replacemap),
                fp)
    fp.close()

def readstate(repo):
    """Returns a tuple of (parentnode, created, replaced, tmp, existing, rules,
                           keep, oldtip, replacemap ).
    """
    fp = open(os.path.join(repo.path, 'histedit-state'))
    return pickle.load(fp)


def verifyrules(rules, repo, ctxs):
    """Verify that there exists exactly one edit rule per given changeset.

    Will abort if there are to many or too few rules, a malformed rule,
    or a rule on a changeset outside of the user-given range.
    """
    parsed = []
    if len(rules) != len(ctxs):
        raise util.Abort(_('must specify a rule for each changeset once'))
    for r in rules:
        if ' ' not in r:
            raise util.Abort(_('malformed line "%s"') % r)
        action, rest = r.split(' ', 1)
        if ' ' in rest.strip():
            ha, rest = rest.split(' ', 1)
        else:
            ha = r.strip()
        try:
            if repo[ha] not in ctxs:
                raise util.Abort(
                    _('may not use changesets other than the ones listed'))
        except error.RepoError:
            raise util.Abort(_('unknown changeset %s listed') % ha)
        if action not in actiontable:
            raise util.Abort(_('unknown action "%s"') % action)
        parsed.append([action, ha])
    return parsed


cmdtable = {
    "histedit":
        (histedit,
         [('', 'commands', '', _(
             'Read history edits from the specified file.')),
          ('c', 'continue', False, _('continue an edit already in progress')),
          ('k', 'keep', False, _(
              "don't strip old nodes after edit is complete")),
          ('', 'abort', False, _('abort an edit in progress')),
          ('o', 'outgoing', False, _('changesets not found in destination')),
          ('f', 'force', False, _(
              'force outgoing even for unrelated repositories')),
          ('r', 'rev', [], _('first revision to be edited')),
          ],
         _("[PARENT]"),
         ),
}
