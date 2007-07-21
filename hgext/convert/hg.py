# hg backend for convert extension

import os, time
from mercurial import hg

from common import NoRepo, converter_sink

class convert_mercurial(converter_sink):
    def __init__(self, ui, path):
        self.path = path
        self.ui = ui
        try:
            self.repo = hg.repository(self.ui, path)
        except:
            raise NoRepo("could open hg repo %s" % path)

    def mapfile(self):
        return os.path.join(self.path, ".hg", "shamap")

    def authorfile(self):
        return os.path.join(self.path, ".hg", "authormap")

    def getheads(self):
        h = self.repo.changelog.heads()
        return [ hg.hex(x) for x in h ]

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
        seen = {}
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
        if commit.branch:
            extra['branch'] = commit.branch
        if commit.rev:
            extra['convert_revision'] = commit.rev

        while parents:
            p1 = p2
            p2 = parents.pop(0)
            a = self.repo.rawcommit(files, text, commit.author, commit.date,
                                    hg.bin(p1), hg.bin(p2), extra=extra)
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
                                date, self.repo.changelog.tip(), hg.nullid)
            return hg.hex(self.repo.changelog.tip())
