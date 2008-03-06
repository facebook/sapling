# hg backend for convert extension

# Notes for hg->hg conversion:
#
# * Old versions of Mercurial didn't trim the whitespace from the ends
#   of commit messages, but new versions do.  Changesets created by
#   those older versions, then converted, may thus have different
#   hashes for changesets that are otherwise identical.
#
# * By default, the source revision is stored in the converted
#   revision.  This will cause the converted revision to have a
#   different identity than the source.  To avoid this, use the
#   following option: "--config convert.hg.saverev=false"


import os, time
from mercurial.i18n import _
from mercurial.repo import RepoError
from mercurial.node import bin, hex, nullid
from mercurial import hg, revlog, util

from common import NoRepo, commit, converter_source, converter_sink

class mercurial_sink(converter_sink):
    def __init__(self, ui, path):
        converter_sink.__init__(self, ui, path)
        self.branchnames = ui.configbool('convert', 'hg.usebranchnames', True)
        self.clonebranches = ui.configbool('convert', 'hg.clonebranches', False)
        self.tagsbranch = ui.config('convert', 'hg.tagsbranch', 'default')
        self.lastbranch = None
        if os.path.isdir(path) and len(os.listdir(path)) > 0:
            try:
                self.repo = hg.repository(self.ui, path)
                if not self.repo.local():
                    raise NoRepo(_('%s is not a local Mercurial repo') % path)
            except RepoError, err:
                ui.print_exc()
                raise NoRepo(err.args[0])
        else:
            try:
                ui.status(_('initializing destination %s repository\n') % path)
                self.repo = hg.repository(self.ui, path, create=True)
                if not self.repo.local():
                    raise NoRepo(_('%s is not a local Mercurial repo') % path)
                self.created.append(path)
            except RepoError, err:
                ui.print_exc()
                raise NoRepo("could not create hg repo %s as sink" % path)
        self.lock = None
        self.wlock = None
        self.filemapmode = False

    def before(self):
        self.ui.debug(_('run hg sink pre-conversion action\n'))
        self.wlock = self.repo.wlock()
        self.lock = self.repo.lock()
        self.repo.dirstate.clear()

    def after(self):
        self.ui.debug(_('run hg sink post-conversion action\n'))
        self.repo.dirstate.invalidate()
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
            self.repo.dirstate.normallookup(f)

    def copyfile(self, source, dest):
        self.repo.copy(source, dest)

    def delfile(self, f):
        try:
            util.unlink(self.repo.wjoin(f))
            #self.repo.remove([f])
        except OSError:
            pass

    def setbranch(self, branch, pbranches):
        if not self.clonebranches:
            return

        setbranch = (branch != self.lastbranch)
        self.lastbranch = branch
        if not branch:
            branch = 'default'
        pbranches = [(b[0], b[1] and b[1] or 'default') for b in pbranches]
        pbranch = pbranches and pbranches[0][1] or 'default'

        branchpath = os.path.join(self.path, branch)
        if setbranch:
            self.after()
            try:
                self.repo = hg.repository(self.ui, branchpath)
            except:
                self.repo = hg.repository(self.ui, branchpath, create=True)
            self.before()

        # pbranches may bring revisions from other branches (merge parents)
        # Make sure we have them, or pull them.
        missings = {}
        for b in pbranches:
            try:
                self.repo.lookup(b[0])
            except:
                missings.setdefault(b[1], []).append(b[0])

        if missings:
            self.after()
            for pbranch, heads in missings.iteritems():
                pbranchpath = os.path.join(self.path, pbranch)
                prepo = hg.repository(self.ui, pbranchpath)
                self.ui.note(_('pulling from %s into %s\n') % (pbranch, branch))
                self.repo.pull(prepo, [prepo.lookup(h) for h in heads])
            self.before()

    def putcommit(self, files, parents, commit):
        seen = {}
        pl = []
        for p in parents:
            if p not in seen:
                pl.append(p)
                seen[p] = 1
        parents = pl
        nparents = len(parents)
        if self.filemapmode and nparents == 1:
            m1node = self.repo.changelog.read(bin(parents[0]))[0]
            parent = parents[0]

        if len(parents) < 2: parents.append("0" * 40)
        if len(parents) < 2: parents.append("0" * 40)
        p2 = parents.pop(0)

        text = commit.desc
        extra = commit.extra.copy()
        if self.branchnames and commit.branch:
            extra['branch'] = commit.branch
        if commit.rev:
            extra['convert_revision'] = commit.rev

        while parents:
            p1 = p2
            p2 = parents.pop(0)
            a = self.repo.rawcommit(files, text, commit.author, commit.date,
                                    bin(p1), bin(p2), extra=extra)
            self.repo.dirstate.clear()
            text = "(octopus merge fixup)\n"
            p2 = hex(self.repo.changelog.tip())

        if self.filemapmode and nparents == 1:
            man = self.repo.manifest
            mnode = self.repo.changelog.read(bin(p2))[0]
            if not man.cmp(m1node, man.revision(mnode)):
                self.repo.rollback()
                self.repo.dirstate.clear()
                return parent
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
            extra = {}
            if self.tagsbranch != 'default':
                extra['branch'] = self.tagsbranch
            try:
                tagparent = self.repo.changectx(self.tagsbranch).node()
            except RepoError, inst:
                tagparent = nullid
            self.repo.rawcommit([".hgtags"], "update tags", "convert-repo",
                                date, tagparent, nullid, extra=extra)
            return hex(self.repo.changelog.tip())

    def setfilemapmode(self, active):
        self.filemapmode = active

class mercurial_source(converter_source):
    def __init__(self, ui, path, rev=None):
        converter_source.__init__(self, ui, path, rev)
        self.saverev = ui.configbool('convert', 'hg.saverev', True)
        try:
            self.repo = hg.repository(self.ui, path)
            # try to provoke an exception if this isn't really a hg
            # repo, but some other bogus compatible-looking url
            if not self.repo.local():
                raise RepoError()
        except RepoError:
            ui.print_exc()
            raise NoRepo("%s is not a local Mercurial repo" % path)
        self.lastrev = None
        self.lastctx = None
        self._changescache = None
        self.convertfp = None

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
        if self._changescache and self._changescache[0] == rev:
            m, a, r = self._changescache[1]
        else:
            m, a, r = self.repo.status(ctx.parents()[0].node(), ctx.node())[:3]
        changes = [(name, rev) for name in m + a + r]
        changes.sort()
        return (changes, self.getcopies(ctx, m + a))

    def getcopies(self, ctx, files):
        copies = {}
        for name in files:
            try:
                copies[name] = ctx.filectx(name).renamed()[0]
            except TypeError:
                pass
        return copies

    def getcommit(self, rev):
        ctx = self.changectx(rev)
        parents = [hex(p.node()) for p in ctx.parents() if p.node() != nullid]
        if self.saverev:
            crev = rev
        else:
            crev = None
        return commit(author=ctx.user(), date=util.datestr(ctx.date()),
                      desc=ctx.description(), rev=crev, parents=parents,
                      branch=ctx.branch(), extra=ctx.extra())

    def gettags(self):
        tags = [t for t in self.repo.tagslist() if t[0] != 'tip']
        return dict([(name, hex(node)) for name, node in tags])

    def getchangedfiles(self, rev, i):
        ctx = self.changectx(rev)
        i = i or 0
        changes = self.repo.status(ctx.parents()[i].node(), ctx.node())[:3]

        if i == 0:
            self._changescache = (rev, changes)

        return changes[0] + changes[1] + changes[2]

    def converted(self, rev, destrev):
        if self.convertfp is None:
            self.convertfp = open(os.path.join(self.path, '.hg', 'shamap'),
                                  'a')
        self.convertfp.write('%s %s\n' % (destrev, rev))
        self.convertfp.flush()

    def before(self):
        self.ui.debug(_('run hg source pre-conversion action\n'))

    def after(self):
        self.ui.debug(_('run hg source post-conversion action\n'))
