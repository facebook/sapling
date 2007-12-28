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

class bisect(object):
    """dichotomic search in the DAG of changesets"""
    def __init__(self, ui, repo):
        self.repo = repo
        self.ui = ui
        self.goodnodes = []
        self.skipnodes = []
        self.badnode = None

        p = self.repo.join("bisect.state")
        if os.path.exists(p):
            for l in self.repo.opener("bisect.state"):
                type, node = l[:-1].split()
                node = self.repo.lookup(node)
                if type == "good":
                    self.goodnodes.append(node)
                elif type == "skip":
                    self.skipnodes.append(node)
                elif type == "bad":
                    self.badnode = node

    def write(self):
        f = self.repo.opener("bisect.state", "w")
        for n in self.goodnodes:
            f.write("good %s\n" % hg.hex(n))
        for n in self.skipnodes:
            f.write("skip %s\n" % hg.hex(n))
        if self.badnode:
            f.write("bad %s\n" % hg.hex(self.badnode))

    def init(self):
        """start a new bisection"""
        p = self.repo.join("bisect.state")
        if os.path.exists(p):
            os.unlink(p)

    def bisect(self):
        cl = self.repo.changelog
        clparents = cl.parentrevs
        bad = self.badnode
        badrev = cl.rev(bad)

        # build ancestors array
        ancestors = [[]] * (cl.count() + 1) # an extra for [-1]

        # clear goodnodes from array
        for good in self.goodnodes:
            ancestors[cl.rev(good)] = None
        for rev in xrange(cl.count(), -1, -1):
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
            return (self.badnode, 0)

        # find the best node to test
        best_rev = None
        best_len = -1
        skip = dict.fromkeys([cl.rev(s) for s in self.skipnodes])
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
        best_node = cl.node(best_rev)

        return (best_node, tot)

    def next(self):
        """find and update to the next revision to test"""
        if self.goodnodes and self.badnode:
            node, changesets = self.bisect()

            if changesets == 0:
                self.ui.write(_("The first bad revision is:\n"))
                displayer = cmdutil.show_changeset(self.ui, self.repo, {})
                displayer.show(changenode=node)
            elif node is not None:
                # compute the approximate number of remaining tests
                tests, size = 0, 2
                while size <= changesets:
                    tests, size = tests + 1, size * 2
                rev = self.repo.changelog.rev(node)
                self.ui.write(_("Testing changeset %s:%s "
                                "(%s changesets remaining, ~%s tests)\n")
                              % (rev, hg.short(node), changesets, tests))
                cmdutil.bail_if_changed(self.repo)
                return hg.clean(self.repo, node)

    def good(self, rev=None):
        """mark revision as good and update to the next revision to test"""
        self.goodnodes.append(self.repo.lookup(rev or '.'))
        self.write()
        return self.next()

    def skip(self, rev=None):
        """mark revision as skipped and update to the next revision to test"""
        self.skipnodes.append(self.repo.lookup(rev or '.'))
        self.write()
        return self.next()

    def bad(self, rev=None):
        """mark revision as bad and update to the next revision to test"""
        self.badnode = self.repo.lookup(rev or '.')
        self.write()
        return self.next()

def bisect_run(ui, repo, cmd=None, *args):
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

For subcommands see "hg bisect help\"
    """
    def help_(cmd=None, *args):
        """show help for a given bisect subcommand or all subcommands"""
        cmdtable = bisectcmdtable
        if cmd:
            doc = cmdtable[cmd][0].__doc__
            synopsis = cmdtable[cmd][2]
            ui.write(synopsis + "\n")
            ui.write("\n" + doc + "\n")
            return
        ui.write(_("list of subcommands for the bisect extension\n\n"))
        cmds = cmdtable.keys()
        cmds.sort()
        m = max([len(c) for c in cmds])
        for cmd in cmds:
            doc = cmdtable[cmd][0].__doc__.splitlines(0)[0].rstrip()
            ui.write(" %-*s   %s\n" % (m, cmd, doc))

    b = bisect(ui, repo)
    bisectcmdtable = {
        "init": (b.init, 0, _("hg bisect init")),
        "bad": (b.bad, 1, _("hg bisect bad [<rev>]")),
        "good": (b.good, 1, _("hg bisect good [<rev>]")),
        "skip": (b.skip, 1, _("hg bisect skip [<rev>]")),
        "next": (b.next, 0, _("hg bisect next")),
        "help": (help_, 1, _("hg bisect help [<subcommand>]")),
    }

    if cmd == "reset":
        cmd = "init"

    if not bisectcmdtable.has_key(cmd):
        ui.warn(_("bisect: Unknown sub-command\n"))
        return help_()
    if len(args) > bisectcmdtable[cmd][1]:
        ui.warn(_("bisect: Too many arguments\n"))
        return help_()
    ret = bisectcmdtable[cmd][0](*args)
    return ret

cmdtable = {
    "bisect": (bisect_run, [], _("hg bisect [help|init|reset|next|good|bad]")),
    #"bisect-test": (test, [], "hg bisect-test rev"),
}
