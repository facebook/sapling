# bisect extension for mercurial
#
# Copyright 2005, 2006 Benoit Boissinot <benoit.boissinot@ens-lyon.org>
# Inspired by git bisect, extension skeleton taken from mq.py.
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial.i18n import _
from mercurial import hg, util, cmdutil
import os, array

def _bisect(changelog, state):
    clparents = changelog.parentrevs
    # only the earliest bad revision matters
    badrev = min([changelog.rev(n) for n in state['bad']])
    bad = changelog.node(badrev)

    # build ancestors array
    ancestors = [[]] * (changelog.count() + 1) # an extra for [-1]

    # clear good revs from array
    for node in state['good']:
        ancestors[changelog.rev(node)] = None
    for rev in xrange(changelog.count(), -1, -1):
        if ancestors[rev] is None:
            for prev in clparents(rev):
                ancestors[prev] = None

    if ancestors[badrev] is None:
        raise util.Abort(_("Inconsistent state, %s:%s is good and bad")
                         % (badrev, hg.short(bad)))

    # accumulate ancestor lists
    for rev in xrange(badrev + 1):
        if ancestors[rev] == []:
            p1, p2 = clparents(rev)
            a1, a2 = ancestors[p1], ancestors[p2]
            if a1:
                if a2:
                    # merge ancestor lists
                    a = dict.fromkeys(a2)
                    a.update(dict.fromkeys(a1))
                    a[rev] = None
                    ancestors[rev] = array.array("l", a.keys())
                else:
                    ancestors[rev] = a1 + array.array("l", [rev])
            elif a2:
                ancestors[rev] = a2 + array.array("l", [rev])
            else:
                ancestors[rev] = array.array("l", [rev])

    if badrev not in ancestors[badrev]:
        raise util.Abort(_("Could not find the first bad revision"))

    # have we narrowed it down to one entry?
    tot = len(ancestors[badrev])
    if tot == 1:
        return (bad, 0)

    # find the best node to test
    best_rev = None
    best_len = -1
    skip = dict.fromkeys([changelog.rev(n) for n in state['skip']])
    for n in ancestors[badrev]:
        if n in skip:
            continue
        a = len(ancestors[n]) # number of ancestors
        b = tot - a # number of non-ancestors
        value = min(a, b) # how good is this test?
        if value > best_len:
            best_len = value
            best_rev = n
    assert best_rev is not None
    best_node = changelog.node(best_rev)

    return (best_node, tot)

def bisect(ui, repo, rev=None, extra=None,
               reset=None, good=None, bad=None, skip=None, noupdate=None):
    """Subdivision search of changesets

This extension helps to find changesets which introduce problems.
To use, mark the earliest changeset you know exhibits the problem
as bad, then mark the latest changeset which is free from the problem
as good. Bisect will update your working directory to a revision for
testing. Once you have performed tests, mark the working directory
as bad or good and bisect will either update to another candidate
changeset or announce that it has found the bad revision.

Note: bisect expects bad revisions to be descendants of good revisions.
If you are looking for the point at which a problem was fixed, then make
the problem-free state "bad" and the problematic state "good."

    """
    # backward compatibility
    if rev in "good bad reset init".split():
        ui.warn(_("(use of 'hg bisect <cmd>' is deprecated)\n"))
        cmd, rev, extra = rev, extra, None
        if cmd == "good":
            good = True
        elif cmd == "bad":
            bad = True
        else:
            reset = True
    elif extra or good + bad + skip + reset > 1:
        raise util.Abort("Incompatible arguments")

    if reset:
        p = repo.join("bisect.state")
        if os.path.exists(p):
            os.unlink(p)
        return

    # load state
    state = {'good': [], 'bad': [], 'skip': []}
    if os.path.exists(repo.join("bisect.state")):
        for l in repo.opener("bisect.state"):
            kind, node = l[:-1].split()
            node = repo.lookup(node)
            if kind not in state:
                raise util.Abort(_("unknown bisect kind %s") % kind)
            state[kind].append(node)

    # update state
    node = repo.lookup(rev or '.')
    if good:
        state['good'].append(node)
    elif bad:
        state['bad'].append(node)
    elif skip:
        state['skip'].append(node)

    # save state
    f = repo.opener("bisect.state", "w", atomictemp=True)
    wlock = repo.wlock()
    try:
        for kind in state:
            for node in state[kind]:
                f.write("%s %s\n" % (kind, hg.hex(node)))
        f.rename()
    finally:
        del wlock

    if not state['good'] or not state['bad']:
        return

    # actually bisect
    node, changesets = _bisect(repo.changelog, state)
    if changesets == 0:
        ui.write(_("The first bad revision is:\n"))
        displayer = cmdutil.show_changeset(ui, repo, {})
        displayer.show(changenode=node)
    elif node is not None:
        # compute the approximate number of remaining tests
        tests, size = 0, 2
        while size <= changesets:
            tests, size = tests + 1, size * 2
        rev = repo.changelog.rev(node)
        ui.write(_("Testing changeset %s:%s "
                   "(%s changesets remaining, ~%s tests)\n")
                 % (rev, hg.short(node), changesets, tests))
        if not noupdate:
            cmdutil.bail_if_changed(repo)
            return hg.clean(repo, node)

cmdtable = {
    "bisect": (bisect,
               [('r', 'reset', False, _('reset bisect state')),
                ('g', 'good', False, _('mark changeset good')),
                ('b', 'bad', False, _('mark changeset bad')),
                ('s', 'skip', False, _('skip testing changeset')),
                ('U', 'noupdate', False, _('do not update to target'))],
               _("hg bisect [-gbsr] [REV]"))
}
