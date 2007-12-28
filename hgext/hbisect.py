# bisect extension for mercurial
#
# Copyright 2005, 2006 Benoit Boissinot <benoit.boissinot@ens-lyon.org>
# Inspired by git bisect, extension skeleton taken from mq.py.
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial.i18n import _
from mercurial import hg, util, commands, cmdutil
import os, sys, sets

versionstr = "0.0.3"

def lookup_rev(ui, repo, rev=None):
    """returns rev or the checked-out revision if rev is None"""
    if not rev is None:
        return repo.lookup(rev)
    parents = [p for p in repo.dirstate.parents() if p != hg.nullid]
    if len(parents) != 1:
        raise util.Abort(_("unexpected number of parents, "
                           "please commit or revert"))
    return parents.pop()

def check_clean(ui, repo):
    modified, added, removed, deleted, unknown = repo.status()[:5]
    if modified or added or removed:
        ui.warn("Repository is not clean, please commit or revert\n")
        sys.exit(1)

class bisect(object):
    """dichotomic search in the DAG of changesets"""
    def __init__(self, ui, repo):
        self.repo = repo
        self.path = repo.join("bisect")
        self.opener = util.opener(self.path)
        self.ui = ui
        self.goodrevs = []
        self.badrev = None
        self.good_path = "good"
        self.bad_path = "bad"
        self.is_reset = False

        if os.path.exists(os.path.join(self.path, self.good_path)):
            self.goodrevs = self.opener(self.good_path).read().splitlines()
            self.goodrevs = [hg.bin(x) for x in self.goodrevs]
        if os.path.exists(os.path.join(self.path, self.bad_path)):
            r = self.opener(self.bad_path).read().splitlines()
            if r:
                self.badrev = hg.bin(r.pop(0))

    def write(self):
        if self.is_reset:
            return
        if not os.path.isdir(self.path):
            os.mkdir(self.path)
        f = self.opener(self.good_path, "w")
        f.write("\n".join([hg.hex(r) for r in  self.goodrevs]))
        if len(self.goodrevs) > 0:
            f.write("\n")
        f = self.opener(self.bad_path, "w")
        if self.badrev:
            f.write(hg.hex(self.badrev) + "\n")

    def init(self):
        """start a new bisection"""
        if os.path.isdir(self.path):
            raise util.Abort(_("bisect directory already exists\n"))
        os.mkdir(self.path)
        check_clean(self.ui, self.repo)
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

    def __ancestors_and_nb_ancestors(self, head, stop=None):
        """
        if stop is None then ancestors of goodrevs are used as
        lower limit.

        returns (anc, n_child) where anc is the set of the ancestors of head
        and n_child is a dictionary with the following mapping:
        node -> number of ancestors (self included)
        """
        cl = self.repo.changelog
        if not stop:
            stop = sets.Set([])
            for i in xrange(len(self.goodrevs)-1, -1, -1):
                g = self.goodrevs[i]
                if g in stop:
                    continue
                stop.update(cl.reachable(g))
        def num_children(a):
            """
            returns a dictionnary with the following mapping
            node -> [number of children, empty set]
            """
            d = {a: [0, sets.Set([])]}
            for i in xrange(cl.rev(a)+1):
                n = cl.node(i)
                if not d.has_key(n):
                    d[n] = [0, sets.Set([])]
                parents = [p for p in cl.parents(n) if p != hg.nullid]
                for p in parents:
                    d[p][0] += 1
            return d

        if head in stop:
            raise util.Abort(_("Inconsistent state, %s:%s is good and bad")
                             % (cl.rev(head), hg.short(head)))
        n_child = num_children(head)
        for i in xrange(cl.rev(head)+1):
            n = cl.node(i)
            parents = [p for p in cl.parents(n) if p != hg.nullid]
            for p in parents:
                n_child[p][0] -= 1
                if not n in stop:
                    n_child[n][1].union_update(n_child[p][1])
                if n_child[p][0] == 0:
                    n_child[p] = len(n_child[p][1])
            if not n in stop:
                n_child[n][1].add(n)
            if n_child[n][0] == 0:
                if n == head:
                    anc = n_child[n][1]
                n_child[n] = len(n_child[n][1])
        return anc, n_child

    def next(self):
        if not self.badrev:
            raise util.Abort(_("You should give at least one bad revision"))
        if not self.goodrevs:
            self.ui.warn(_("No good revision given\n"))
            self.ui.warn(_("Marking the first revision as good\n"))
        ancestors, num_ancestors = self.__ancestors_and_nb_ancestors(
                                    self.badrev)
        tot = len(ancestors)
        if tot == 1:
            if ancestors.pop() != self.badrev:
                raise util.Abort(_("Could not find the first bad revision"))
            self.ui.write(_("The first bad revision is:\n"))
            displayer = cmdutil.show_changeset(self.ui, self.repo, {})
            displayer.show(changenode=self.badrev)
            return None
        best_rev = None
        best_len = -1
        for n in ancestors:
            l = num_ancestors[n]
            l = min(l, tot - l)
            if l > best_len:
                best_len = l
                best_rev = n
        assert best_rev is not None
        nb_tests = 0
        q, r = divmod(tot, 2)
        while q:
            nb_tests += 1
            q, r = divmod(q, 2)
        msg = _("Testing changeset %s:%s (%s changesets remaining, "
                "~%s tests)\n") % (self.repo.changelog.rev(best_rev),
                                   hg.short(best_rev), tot, nb_tests)
        self.ui.write(msg)
        return best_rev

    def autonext(self):
        """find and update to the next revision to test"""
        rev = self.next()
        check_clean(self.ui, self.repo)
        if rev is not None:
            return hg.clean(self.repo, rev)

    def autogood(self, rev=None):
        """mark revision as good and update to the next revision to test"""
        rev = lookup_rev(self.ui, self.repo, rev)
        self.goodrevs.append(rev)
        if self.badrev:
            return self.autonext()

    def autobad(self, rev=None):
        """mark revision as bad and update to the next revision to test"""
        rev = lookup_rev(self.ui, self.repo, rev)
        self.badrev = rev
        if self.goodrevs:
            self.autonext()

# should we put it in the class ?
def test(ui, repo, rev):
    """test the bisection code"""
    b = bisect(ui, repo)
    rev = repo.lookup(rev)
    ui.write("testing with rev %s\n" % hg.hex(rev))
    anc = b.ancestors()
    while len(anc) > 1:
        if not rev in anc:
            ui.warn("failure while bisecting\n")
            sys.exit(1)
        ui.write("it worked :)\n")
        new_rev = b.next()
        ui.write("choosing if good or bad\n")
        if rev in b.ancestors(head=new_rev):
            b.bad(new_rev)
            ui.write("it is bad\n")
        else:
            b.good(new_rev)
            ui.write("it is good\n")
        anc = b.ancestors()
        #repo.update(new_rev, force=True)
    for v in anc:
        if v != rev:
            ui.warn("fail to found cset! :(\n")
            return 1
    ui.write("Found bad cset: %s\n" % hg.hex(b.badrev))
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
