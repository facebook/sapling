# bisect extension for mercurial
#
# Copyright 2005, 2006 Benoit Boissinot <benoit.boissinot@ens-lyon.org>
# Inspired by git bisect, extension skeleton taken from mq.py.
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial.i18n import _
from mercurial import hg, util, commands, cmdutil
import os, sys

class bisect(object):
    """dichotomic search in the DAG of changesets"""
    def __init__(self, ui, repo):
        self.repo = repo
        self.path = repo.join("bisect")
        self.opener = util.opener(self.path)
        self.ui = ui
        self.goodnodes = []
        self.badnode = None
        self.good_path = "good"
        self.bad_path = "bad"
        self.is_reset = False

        if os.path.exists(os.path.join(self.path, self.good_path)):
            self.goodnodes = self.opener(self.good_path).read().splitlines()
            self.goodnodes = [hg.bin(x) for x in self.goodnodes]
        if os.path.exists(os.path.join(self.path, self.bad_path)):
            r = self.opener(self.bad_path).read().splitlines()
            if r:
                self.badnode = hg.bin(r.pop(0))

    def write(self):
        if self.is_reset:
            return
        if not os.path.isdir(self.path):
            os.mkdir(self.path)
        f = self.opener(self.good_path, "w")
        f.write("\n".join([hg.hex(r) for r in  self.goodnodes]))
        if len(self.goodnodes) > 0:
            f.write("\n")
        f = self.opener(self.bad_path, "w")
        if self.badnode:
            f.write(hg.hex(self.badnode) + "\n")

    def init(self):
        """start a new bisection"""
        if os.path.isdir(self.path):
            raise util.Abort(_("bisect directory already exists\n"))
        os.mkdir(self.path)
        return 0

    def reset(self):
        """finish a bisection"""
        if os.path.isdir(self.path):
            sl = [os.path.join(self.path, p)
                  for p in [self.bad_path, self.good_path]]
            for s in sl:
                if os.path.exists(s):
                    os.unlink(s)
            os.rmdir(self.path)
        # Not sure about this
        #self.ui.write("Going back to tip\n")
        #self.repo.update(self.repo.changelog.tip())
        self.is_reset = True
        return 0

    def bisect(self):
        cl = self.repo.changelog
        clparents = cl.parentrevs
        bad = self.badnode
        badrev = cl.rev(bad)

        if not bad:
            raise util.Abort(_("You should give at least one bad revision"))
        if not self.goodnodes:
            self.ui.warn(_("No good revision given\n"))
            self.ui.warn(_("Marking the first revision as good\n"))

        # build ancestors array
        ancestors = [{}] * (cl.count() + 1) # an extra for [-1]

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
            if ancestors[rev] == {}:
                ancestors[rev] = {rev: 1}
                for p in clparents(rev):
                    if ancestors[p]:
                        # add parent ancestors to our ancestors
                        ancestors[rev].update(ancestors[p])

        if badrev not in ancestors[badrev]:
            raise util.Abort(_("Could not find the first bad revision"))

        # have we narrowed it down to one entry?
        tot = len(ancestors[badrev])
        if tot == 1:
            self.ui.write(_("The first bad revision is:\n"))
            displayer = cmdutil.show_changeset(self.ui, self.repo, {})
            displayer.show(changenode=self.badnode)
            return None

        # find the best node to test
        best_rev = None
        best_len = -1
        for n in ancestors[badrev]:
            a = len(ancestors[n]) # number of ancestors
            b = tot - a # number of non-ancestors
            value = min(a, b) # how good is this test?
            if value > best_len:
                best_len = value
                best_rev = n
        assert best_rev is not None
        best_node = cl.node(best_rev)

        # compute the approximate number of remaining tests
        nb_tests = 0
        q, r = divmod(tot, 2)
        while q:
            nb_tests += 1
            q, r = divmod(q, 2)

        self.ui.write(_("Testing changeset %s:%s "
                        "(%s changesets remaining, ~%s tests)\n")
                      % (best_rev, hg.short(best_node), tot, nb_tests))
        return best_node

    def autonext(self):
        """find and update to the next revision to test"""
        node = self.bisect()
        if node is not None:
            cmdutil.bail_if_changed(self.repo)
            return hg.clean(self.repo, node)

    def autogood(self, rev=None):
        """mark revision as good and update to the next revision to test"""
        self.goodnodes.append(self.repo.lookup(rev or '.'))
        if self.badnode:
            return self.autonext()

    def autobad(self, rev=None):
        """mark revision as bad and update to the next revision to test"""
        self.badnode = self.repo.lookup(rev or '.')
        if self.goodnodes:
            self.autonext()

# should we put it in the class ?
def test(ui, repo, rev):
    """test the bisection code"""
    b = bisect(ui, repo)
    node = repo.lookup(rev)
    ui.write("testing with rev %s\n" % hg.hex(node))
    anc = b.ancestors()
    while len(anc) > 1:
        if not node in anc:
            ui.warn("failure while bisecting\n")
            sys.exit(1)
        ui.write("it worked :)\n")
        new_node = b.next()
        ui.write("choosing if good or bad\n")
        if node in b.ancestors(head=new_node):
            b.bad(new_node)
            ui.write("it is bad\n")
        else:
            b.good(new_node)
            ui.write("it is good\n")
        anc = b.ancestors()
        #repo.update(new_node, force=True)
    for v in anc:
        if v != node:
            ui.warn("fail to found cset! :(\n")
            return 1
    ui.write("Found bad cset: %s\n" % hg.hex(b.badnode))
    ui.write("Everything is ok :)\n")
    return 0

def bisect_run(ui, repo, cmd=None, *args):
    """Dichotomic search in the DAG of changesets

This extension helps to find changesets which cause problems.
To use, mark the earliest changeset you know introduces the problem
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
        "bad": (b.autobad, 1, _("hg bisect bad [<rev>]")),
        "good": (b.autogood, 1, _("hg bisect good [<rev>]")),
        "next": (b.autonext, 0, _("hg bisect next")),
        "reset": (b.reset, 0, _("hg bisect reset")),
        "help": (help_, 1, _("hg bisect help [<subcommand>]")),
    }

    if not bisectcmdtable.has_key(cmd):
        ui.warn(_("bisect: Unknown sub-command\n"))
        return help_()
    if len(args) > bisectcmdtable[cmd][1]:
        ui.warn(_("bisect: Too many arguments\n"))
        return help_()
    ret = bisectcmdtable[cmd][0](*args)
    b.write()
    return ret

cmdtable = {
    "bisect": (bisect_run, [], _("hg bisect [help|init|reset|next|good|bad]")),
    #"bisect-test": (test, [], "hg bisect-test rev"),
}
