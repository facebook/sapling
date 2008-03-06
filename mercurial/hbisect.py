# changelog bisection for mercurial
#
# Copyright 2007 Matt Mackall
# Copyright 2005, 2006 Benoit Boissinot <benoit.boissinot@ens-lyon.org>
# Inspired by git bisect, extension skeleton taken from mq.py.
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from i18n import _
from node import short
import util

def bisect(changelog, state):
    clparents = changelog.parentrevs
    skip = dict.fromkeys([changelog.rev(n) for n in state['skip']])

    def buildancestors(bad, good):
        # only the earliest bad revision matters
        badrev = min([changelog.rev(n) for n in bad])
        goodrevs = [changelog.rev(n) for n in good]
        # build ancestors array
        ancestors = [[]] * (changelog.count() + 1) # an extra for [-1]

        # clear good revs from array
        for node in goodrevs:
            ancestors[node] = None
        for rev in xrange(changelog.count(), -1, -1):
            if ancestors[rev] is None:
                for prev in clparents(rev):
                    ancestors[prev] = None

        if ancestors[badrev] is None:
            return badrev, None
        return badrev, ancestors

    good = 0
    badrev, ancestors = buildancestors(state['bad'], state['good'])
    if not ancestors: # looking for bad to good transition?
        good = 1
        badrev, ancestors = buildancestors(state['good'], state['bad'])
    bad = changelog.node(badrev)
    if not ancestors: # now we're confused
        raise util.Abort(_("Inconsistent state, %s:%s is good and bad")
                         % (badrev, short(bad)))

    # build children dict
    children = {}
    visit = [badrev]
    candidates = []
    while visit:
        rev = visit.pop(0)
        if ancestors[rev] == []:
            candidates.append(rev)
            for prev in clparents(rev):
                if prev != -1:
                    if prev in children:
                        children[prev].append(rev)
                    else:
                        children[prev] = [rev]
                        visit.append(prev)

    candidates.sort()
    # have we narrowed it down to one entry?
    tot = len(candidates)
    if tot == 1:
        return (bad, 0, good)
    perfect = tot / 2

    # find the best node to test
    best_rev = None
    best_len = -1
    poison = {}
    for rev in candidates:
        if rev in poison:
            for c in children.get(rev, []):
                poison[c] = True # poison children
            continue

        a = ancestors[rev] or [rev]
        ancestors[rev] = None

        x = len(a) # number of ancestors
        y = tot - x # number of non-ancestors
        value = min(x, y) # how good is this test?
        if value > best_len and rev not in skip:
            best_len = value
            best_rev = rev
            if value == perfect: # found a perfect candidate? quit early
                break

        if y < perfect: # all downhill from here?
            for c in children.get(rev, []):
                poison[c] = True # poison children
            continue

        for c in children.get(rev, []):
            if ancestors[c]:
                ancestors[c] = dict.fromkeys(ancestors[c] + a).keys()
            else:
                ancestors[c] = a + [c]

    assert best_rev is not None
    best_node = changelog.node(best_rev)

    return (best_node, tot, good)
