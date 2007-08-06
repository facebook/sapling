# hg backend for convert extension

# Note for hg->hg conversion: Old versions of Mercurial didn't trim
# the whitespace from the ends of commit messages, but new versions
# do.  Changesets created by those older versions, then converted, may
# thus have different hashes for changesets that are otherwise
# identical.


import os, time
from mercurial.i18n import _
from mercurial.node import *
from mercurial import hg, lock, revlog, util

from common import NoRepo, commit, converter_source, converter_sink

class mercurial_sink(converter_sink):
    def __init__(self, ui, path):
        self.path = path
        self.ui = ui
        try:
            self.repo = hg.repository(self.ui, path)
        except:
            raise NoRepo("could not open hg repo %s as sink" % path)
        self.lock = None
        self.wlock = None
        self.branchnames = ui.configbool('convert', 'hg.usebranchnames', True)

    def before(self):
        self.wlock = self.repo.wlock()
        self.lock = self.repo.lock()

    def after(self):
        self.lock = None
        self.wlock = None

    def revmapfile(self):
        return os.path.join(self.path, ".hg", "shamap")

    def authorfile(self):
        return os.path.join(self.path, ".hg", "authormap")

    def getheads(self):
        h = self.repo.changelog.heads()
        return [ hex(x) for x in h ]

    def putfile(self, f, e, data):
        self.repo.wwrite(f, data, e)
        if f not in self.repo.dirstate:
            self.repo.dirstate.add(f)

    def copyfile(self, source, dest):
        self.repo.copy(source, dest)

    def delfile(self, f):
        try:
            os.unlink(self.repo.wjoin(f))
            #self.repo.remove([f])
        except:
            pass

    def putcommit(self, files, parents, commit):
        if not files:
            return hex(self.repo.changelog.tip())

        seen = {hex(nullid): 1}
        pl = []
        for p in parents:
            if p not in seen:
                pl.append(p)
                seen[p] = 1
        parents = pl

        if len(parents) < 2: parents.append("0" * 40)
        if len(parents) < 2: parents.append("0" * 40)
        p2 = parents.pop(0)

        text = commit.desc
        extra = {}
        if self.branchnames and commit.branch:
            extra['branch'] = commit.branch
        if commit.rev:
            extra['convert_revision'] = commit.rev

        while parents:
            p1 = p2
            p2 = parents.pop(0)
            a = self.repo.rawcommit(files, text, commit.author, commit.date,
                                    bin(p1), bin(p2), extra=extra)
            self.repo.dirstate.invalidate()
            text = "(octopus merge fixup)\n"
            p2 = hg.hex(self.repo.changelog.tip())

        return p2

    def puttags(self, tags):
        try:
            old = self.repo.wfile(".hgtags").read()
            oldlines = old.splitlines(1)
            oldlines.sort()
        except:
            oldlines = []

        k = tags.keys()
        k.sort()
        newlines = []
        for tag in k:
            newlines.append("%s %s\n" % (tags[tag], tag))

        newlines.sort()

        if newlines != oldlines:
            self.ui.status("updating tags\n")
            f = self.repo.wfile(".hgtags", "w")
            f.write("".join(newlines))
            f.close()
            if not oldlines: self.repo.add([".hgtags"])
            date = "%s 0" % int(time.mktime(time.gmtime()))
            self.repo.rawcommit([".hgtags"], "update tags", "convert-repo",
                                date, self.repo.changelog.tip(), nullid)
            return hex(self.repo.changelog.tip())

class mercurial_source(converter_source):
    def __init__(self, ui, path, rev=None):
        converter_source.__init__(self, ui, path, rev)
        self.repo = hg.repository(self.ui, path)
        self.lastrev = None
        self.lastctx = None

    def changectx(self, rev):
        if self.lastrev != rev:
            self.lastctx = self.repo.changectx(rev)
            self.lastrev = rev
        return self.lastctx

    def getheads(self):
        if self.rev:
            return [hex(self.repo.changectx(self.rev).node())]
        else:
            return [hex(node) for node in self.repo.heads()]

    def getfile(self, name, rev):
        try:
            return self.changectx(rev).filectx(name).data()
        except revlog.LookupError, err:
            raise IOError(err)

    def getmode(self, name, rev):
        m = self.changectx(rev).manifest()
        return (m.execf(name) and 'x' or '') + (m.linkf(name) and 'l' or '')

    def getchanges(self, rev):
        ctx = self.changectx(rev)
        m, a, r = self.repo.status(ctx.parents()[0].node(), ctx.node())[:3]
        changes = [(name, rev) for name in m + a + r]
        changes.sort()
        return (changes, self.getcopies(ctx))

    def getcopies(self, ctx):
        added = self.repo.status(ctx.parents()[0].node(), ctx.node())[1]
        copies = {}
        for name in added:
            try:
                copies[name] = ctx.filectx(name).renamed()[0]
            except TypeError:
                pass
        return copies
        
    def getcommit(self, rev):
        ctx = self.changectx(rev)
        parents = [hex(p.node()) for p in ctx.parents() if p.node() != nullid]
        return commit(author=ctx.user(), date=util.datestr(ctx.date()),
                      desc=ctx.description(), parents=parents,
                      branch=ctx.branch())

    def gettags(self):
        tags = [t for t in self.repo.tagslist() if t[0] != 'tip']
        return dict([(name, hex(node)) for name, node in tags])
