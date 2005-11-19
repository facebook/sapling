#!/usr/bin/env python
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from mercurial.demandload import demandload
demandload(globals(), "os sys sets")
from mercurial import hg

versionstr = "0.0.3"

def lookup_rev(ui, repo, rev=None):
    """returns rev or the checked-out revision if rev is None"""
    if not rev is None:
        return repo.lookup(rev)
    parents = [p for p in repo.dirstate.parents() if p != hg.nullid]
    if len(parents) != 1:
        ui.warn("unexpected number of parents\n")
        ui.warn("please commit or revert\n")
        sys.exit(1)
    return parents.pop()

def check_clean(ui, repo):
        c, a, d, u = repo.changes()
        if c or a or d:
            ui.warn("Repository is not clean, please commit or revert\n")
            sys.exit(1)

class bisect(object):
    """dichotomic search in the DAG of changesets"""
    def __init__(self, ui, repo):
        self.repo = repo
        self.path = os.path.join(repo.join(""), "bisect")
        self.ui = ui
        self.goodrevs = []
        self.badrev = None
        self.good_dirty = 0
        self.bad_dirty = 0
        self.good_path = os.path.join(self.path, "good")
        self.bad_path = os.path.join(self.path, "bad")

        s = self.good_path
        if os.path.exists(s):
            self.goodrevs = self.repo.opener(s).read().splitlines()
            self.goodrevs = [hg.bin(x) for x in self.goodrevs]
        s = self.bad_path
        if os.path.exists(s):
            r = self.repo.opener(s).read().splitlines()
            if r:
                self.badrev = hg.bin(r.pop(0))

    def __del__(self):
        if not os.path.isdir(self.path):
            return
        f = self.repo.opener(self.good_path, "w")
        f.write("\n".join([hg.hex(r) for r in  self.goodrevs]))
        if len(self.goodrevs) > 0:
            f.write("\n")
        f = self.repo.opener(self.bad_path, "w")
        if self.badrev:
            f.write(hg.hex(self.badrev) + "\n")

    def init(self):
        """start a new bisection"""
        if os.path.isdir(self.path):
            self.ui.warn("bisect directory already exists\n")
            return 1
        os.mkdir(self.path)
        check_clean(self.ui, self.repo)
        return 0

    def reset(self):
        """finish a bisection"""
        if os.path.isdir(self.path):
            sl = [self.bad_path, self.good_path]
            for s in sl:
                if os.path.exists(s):
                    os.unlink(s)
            os.rmdir(self.path)
        # Not sure about this
        #self.ui.write("Going back to tip\n")
        #self.repo.update(self.repo.changelog.tip())
        return 1

    def num_ancestors(self, head=None, stop=None):
        """
        returns a dict with the mapping:
        node -> number of ancestors (self included)
        for all nodes who are ancestor of head and
        not in stop.
        """
        if head is None:
            head = self.badrev
        return self.__ancestors_and_nb_ancestors(head, stop)[1]
        
    def ancestors(self, head=None, stop=None):
        """
        returns the set of the ancestors of head (self included)
        who are not in stop.
        """
        if head is None:
            head = self.badrev
        return self.__ancestors_and_nb_ancestors(head, stop)[0]
        
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
            for g in reversed(self.goodrevs):
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
            self.ui.warn("Unconsistent state, %s is good and bad\n"
                          % hg.hex(head))
            sys.exit(1)
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
            self.ui.warn("You should give at least one bad\n")
            sys.exit(1)
        if not self.goodrevs:
            self.ui.warn("No good revision given\n")
            self.ui.warn("Assuming the first revision is good\n")
        ancestors, num_ancestors = self.__ancestors_and_nb_ancestors(self.badrev)
        tot = len(ancestors)
        if tot == 1:
            if ancestors.pop() != self.badrev:
                self.ui.warn("Could not find the first bad revision\n")
                sys.exit(1)
            self.ui.write(
                "The first bad revision is : %s\n" % hg.hex(self.badrev))
            sys.exit(0)
        self.ui.write("%d revisions left\n" % tot)
        best_rev = None
        best_len = -1
        for n in ancestors:
            l = num_ancestors[n]
            l = min(l, tot - l)
            if l > best_len:
                best_len = l
                best_rev = n
        return best_rev

    def autonext(self):
        """find and update to the next revision to test"""
        check_clean(self.ui, self.repo)
        rev = self.next()
        self.ui.write("Now testing %s\n" % hg.hex(rev))
        return self.repo.update(rev, allow=True, force=True)

    def good(self, rev):
        self.goodrevs.append(rev)

    def autogood(self, rev=None):
        """mark revision as good and update to the next revision to test"""
        check_clean(self.ui, self.repo)
        rev = lookup_rev(self.ui, self.repo, rev)
        self.good(rev)
        if self.badrev:
            self.autonext()

    def bad(self, rev):
        self.badrev = rev

    def autobad(self, rev=None):
        """mark revision as bad and update to the next revision to test"""
        check_clean(self.ui, self.repo)
        rev = lookup_rev(self.ui, self.repo, rev)
        self.bad(rev)
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
        repo.update(new_rev, allow=True, force=True)
    for v in anc:
        if v != rev:
            ui.warn("fail to found cset! :(\n")
            return 1
    ui.write("Found bad cset: %s\n" % hg.hex(b.badrev))
    ui.write("Everything is ok :)\n")
    return 0

def bisect_run(ui, repo, cmd=None, *args):
    """bisect extension: dichotomic search in the DAG of changesets
for subcommands see "hg bisect help\"
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
        ui.write("list of subcommands for the bisect extension\n\n")
        cmds = cmdtable.keys()
        cmds.sort()
        m = max([len(c) for c in cmds])
        for cmd in cmds:
            doc = cmdtable[cmd][0].__doc__.splitlines(0)[0].rstrip()
            ui.write(" %-*s   %s\n" % (m, cmd, doc))
    
    b = bisect(ui, repo)
    bisectcmdtable = {
        "init": (b.init, 0, "hg bisect init"),
        "bad": (b.autobad, 1, "hg bisect bad [<rev>]"),
        "good": (b.autogood, 1, "hg bisect good [<rev>]"),
        "next": (b.autonext, 0, "hg bisect next"),
        "reset": (b.reset, 0, "hg bisect reset"),
        "help": (help_, 1, "hg bisect help [<subcommand>]"),
    }
            
    if not bisectcmdtable.has_key(cmd):
        ui.warn("bisect: Unknown sub-command\n")
        return help_()
    if len(args) > bisectcmdtable[cmd][1]:
        ui.warn("bisect: Too many arguments\n")
        return help_()
    return bisectcmdtable[cmd][0](*args)

cmdtable = {
    "bisect": (bisect_run, [], 
               "hg bisect [help|init|reset|next|good|bad]"),
    #"bisect-test": (test, [], "hg bisect-test rev"),
}
