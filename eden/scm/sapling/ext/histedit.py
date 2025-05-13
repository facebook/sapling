# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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

If you were to run ``@prog@ histedit c561b4e977df``, you would see the following
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
 #  r, roll = like fold, but discard this commit's description and date
 #  d, drop = remove commit from history
 #  m, mess = edit commit message without changing commit content
 #  b, base = checkout changeset and apply further changesets from there
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
 #  r, roll = like fold, but discard this commit's description and date
 #  d, drop = remove commit from history
 #  m, mess = edit commit message without changing commit content
 #  b, base = checkout changeset and apply further changesets from there
 #

At which point you close the editor and ``histedit`` starts working. When you
specify a ``fold`` operation, ``histedit`` will open an editor when it folds
those revisions together, offering you a chance to clean up the commit message::

 Add beta
 ***
 Add delta

Edit the commit message to your liking, then close the editor. The date used
for the commit will be the later of the two commits' dates. For this example,
let's assume that the commit message was changed to ``Add beta and delta.``
After histedit has run and had a chance to remove any old or temporary
revisions it needed, the history looks like this::

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
allowing you to edit files freely, or even use ``@prog@ record`` to commit
some changes as a separate commit. When you're done, any remaining
uncommitted changes will be committed as well. When done, run ``@prog@
histedit --continue`` to finish this step. If there are uncommitted
changes, you'll be prompted for a new commit message, but the default
commit message will be the original message for the ``edit`` ed
revision, and the date of the original commit will be preserved.

The ``message`` operation will give you a chance to revise a commit
message without changing the contents. It's a shortcut for doing
``edit`` immediately followed by `@prog@ histedit --continue``.

If ``histedit`` encounters a conflict when moving a revision (while
handling ``pick`` or ``fold``), it'll stop in a similar manner to
``edit`` with the difference that it won't prompt you for a commit
message when done. If you decide at this point that you don't like how
much work it will be to rearrange history, or that you made a mistake,
you can use ``@prog@ histedit --abort`` to abandon the new changes you
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

Autoverb
--------
Autoverb is an experimental feature that uses the first lines of commit
messages to pre-construct your histedit plan. An autoverb commit message
begins with a four-letter command from the list above, then !, then
optionally the first line of another commit message to set order.

For example, given the following history:

 @  3[tip]   7c2fd3b9020c   2009-04-27 18:04 -0500   durin42
 |    roll! Add beta
 |
 o  2   030b686bedc4   2009-04-27 18:04 -0500   durin42
 |    mess! Add gamma
 |
 o  1   c561b4e977df   2009-04-27 18:04 -0500   durin42
 |    Add beta
 |
 o  0   d8d2fcd0e319   2009-04-27 18:04 -0500   durin42
      Add alpha

``@prog@ histedit c561b4e977df`` would construct the histedit plan below and
present it to you for final fixes before you close the editor:

 pick c561b4e977df Add beta
 roll 7c2fd3b9020c roll! Add beta
 mess 030b686bedc4 mess! Add gamma

 # Edit history between c561b4e977df and 7c2fd3b9020c
 #
 # Commits are listed from least to most recent
 #
 # Commands:
 #  p, pick = use commit
 #  e, edit = use commit, but stop for amending
 #  f, fold = use commit, but combine it with the one above
 #  r, roll = like fold, but discard this commit's description and date
 #  d, drop = remove commit from history
 #  m, mess = edit commit message without changing commit content
 #  b, base = checkout changeset and apply further changesets from there
 #

This lets you use ordinary ``@prog@ commit`` commands to build up a set of changes
to histedit into place, then ``@prog@ histedit`` when you are done.

Config
------

Histedit rule lines are truncated to 80 characters by default. You
can customize this behavior by setting a different length in your
configuration file::

  [histedit]
  linelen = 120      # truncate rule lines at 120 characters

``@prog@ histedit`` attempts to automatically choose an appropriate base
revision to use. To change which base revision is used, define a
revset in your configuration file::

  [histedit]
  defaultrev = only(.) & draft()

By default each edited revision needs to be present in histedit commands.
To remove revision you need to use ``drop`` operation. You can configure
the drop to be implicit for missing commits by adding::

  [histedit]
  dropmissing = True

By default, histedit will close the transaction after each action. For
performance purposes, you can configure histedit to use a single transaction
across the entire histedit. WARNING: This setting introduces a significant risk
of losing the work you've done in a histedit if the histedit aborts
unexpectedly::

  [histedit]
  singletransaction = True

If you wish to use autoverb, you will need to enable it:

  [experimental]
  histedit.autoverb = True

"""

import errno
import os

from sapling import (
    bundle2,
    cmdutil,
    context,
    copies,
    destutil,
    error,
    exchange,
    extensions,
    hg,
    lock,
    match as matchmod,
    merge as mergemod,
    mergeutil,
    mutation,
    node,
    progress,
    registrar,
    repair,
    scmutil,
    util,
    visibility,
)
from sapling.i18n import _

# pyre-fixme[11]: Annotation `pickle` is not defined as a type.
pickle = util.pickle
release = lock.release
cmdtable = {}
command = registrar.command(cmdtable)


# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = "ships-with-hg-core"

actiontable = {}
primaryactions = set()
secondaryactions = set()
tertiaryactions = set()
internalactions = set()


def geteditcomment(ui, first, last):
    """construct the editor comment
    The comment includes::
     - an intro
     - sorted primary commands
     - sorted short commands
     - sorted long commands
     - additional hints

    Commands are only included once.
    """
    intro = _(
        """Edit history between %s and %s

Commits are listed from least to most recent

You can reorder changesets by reordering the lines

Commands:
"""
    )
    actions = []

    def addverb(v):
        a = actiontable[v]
        lines = a.message.split("\n")
        if len(a.verbs):
            v = ", ".join(sorted(a.verbs, key=lambda v: len(v)))
        actions.append(" %s = %s" % (v, lines[0]))
        actions.extend(["  %s" for l in lines[1:]])

    for v in (
        sorted(primaryactions) + sorted(secondaryactions) + sorted(tertiaryactions)
    ):
        addverb(v)
    actions.append("")

    hints = []
    if ui.configbool("histedit", "dropmissing"):
        hints.append(
            "Deleting a changeset from the list "
            "will DISCARD it from the edited history!"
        )

    lines = (intro % (first, last)).split("\n") + actions + hints

    return "".join(["# %s\n" % l if l else "#\n" for l in lines])


class histeditstate:
    def __init__(
        self,
        repo,
        parentctxnode=None,
        actions=None,
        keep=None,
        topmost=None,
        replacements=None,
        lock=None,
        wlock=None,
    ):
        self.repo = repo
        self.actions = actions
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
            state = self.repo.localvfs.read("histedit-state").decode()
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            cmdutil.wrongtooltocontinue(self.repo, _("histedit"))

        if state.startswith("v1\n"):
            data = self._load()
            parentctxnode, rules, keep, topmost, replacements, backupfile = data
        else:
            data = pickle.loads(state)
            parentctxnode, rules, keep, topmost, replacements = data
            backupfile = None

        self.parentctxnode = parentctxnode
        rules = "\n".join(["%s %s" % (verb, rest) for [verb, rest] in rules])
        actions = parserules(rules, self)
        self.actions = actions
        self.keep = keep
        self.topmost = topmost
        self.replacements = replacements
        self.backupfile = backupfile

    def write(self, tr=None):
        if tr:
            tr.addfilegenerator(
                "histedit-state", ("histedit-state",), self._write, location="local"
            )
        else:
            with self.repo.localvfs("histedit-state", "w") as f:
                self._write(f)

    def _write(self, fp):
        writeutf8(fp, "v1\n")
        writeutf8(fp, "%s\n" % node.hex(self.parentctxnode))
        writeutf8(fp, "%s\n" % node.hex(self.topmost))
        writeutf8(fp, "%s\n" % self.keep)
        writeutf8(fp, "%d\n" % len(self.actions))
        for action in self.actions:
            writeutf8(fp, "%s\n" % action.tostate())
        writeutf8(fp, "%d\n" % len(self.replacements))
        for replacement in self.replacements:
            writeutf8(
                fp,
                "%s%s\n"
                % (
                    node.hex(replacement[0]),
                    "".join(node.hex(r) for r in replacement[1]),
                ),
            )
        backupfile = self.backupfile
        if not backupfile:
            backupfile = ""
        writeutf8(fp, "%s\n" % backupfile)

    def _load(self):
        fp = self.repo.localvfs("histedit-state", "r")
        lines = [l[:-1].decode() for l in fp.readlines()]

        index = 0
        lines[index]  # version number
        index += 1

        parentctxnode = node.bin(lines[index])
        index += 1

        topmost = node.bin(lines[index])
        index += 1

        keep = lines[index] == "True"
        index += 1

        # Rules
        rules = []
        rulelen = int(lines[index])
        index += 1
        for i in range(rulelen):
            ruleaction = lines[index]
            index += 1
            rule = lines[index]
            index += 1
            rules.append((ruleaction, rule))

        # Replacements
        replacements = []
        replacementlen = int(lines[index])
        index += 1
        for i in range(replacementlen):
            replacement = lines[index]
            original = node.bin(replacement[:40])
            succ = [
                node.bin(replacement[i : i + 40])
                for i in range(40, len(replacement), 40)
            ]
            replacements.append((original, succ))
            index += 1

        backupfile = lines[index]
        index += 1

        fp.close()

        return parentctxnode, rules, keep, topmost, replacements, backupfile

    def clear(self):
        if self.inprogress():
            self.repo.localvfs.unlink("histedit-state")

    def inprogress(self):
        return self.repo.localvfs.exists("histedit-state")


class histeditaction:
    def __init__(self, state, node):
        self.state = state
        self.repo = state.repo
        self.node = node

    @classmethod
    def fromrule(cls, state, rule):
        """Parses the given rule, returning an instance of the histeditaction."""
        rulehash = rule.strip().split(" ", 1)[0]
        try:
            rev = node.bin(rulehash)
        except TypeError:
            raise error.ParseError("invalid changeset %s" % rulehash)
        return cls(state, rev)

    def verify(self, prev, expected, seen):
        """Verifies semantic correctness of the rule"""
        repo = self.repo
        ha = node.hex(self.node)
        try:
            self.node = repo[ha].node()
        except error.RepoError:
            raise error.ParseError(_("unknown changeset %s listed") % ha[:12])
        if self.node is not None:
            self._verifynodeconstraints(prev, expected, seen)

    def _verifynodeconstraints(self, prev, expected, seen):
        # by default command need a node in the edited list
        if self.node not in expected:
            raise error.ParseError(
                _('%s "%s" changeset was not a candidate')
                % (self.verb, node.short(self.node)),
                hint=_("only use listed changesets"),
            )
        # and only one command per node
        if self.node in seen:
            raise error.ParseError(
                _("duplicated command for changeset %s") % node.short(self.node)
            )

    def torule(self):
        """build a histedit rule line for an action

        by default lines are in the form:
        <hash> <rev> <summary>
        """
        ctx = self.repo[self.node]
        summary = _getsummary(ctx)
        line = "%s %s %s" % (self.verb, ctx, summary)
        # trim to 75 columns by default so it's not stupidly wide in my editor
        # (the 5 more are left for verb)
        maxlen = self.repo.ui.configint("histedit", "linelen")
        maxlen = max(maxlen, 22)  # avoid truncating hash
        return util.ellipsis(line, maxlen)

    def tostate(self):
        """Print an action in format used by histedit state files
        (the first line is a verb, the remainder is the second)
        """
        return "%s\n%s" % (self.verb, node.hex(self.node))

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
        repo.ui.pushbuffer(error=True, labeled=True)
        with repo.wlock(), repo.lock(), repo.transaction("histedit"):
            hg.update(repo, self.state.parentctxnode, quietempty=True)
            stats = applychanges(repo.ui, repo, rulectx, {})
        if stats and stats[3] > 0:
            buf = repo.ui.popbuffer()
            repo.ui.write(*buf)
            raise error.InterventionRequired(
                _("Fix up the change (%s %s)") % (self.verb, node.short(self.node)),
                hint=_("@prog@ histedit --continue to resume"),
            )
        else:
            repo.ui.popbuffer()

    def continuedirty(self):
        """Continues the action when changes have been applied to the working
        copy. The default behavior is to commit the dirty changes."""
        repo = self.repo
        rulectx = repo[self.node]

        editor = self.commiteditor()
        commit = commitfuncfor(repo, rulectx)

        commit(
            text=rulectx.description(),
            user=rulectx.user(),
            date=rulectx.date(),
            extra=rulectx.extra(),
            editor=editor,
        )

    def commiteditor(self):
        """The editor to be used to edit the commit message."""
        return False

    def continueclean(self):
        """Continues the action when the working copy is clean. The default
        behavior is to accept the current commit as the new version of the
        rulectx."""
        ctx = self.repo["."]
        if ctx.node() == self.state.parentctxnode:
            self.repo.ui.warn(
                _("%s: skipping changeset (no changes)\n") % node.short(self.node)
            )
            return ctx, [(self.node, tuple())]
        if ctx.node() == self.node:
            # Nothing changed
            return ctx, []
        return ctx, [(self.node, (ctx.node(),))]


def writeutf8(fp, text):
    fp.write(text.encode())


def commitfuncfor(repo, src):
    """Build a commit function for the replacement of <src>

    This function ensure we apply the same treatment to all changesets.

    - Add a 'histedit_source' entry in extra.

    Note that fold has its own separated logic because its handling is a bit
    different and not easily factored out of the fold method.
    """
    phasemin = src.phase()

    def commitfunc(**kwargs):
        overrides = {("phases", "new-commit"): phasemin}
        with repo.ui.configoverride(overrides, "histedit"):
            extra = kwargs.get(r"extra", {}).copy()
            extra["histedit_source"] = src.hex()
            kwargs[r"mutinfo"] = mutation.record(repo, extra, [src.node()], "histedit")
            kwargs[r"extra"] = extra
            kwargs[r"loginfo"] = {"predecessors": src.hex(), "mutation": "histedit"}
            return repo.commit(**kwargs)

    return commitfunc


def applychanges(ui, repo, ctx, opts):
    """Merge changeset from ctx (only) in the current working directory"""
    with repo.wlock(), repo.lock(), repo.transaction("histedit"):
        wcpar = repo.dirstate.p1()
        if ctx.p1().node() == wcpar:
            # edits are "in place" we do not need to make any merge,
            # just applies changes on parent for editing
            cmdutil.revert(ui, repo, ctx, (wcpar, node.nullid), all=True)
            stats = None
        else:
            with repo.ui.configoverride(
                {("ui", "forcemerge"): opts.get("tool", "")}, "histedit"
            ):
                stats = mergemod.graft(repo, ctx, ctx.p1(), ["local", "histedit"])
    return stats


def collapse(repo, first, commitopts, skipprompt=False):
    """collapse the set of revisions from first to the working context one as new one.

    Expected commit options are:
        - message
        - date
        - username
    Commit message is edited in all cases.

    This function works in memory."""
    last = repo[None]
    ctxs = list(repo.set("%n::.", first.node())) + [last]
    if not ctxs:
        return None
    for c in ctxs:
        if not c.mutable():
            raise error.ParseError(
                _("cannot fold into public change %s") % node.short(c.node())
            )
    base = first.p1()

    # commit a new version of the old changeset, including the update
    # collect all files which might be affected
    all_files = set()
    for ctx in ctxs:
        all_files.update(ctx.files())

    # Recompute copies (avoid recording a -> b -> a)
    copied = copies.pathcopies(base, last)

    if "remotefilelog" in repo.requirements:
        # Prefetch files in `base` to avoid serial lookups.
        fileids = base.manifest().walkfiles(matchmod.exact("", "", all_files))
        repo.fileslog.filestore.prefetch(fileids)
        repo.fileslog.metadatastore.prefetch(fileids, length=1)

    # prune files which were reverted by the updates
    files = [f for f in all_files if not cmdutil.samefile(f, last, base)]
    # commit version of these files as defined by head
    headmf = last.manifest()

    def filectxfn(repo, ctx, path):
        if path in headmf:
            fctx = last[path]
            return context.overlayfilectx(fctx, ctx=ctx, copied=copied.get(path, False))
        return None

    if commitopts.get("message"):
        message = commitopts["message"]
    else:
        message = first.description()
    user = commitopts.get("user")
    date = commitopts.get("date")
    extra = commitopts.get("extra")
    mutinfo = commitopts.get("mutinfo")

    parents = (first.p1(), first.p2())
    editor = None
    if not skipprompt:
        editor = cmdutil.getcommiteditor(edit=True, editform="histedit.fold")

    loginfo = {"predecessors": " ".join(c.hex() for c in ctxs), "mutation": "histedit"}

    new = context.memctx(
        repo,
        parents=parents,
        text=message,
        files=files,
        filectxfn=filectxfn,
        user=user,
        date=date,
        extra=extra,
        editor=editor,
        loginfo=loginfo,
        mutinfo=mutinfo,
    )
    n = repo.commitctx(new)

    # Similar logic to "dirstate.rebuild()", but don't leave untracked files.
    with repo.dirstate.parentchange():
        for f in all_files:
            if f in new:
                repo.dirstate.normal(f)
            else:
                repo.dirstate.delete(f)
        repo.dirstate.setparents(n)

    return n


def _isdirtywc(repo):
    return repo[None].dirty(missing=True)


def abortdirty():
    raise error.Abort(
        _("working copy has pending changes"),
        hint=_(
            "amend, commit, or revert them and run histedit "
            "--continue, or abort with histedit --abort"
        ),
    )


def action(verbs, message, priority=False, internal=False):
    def wrap(cls):
        assert not priority or not internal
        verb = verbs[0]
        if priority:
            primaryactions.add(verb)
        elif internal:
            internalactions.add(verb)
        elif len(verbs) > 1:
            secondaryactions.add(verb)
        else:
            tertiaryactions.add(verb)

        cls.verb = verb
        cls.verbs = verbs
        cls.message = message
        for verb in verbs:
            actiontable[verb] = cls
        return cls

    return wrap


@action(["pick", "p"], _("use commit"), priority=True)
class pick(histeditaction):
    def run(self):
        rulectx = self.repo[self.node]
        if rulectx.p1().node() == self.state.parentctxnode:
            self.repo.ui.debug("node %s unchanged\n" % node.short(self.node))
            return rulectx, []

        return super(pick, self).run()


@action(["edit", "e"], _("use commit, but stop for amending"), priority=True)
class edit(histeditaction):
    def run(self):
        repo = self.repo
        rulectx = repo[self.node]
        with repo.wlock(), repo.lock(), repo.transaction("revert"):
            hg.update(repo, self.state.parentctxnode, quietempty=True)
            applychanges(repo.ui, repo, rulectx, {})
        raise error.InterventionRequired(
            _("Editing (%s), you may commit or record as needed now.")
            % node.short(self.node),
            hint=_("@prog@ histedit --continue to resume"),
        )

    def commiteditor(self):
        return cmdutil.getcommiteditor(edit=True, editform="histedit.edit")


@action(["fold", "f"], _("use commit, but combine it with the one above"))
class fold(histeditaction):
    def __init__(self, state, node):
        super(fold, self).__init__(state, node)
        self.collapsedctx = None
        self.replacements = None

    def verify(self, prev, expected, seen):
        """Verifies semantic correctness of the fold rule"""
        super(fold, self).verify(prev, expected, seen)
        repo = self.repo
        if not prev:
            c = repo[self.node].p1()
        elif not prev.verb in ("pick", "base"):
            return
        else:
            c = repo[prev.node]
        if not c.mutable():
            raise error.ParseError(
                _("cannot fold into public change %s") % node.short(c.node())
            )

    def continuedirty(self):
        self.collapsedctx, self.replacements = self.finishfold()

    def continueclean(self):
        if self.collapsedctx is None:
            return self.finishfold()
        return self.collapsedctx, self.replacements

    def skipprompt(self):
        """Returns true if the rule should skip the message editor.

        For example, 'fold' wants to show an editor, but 'rollup'
        doesn't want to.
        """
        return False

    def mergedescs(self):
        """Returns true if the rule should merge messages of multiple changes.

        This exists mainly so that 'rollup' rules can be a subclass of
        'fold'.
        """
        return True

    def firstdate(self):
        """Returns true if the rule should preserve the date of the first
        change.

        This exists mainly so that 'rollup' rules can be a subclass of
        'fold'.
        """
        return False

    def finishfold(self):
        repo = self.repo
        parentctxnode = self.state.parentctxnode
        ctx = repo[parentctxnode]
        oldctx = repo[self.node]
        wctx = repo[None]
        if len(wctx.files()) == 0:
            self.repo.ui.warn(_("%s: empty changeset\n") % node.short(self.node))
            return ctx, [(self.node, (parentctxnode,))]
        newnodes = list(repo.nodes("(%n::. - %n)", parentctxnode, parentctxnode))
        ### prepare new commit data
        commitopts = {}
        commitopts["user"] = ctx.user()
        # commit message
        if not self.mergedescs():
            newmessage = ctx.description()
        else:
            newmessage = (
                "\n***\n".join(
                    [ctx.description()]
                    + [repo[r].description() for r in newnodes]
                    + [oldctx.description()]
                )
                + "\n"
            )
        commitopts["message"] = newmessage
        # date
        if self.firstdate():
            commitopts["date"] = ctx.date()
        else:
            commitopts["date"] = max(ctx.date(), oldctx.date())
        extra = ctx.extra().copy()
        # histedit_source
        # note: ctx is likely a temporary commit but that the best we can do
        #       here. This is sufficient to solve issue3681 anyway.
        extra["histedit_source"] = "%s,%s" % (ctx.hex(), oldctx.hex())
        # mutation predecessors - ctx is likely an intermediate commit, but its
        # predecessors will refer to the original commits.
        preds = [ctx.node()] + newnodes + [oldctx.node()]
        commitopts["mutinfo"] = mutation.record(repo, extra, preds, "histedit")
        commitopts["extra"] = extra
        phasemin = max(ctx.phase(), oldctx.phase())
        overrides = {("phases", "new-commit"): phasemin}
        with repo.ui.configoverride(overrides, "histedit"):
            n = collapse(repo, ctx, commitopts, skipprompt=self.skipprompt())
        if n is None:
            return ctx, []
        mergemod.mergestate.read(repo).reset()
        replacements = [
            (oldctx.node(), (n,)),
            (ctx.node(), (n,)),
        ]
        for ich in newnodes:
            replacements.append((ich, (n,)))
        return repo[n], replacements


@action(["base", "b"], _("checkout changeset and apply further changesets from there"))
class base(histeditaction):
    def run(self):
        if self.repo["."].node() != self.node:
            with (
                self.repo.wlock(),
                self.repo.lock(),
                self.repo.transaction("histedit-base"),
            ):
                mergemod.goto(self.repo, self.node, force=True)
        return self.continueclean()

    def continuedirty(self):
        abortdirty()

    def continueclean(self):
        basectx = self.repo["."]
        return basectx, []

    def _verifynodeconstraints(self, prev, expected, seen):
        # base can only be use with a node not in the edited set
        if self.node in expected:
            msg = _('%s "%s" changeset was an edited list candidate')
            raise error.ParseError(
                msg % (self.verb, node.short(self.node)),
                hint=_("base must only use unlisted changesets"),
            )


@action(
    ["_multifold"],
    _(
        """fold subclass used for when multiple folds happen in a row

    We only want to fire the editor for the folded message once when
    (say) four changes are folded down into a single change. This is
    similar to rollup, but we should preserve both messages so that
    when the last fold operation runs we can show the user all the
    commit messages in their editor.
    """
    ),
    internal=True,
)
class _multifold(fold):
    def skipprompt(self):
        return True


@action(["roll", "r"], _("like fold, but discard this commit's description and date"))
class rollup(fold):
    def mergedescs(self):
        return False

    def skipprompt(self):
        return True

    def firstdate(self):
        return True


@action(["drop", "d"], _("remove commit from history"))
class drop(histeditaction):
    def run(self):
        parentctx = self.repo[self.state.parentctxnode]
        return parentctx, [(self.node, tuple())]


@action(
    ["mess", "m"],
    _("edit commit message without changing commit content"),
    priority=True,
)
class message(histeditaction):
    def commiteditor(self):
        return cmdutil.getcommiteditor(edit=True, editform="histedit.mess")


@command(
    "histedit",
    [
        (
            "",
            "commands",
            "",
            _("read history edits from the specified file"),
            _("FILE"),
        ),
        ("c", "continue", False, _("continue an edit already in progress")),
        ("", "edit-plan", False, _("edit remaining actions list")),
        ("k", "keep", False, _("don't strip old nodes after edit is complete")),
        ("", "abort", False, _("abort an edit in progress")),
        ("r", "rev", [], _("first revision to be edited"), _("REV")),
    ]
    + cmdutil.formatteropts,
    _("[OPTION]... [ANCESTOR]"),
    legacyaliases=["histe", "histed", "histedi"],
)
def histedit(ui, repo, *freeargs, **opts):
    """interactively reorder, combine, or delete commits

    This command lets you edit a linear series of commits up to
    and including the working copy, which should be clean.
    You can:

    - `pick` to (re)order a commit

    - `drop` to omit a commit

    - `mess` to reword a commit message

    - `fold` to combine a commit with the preceding commit, using the later date

    - `roll` like fold, but discarding this commit's description and date

    - `edit` to edit a commit, preserving date

    - `base` to checkout a commit and continue applying subsequent commits

    There are multiple ways to select the root changeset:

    - Specify ANCESTOR directly

    - Otherwise, the value from the ``histedit.defaultrev`` config option
      is used as a revset to select the base commit when ANCESTOR is not
      specified. The first commit returned by the revset is used. By
      default, this selects the editable history that is unique to the
      ancestry of the working directory.

    .. container:: verbose

       Examples:

         - A number of changes have been made.
           Commit `a113a4006` is no longer needed.

           Start history editing from commit a::

             @prog@ histedit -r a113a4006

           An editor opens, containing the list of commits,
           with specific actions specified::

             pick a113a4006 Zworgle the foobar
             pick 822478b68 Bedazzle the zerlog
             pick d275e7ed9 5 Morgify the cromulancy

           Additional information about the possible actions
           to take appears below the list of commits.

           To remove commit a113a4006 from the history,
           its action (at the beginning of the relevant line)
           is changed to ``drop``::

             drop a113a4006 Zworgle the foobar
             pick 822478b68 Bedazzle the zerlog
             pick d275e7ed9 Morgify the cromulancy

         - A number of changes have been made.
           Commit fe2bff2ce and c9116c09e need to be swapped.

           Start history editing from commit fe2bff2ce::

             @prog@ histedit -r fe2bff2ce

           An editor opens, containing the list of commits,
           with specific actions specified::

             pick fe2bff2ce Blorb a morgwazzle
             pick 99a93da65 Zworgle the foobar
             pick c9116c09e Bedazzle the zerlog

           To swap commits fe2bff2ce and c9116c09e, simply swap their lines::

             pick 8ef592ce7cc4 4 Bedazzle the zerlog
             pick 5339bf82f0ca 3 Zworgle the foobar
             pick 252a1af424ad 2 Blorb a morgwazzle

    Returns 0 on success, 1 if user intervention is required for
    ``edit`` command or to resolve merge conflicts.
    """
    state = histeditstate(repo)
    try:
        state.wlock = repo.wlock()
        state.lock = repo.lock()
        _histedit(ui, repo, state, *freeargs, **opts)
    finally:
        release(state.lock, state.wlock)


goalcontinue = "continue"
goalabort = "abort"
goaleditplan = "edit-plan"
goalnew = "new"


def _getgoal(opts):
    if opts.get("continue"):
        return goalcontinue
    if opts.get("abort"):
        return goalabort
    if opts.get("edit_plan"):
        return goaleditplan
    return goalnew


def _readfile(ui, path):
    if path == "-":
        with ui.timeblockedsection("histedit"):
            return ui.fin.read().decode()
    else:
        with open(path, "rb") as f:
            return f.read().decode()


def _validateargs(ui, repo, state, freeargs, opts, goal, rules, revs):
    # basic argument incompatibility processing
    editplan = opts.get("edit_plan")
    abort = opts.get("abort")
    if goal == "continue":
        if any((abort, revs, freeargs, rules, editplan)):
            raise error.Abort(_("no arguments allowed with --continue"))
    elif goal == "abort":
        if any((revs, freeargs, rules, editplan)):
            raise error.Abort(_("no arguments allowed with --abort"))
    elif goal == "edit-plan":
        if any((revs, freeargs)):
            raise error.Abort(_("only --commands argument allowed with --edit-plan"))
    else:
        if os.path.exists(os.path.join(repo.path, "histedit-state")):
            raise error.Abort(
                _("history edit already in progress, try --continue or --abort")
            )
        revs.extend(freeargs)
        if len(revs) == 0:
            defaultrev = destutil.desthistedit(ui, repo)
            if defaultrev is not None:
                revs.append(repo[defaultrev].hex())

        if len(revs) != 1:
            raise error.Abort(_("histedit requires exactly one ancestor revision"))


def _histedit(ui, repo, state, *freeargs, **opts):
    fm = ui.formatter("histedit", opts)
    fm.startitem()
    goal = _getgoal(opts)
    revs = opts.get("rev", [])
    rules = opts.get("commands", "")
    state.keep = opts.get("keep", False)

    _validateargs(ui, repo, state, freeargs, opts, goal, rules, revs)

    # rebuild state
    if goal == goalcontinue:
        state.read()
        state = bootstrapcontinue(ui, state, opts)
    elif goal == goaleditplan:
        _edithisteditplan(ui, repo, state, rules)
        return
    elif goal == goalabort:
        _aborthistedit(ui, repo, state)
        return
    else:
        # goal == goalnew
        _newhistedit(ui, repo, state, revs, freeargs, opts)

    _continuehistedit(ui, repo, state)
    _finishhistedit(ui, repo, state, fm)
    fm.end()


def _continuehistedit(ui, repo, state):
    """This function runs after either:
    - bootstrapcontinue (if the goal is 'continue')
    - _newhistedit (if the goal is 'new')
    """
    # preprocess rules so that we can hide inner folds from the user
    # and only show one editor
    actions = state.actions[:]
    for idx, (action, nextact) in enumerate(zip(actions, actions[1:] + [None])):
        if action.verb == "fold" and nextact and nextact.verb == "fold":
            state.actions[idx].__class__ = _multifold

    # Force an initial state file write, so the user can run --abort/continue
    # even if there's an exception before the first transaction serialize.
    state.write()

    total = len(state.actions)
    pos = 0
    tr = None
    # Don't use singletransaction by default since it rolls the entire
    # transaction back if an unexpected exception happens (like a
    # pretxncommit hook throws, or the user aborts the commit msg editor).
    if ui.configbool("histedit", "singletransaction"):
        # Don't use a 'with' for the transaction, since actions may close
        # and reopen a transaction. For example, if the action executes an
        # external process it may choose to commit the transaction first.
        tr = repo.transaction("histedit")
    with progress.bar(ui, _("editing"), _("changes"), total) as prog:
        with util.acceptintervention(tr):
            while state.actions:
                state.write(tr=tr)
                actobj = state.actions[0]
                pos += 1
                prog.value = (pos, actobj.torule())
                ui.debug(
                    "histedit: processing %s %s\n" % (actobj.verb, actobj.torule())
                )
                parentctx, replacement_ = actobj.run()
                state.parentctxnode = parentctx.node()
                state.replacements.extend(replacement_)
                state.actions.pop(0)

        state.write()


def _finishhistedit(ui, repo, state, fm):
    """This action runs when histedit is finishing its session"""
    repo.ui.pushbuffer()
    with repo.transaction("histedit"):
        hg.update(repo, state.parentctxnode, quietempty=True)
    repo.ui.popbuffer()

    mapping, tmpnodes, created, ntm = processreplacement(state)
    if mapping:
        for prec, succs in mapping.items():
            if not succs:
                ui.debug("histedit: %s is dropped\n" % node.short(prec))
            else:
                ui.debug(
                    "histedit: %s is replaced by %s\n"
                    % (node.short(prec), node.short(succs[0]))
                )
                if len(succs) > 1:
                    m = "histedit:                            %s"
                    for n in succs[1:]:
                        ui.debug(m % node.short(n))

    if not state.keep:
        if mapping:
            movetopmostbookmarks(repo, state.topmost, ntm)
    else:
        mapping = {}

    for n in tmpnodes:
        mapping[n] = ()

    # remove entries about unknown nodes
    nodemap = repo.changelog.nodemap
    mapping = {
        k: v
        for k, v in mapping.items()
        if k in nodemap and all(n in nodemap for n in v)
    }
    scmutil.cleanupnodes(repo, mapping, "histedit")
    hf = fm.hexfunc
    fl = fm.formatlist
    fd = fm.formatdict
    nodechanges = fd(
        {
            hf(oldn): fl([hf(n) for n in newn], name="node")
            for oldn, newn in mapping.items()
        },
        key="oldnode",
        value="newnodes",
    )
    fm.data(nodechanges=nodechanges)

    state.clear()
    if os.path.exists(repo.sjoin("undo")):
        os.unlink(repo.sjoin("undo"))
    if repo.localvfs.exists("histedit-last-edit.txt"):
        repo.localvfs.unlink("histedit-last-edit.txt")


def _aborthistedit(ui, repo, state):
    try:
        state.read()
        __, leafs, tmpnodes, __ = processreplacement(state)
        ui.debug("restore wc to old parent %s\n" % node.short(state.topmost))

        # Recover our old commits if necessary
        if not state.topmost in repo and state.backupfile:
            backupfile = repo.localvfs.join(state.backupfile)
            f = hg.openpath(ui, backupfile)
            gen = exchange.readbundle(ui, f, backupfile)
            with repo.transaction("histedit.abort") as tr:
                bundle2.applybundle(
                    repo, gen, tr, source="histedit", url="bundle:" + backupfile
                )

            os.remove(backupfile)

        # check whether we should update away
        unfi = repo
        revs = list(unfi.revs("%ln::", leafs | tmpnodes))
        if unfi.revs("parents() and (%n  or %ld)", state.parentctxnode, revs):
            with repo.transaction("histedit.abort") as tr:
                hg.clean(unfi, state.topmost, show_stats=True, quietempty=True)

        nodes = list(map(unfi.changelog.node, revs))
        scmutil.cleanupnodes(repo, nodes, "histedit")
    except Exception:
        if state.inprogress():
            ui.warn(
                _(
                    "warning: encountered an exception during histedit "
                    "--abort; the repository may not have been completely "
                    "cleaned up\n"
                )
            )
        raise
    finally:
        state.clear()


def _edithisteditplan(ui, repo, state, rules):
    state.read()
    if not rules:
        comment = geteditcomment(
            ui, node.short(state.parentctxnode), node.short(state.topmost)
        )
        rules = ruleeditor(repo, ui, state.actions, comment)
    else:
        rules = _readfile(ui, rules)
    actions = parserules(rules, state)
    ctxs = [repo[act.node] for act in state.actions if act.node]
    warnverifyactions(ui, repo, actions, state, ctxs)
    state.actions = actions
    state.write()


def _newhistedit(ui, repo, state, revs, freeargs, opts):
    rules = opts.get("commands", "")

    cmdutil.checkunfinished(repo)
    cmdutil.bailifchanged(repo)

    topmost, empty = repo.dirstate.parents()
    rr = list(repo.set("roots(%ld)", scmutil.revrange(repo, revs)))
    if len(rr) != 1:
        raise error.Abort(
            _("The specified revisions must have exactly one common root")
        )
    root = rr[0].node()

    revs = between(repo, root, topmost, state.keep)
    if not revs:
        raise error.Abort(
            _("%s is not an ancestor of working directory") % node.short(root)
        )

    ctxs = [repo[r] for r in revs]
    if not rules:
        comment = geteditcomment(ui, node.short(root), node.short(topmost))
        actions = [pick(state, r) for r in revs]
        rules = ruleeditor(repo, ui, actions, comment)
    else:
        rules = _readfile(ui, rules)
    actions = parserules(rules, state)
    warnverifyactions(ui, repo, actions, state, ctxs)

    parentctxnode = repo[root].p1().node()

    state.parentctxnode = parentctxnode
    state.actions = actions
    state.topmost = topmost
    state.replacements = []

    ui.log(
        "histedit",
        "%d actions to histedit",
        len(actions),
        histedit_num_actions=len(actions),
    )

    # Create a backup so we can always abort completely.
    backupfile = None
    if not mutation.enabled(repo):
        backupfile = repair._bundle(repo, [parentctxnode], [topmost], root, "histedit")
    state.backupfile = backupfile


def _getsummary(ctx):
    # a common pattern is to extract the summary but default to the empty
    # string
    summary = ctx.description() or ""
    if summary:
        summary = summary.splitlines()[0]
    return summary


def bootstrapcontinue(ui, state, opts):
    repo = state.repo

    ms = mergemod.mergestate.read(repo)
    mergeutil.checkunresolved(ms)

    if state.actions:
        actobj = state.actions.pop(0)

        if _isdirtywc(repo):
            actobj.continuedirty()
            if _isdirtywc(repo):
                abortdirty()

        parentctx, replacements = actobj.continueclean()

        state.parentctxnode = parentctx.node()
        state.replacements.extend(replacements)

    return state


def between(repo, old, new, keep):
    """select and validate the set of revision to edit

    When keep is false, the specified set can't have children."""
    ctxs = list(repo.set("%n::%n", old, new))
    if ctxs and not keep:
        if not (visibility.tracking(repo)) and repo.revs("(%ld::) - (%ld)", ctxs, ctxs):
            raise error.Abort(
                _("can only histedit a changeset together with all its descendants")
            )
        if repo.revs("(%ld) and merge()", ctxs):
            raise error.Abort(_("cannot edit history that contains merges"))
        root = ctxs[0]  # list is already sorted by repo.set
        if not root.mutable():
            raise error.Abort(
                _("cannot edit public changeset: %s") % root,
                hint=_("see '@prog@ help phases' for details"),
            )
    return [c.node() for c in ctxs]


def ruleeditor(repo, ui, actions, editcomment=""):
    """open an editor to edit rules

    rules are in the format [ [act, ctx], ...] like in state.rules
    """
    if repo.ui.configbool("experimental", "histedit.autoverb"):
        newact = util.sortdict()
        for act in actions:
            ctx = repo[act.node]
            summary = _getsummary(ctx)
            fword = summary.split(" ", 1)[0].lower()
            added = False

            # if it doesn't end with the special character '!' just skip this
            if fword.endswith("!"):
                fword = fword[:-1]
                if fword in primaryactions | secondaryactions | tertiaryactions:
                    act.verb = fword
                    # get the target summary
                    tsum = summary[len(fword) + 1 :].lstrip()
                    # safe but slow: reverse iterate over the actions so we
                    # don't clash on two commits having the same summary
                    for na, l in reversed(list(newact.items())):
                        actx = repo[na.node]
                        asum = _getsummary(actx)
                        if asum == tsum:
                            added = True
                            l.append(act)
                            break

            if not added:
                newact[act] = []

        # copy over and flatten the new list
        actions = []
        for na, l in newact.items():
            actions.append(na)
            actions += l

    rules = "\n".join([act.torule() for act in actions])
    rules += "\n\n"
    rules += editcomment
    rules = ui.edit(
        rules,
        ui.username(),
        {"prefix": "histedit"},
        repopath=repo.path,
        action="histedit",
    )

    # Save edit rules in .hg/histedit-last-edit.txt in case
    # the user needs to ask for help after something
    # surprising happens.
    f = open(repo.localvfs.join("histedit-last-edit.txt"), "w")
    f.write(rules)
    f.close()

    return rules


def parserules(rules, state):
    """Read the histedit rules string and return list of action objects"""
    rules = [
        l
        for l in (r.strip() for r in rules.splitlines())
        if l and not l.startswith("#")
    ]
    actions = []
    for r in rules:
        if " " not in r:
            raise error.ParseError(_('malformed line "%s"') % r)
        verb, rest = r.split(" ", 1)

        if verb not in actiontable:
            raise error.ParseError(_('unknown action "%s"') % verb)

        action = actiontable[verb].fromrule(state, rest)
        actions.append(action)
    return actions


def warnverifyactions(ui, repo, actions, state, ctxs):
    try:
        verifyactions(actions, state, ctxs)
    except error.ParseError:
        if repo.localvfs.exists("histedit-last-edit.txt"):
            ui.warn(_("warning: histedit rules saved to: .hg/histedit-last-edit.txt\n"))
        raise


def verifyactions(actions, state, ctxs):
    """Verify that there exists exactly one action per given changeset and
    other constraints.

    Will abort if there are to many or too few rules, a malformed rule,
    or a rule on a changeset outside of the user-given range.
    """
    expected = set(c.node() for c in ctxs)
    seen = set()
    prev = None

    if actions and actions[0].verb in ["roll", "fold"]:
        raise error.ParseError(
            _('first changeset cannot use verb "%s"') % actions[0].verb
        )

    for action in actions:
        action.verify(prev, expected, seen)
        prev = action
        if action.node is not None:
            seen.add(action.node)
    missing = sorted(expected - seen)  # sort to stabilize output

    if state.repo.ui.configbool("histedit", "dropmissing"):
        if len(actions) == 0:
            raise error.ParseError(
                _("no rules provided"), hint=_("use strip extension to remove commits")
            )

        drops = [drop(state, n) for n in missing]
        # put the in the beginning so they execute immediately and
        # don't show in the edit-plan in the future
        actions[:0] = drops
    elif missing:
        raise error.ParseError(
            _("missing rules for changeset %s") % node.short(missing[0]),
            hint=_(
                'use "drop %s" to discard, see also: '
                "'@prog@ help -e histedit.config'"
            )
            % node.short(missing[0]),
        )


def adjustreplacementsfrommutation(repo, oldreplacements):
    """Adjust replacements from commit mutation

    The replacements structure is originally generated based on histedit's
    state and does not account for changes that are not recorded there.  This
    function fixes that by adding data read from commit mutation records.
    """
    unfi = repo
    newreplacements = list(oldreplacements)
    oldsuccs = [r[1] for r in oldreplacements]
    # successors that have already been added to succstocheck once
    seensuccs = set().union(*oldsuccs)  # create a set from an iterable of tuples
    succstocheck = list(seensuccs)
    while succstocheck:
        n = succstocheck.pop()
        succsets = mutation.lookupsuccessors(unfi, n)
        if succsets:
            for succset in succsets:
                newreplacements.append((n, succset))
                for succ in succset:
                    if succ not in seensuccs:
                        seensuccs.add(succ)
                        succstocheck.append(succ)
        elif n not in repo:
            newreplacements.append((n, ()))
    return newreplacements


def processreplacement(state):
    """process the list of replacements to return

    1) the final mapping between original and created nodes
    2) the list of temporary node created by histedit
    3) the list of new commit created by histedit"""
    if mutation.enabled(state.repo):
        replacements = adjustreplacementsfrommutation(state.repo, state.replacements)
    else:
        replacements = state.replacements
    allsuccs = set()
    replaced = set()
    fullmapping = {}
    # initialize basic set
    # fullmapping records all operations recorded in replacement
    for rep in replacements:
        allsuccs.update(rep[1])
        replaced.add(rep[0])
        fullmapping.setdefault(rep[0], set()).update(rep[1])
    new = allsuccs - replaced
    tmpnodes = allsuccs & replaced
    # Reduce content fullmapping into direct relation between original nodes
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


def movetopmostbookmarks(repo, oldtopmost, newtopmost):
    """Move bookmark from oldtopmost to newly created topmost

    This is arguably a feature and we may only want that for the active
    bookmark. But the behavior is kept compatible with the old version for now.
    """
    if not oldtopmost or not newtopmost:
        return
    oldbmarks = repo.nodebookmarks(oldtopmost)
    if oldbmarks:
        with repo.lock(), repo.transaction("histedit") as tr:
            marks = repo._bookmarks
            changes = []
            for name in oldbmarks:
                changes.append((name, newtopmost))
            marks.applychanges(repo, tr, changes)


def stripwrapper(orig, ui, repo, nodelist, *args, **kwargs):
    if isinstance(nodelist, str):
        nodelist = [nodelist]
    if os.path.exists(os.path.join(repo.path, "histedit-state")):
        state = histeditstate(repo)
        state.read()
        histedit_nodes = {action.node for action in state.actions if action.node}
        common_nodes = histedit_nodes & set(nodelist)
        if common_nodes:
            raise error.Abort(
                _("histedit in progress, can't strip %s")
                % ", ".join(node.short(x) for x in common_nodes)
            )
    return orig(ui, repo, nodelist, *args, **kwargs)


extensions.wrapfunction(repair, "strip", stripwrapper)


def summaryhook(ui, repo):
    if not os.path.exists(repo.localvfs.join("histedit-state")):
        return
    state = histeditstate(repo)
    state.read()
    if state.actions:
        # i18n: column positioning for "hg summary"
        ui.write(
            _("hist:   %s (histedit --continue)\n")
            % (ui.label(_("%d remaining"), "histedit.remaining") % len(state.actions))
        )


def extsetup(ui):
    cmdutil.summaryhooks.add("histedit", summaryhook)
    cmdutil.afterresolvedstates.append(
        ("histedit-state", _("@prog@ histedit --continue"))
    )
