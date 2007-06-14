# convert.py Foreign SCM converter
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from common import NoRepo
from cvs import convert_cvs
from git import convert_git
from hg import convert_mercurial

import os
from mercurial import hg, ui, util, commands

commands.norepo += " convert"

converters = [convert_cvs, convert_git, convert_mercurial]

def converter(ui, path):
    if not os.path.isdir(path):
        raise util.Abort("%s: not a directory" % path)
    for c in converters:
        try:
            return c(ui, path)
        except NoRepo:
            pass
    raise util.Abort("%s: unknown repository type" % path)

class convert(object):
    def __init__(self, ui, source, dest, mapfile, opts):

        self.source = source
        self.dest = dest
        self.ui = ui
        self.opts = opts
        self.commitcache = {}
        self.mapfile = mapfile
        self.mapfilefd = None
        self.authors = {}
        self.authorfile = None

        self.map = {}
        try:
            origmapfile = open(self.mapfile, 'r')
            for l in origmapfile:
                sv, dv = l[:-1].split()
                self.map[sv] = dv
            origmapfile.close()
        except IOError:
            pass

        # Read first the dst author map if any
        authorfile = self.dest.authorfile()
        if authorfile and os.path.exists(authorfile):
            self.readauthormap(authorfile)
        # Extend/Override with new author map if necessary
        if opts.get('authors'):
            self.readauthormap(opts.get('authors'))
            self.authorfile = self.dest.authorfile()

    def walktree(self, heads):
        visit = heads
        known = {}
        parents = {}
        while visit:
            n = visit.pop(0)
            if n in known or n in self.map: continue
            known[n] = 1
            self.commitcache[n] = self.source.getcommit(n)
            cp = self.commitcache[n].parents
            for p in cp:
                parents.setdefault(n, []).append(p)
                visit.append(p)

        return parents

    def toposort(self, parents):
        visit = parents.keys()
        seen = {}
        children = {}

        while visit:
            n = visit.pop(0)
            if n in seen: continue
            seen[n] = 1
            pc = 0
            if n in parents:
                for p in parents[n]:
                    if p not in self.map: pc += 1
                    visit.append(p)
                    children.setdefault(p, []).append(n)
            if not pc: root = n

        s = []
        removed = {}
        visit = children.keys()
        while visit:
            n = visit.pop(0)
            if n in removed: continue
            dep = 0
            if n in parents:
                for p in parents[n]:
                    if p in self.map: continue
                    if p not in removed:
                        # we're still dependent
                        visit.append(n)
                        dep = 1
                        break

            if not dep:
                # all n's parents are in the list
                removed[n] = 1
                if n not in self.map:
                    s.append(n)
                if n in children:
                    for c in children[n]:
                        visit.insert(0, c)

        if self.opts.get('datesort'):
            depth = {}
            for n in s:
                depth[n] = 0
                pl = [p for p in self.commitcache[n].parents
                      if p not in self.map]
                if pl:
                    depth[n] = max([depth[p] for p in pl]) + 1

            s = [(depth[n], self.commitcache[n].date, n) for n in s]
            s.sort()
            s = [e[2] for e in s]

        return s

    def mapentry(self, src, dst):
        if self.mapfilefd is None:
            try:
                self.mapfilefd = open(self.mapfile, "a")
            except IOError, (errno, strerror):
                raise util.Abort("Could not open map file %s: %s, %s\n" % (self.mapfile, errno, strerror))
        self.map[src] = dst
        self.mapfilefd.write("%s %s\n" % (src, dst))
        self.mapfilefd.flush()

    def writeauthormap(self):
        authorfile = self.authorfile
        if authorfile:
           self.ui.status('Writing author map file %s\n' % authorfile)
           ofile = open(authorfile, 'w+')
           for author in self.authors:
               ofile.write("%s=%s\n" % (author, self.authors[author]))
           ofile.close()

    def readauthormap(self, authorfile):
        afile = open(authorfile, 'r')
        for line in afile:
            try:
                srcauthor = line.split('=')[0].strip()
                dstauthor = line.split('=')[1].strip()
                if srcauthor in self.authors and dstauthor != self.authors[srcauthor]:
                    self.ui.status(
                        'Overriding mapping for author %s, was %s, will be %s\n'
                        % (srcauthor, self.authors[srcauthor], dstauthor))
                else:
                    self.ui.debug('Mapping author %s to %s\n'
                                  % (srcauthor, dstauthor))
                    self.authors[srcauthor] = dstauthor
            except IndexError:
                self.ui.warn(
                    'Ignoring bad line in author file map %s: %s\n'
                    % (authorfile, line))
        afile.close()

    def copy(self, rev):
        c = self.commitcache[rev]
        files = self.source.getchanges(rev)

        for f, v in files:
            try:
                data = self.source.getfile(f, v)
            except IOError, inst:
                self.dest.delfile(f)
            else:
                e = self.source.getmode(f, v)
                self.dest.putfile(f, e, data)

        r = [self.map[v] for v in c.parents]
        f = [f for f, v in files]
        newnode = self.dest.putcommit(f, r, c)
        self.mapentry(rev, newnode)

    def convert(self):
        try:
            self.ui.status("scanning source...\n")
            heads = self.source.getheads()
            parents = self.walktree(heads)
            self.ui.status("sorting...\n")
            t = self.toposort(parents)
            num = len(t)
            c = None

            self.ui.status("converting...\n")
            for c in t:
                num -= 1
                desc = self.commitcache[c].desc
                if "\n" in desc:
                    desc = desc.splitlines()[0]
                author = self.commitcache[c].author
                author = self.authors.get(author, author)
                self.commitcache[c].author = author
                self.ui.status("%d %s\n" % (num, desc))
                self.copy(c)

            tags = self.source.gettags()
            ctags = {}
            for k in tags:
                v = tags[k]
                if v in self.map:
                    ctags[k] = self.map[v]

            if c and ctags:
                nrev = self.dest.puttags(ctags)
                # write another hash correspondence to override the previous
                # one so we don't end up with extra tag heads
                if nrev:
                    self.mapentry(c, nrev)

            self.writeauthormap()
        finally:
            self.cleanup()

    def cleanup(self):
       if self.mapfilefd:
           self.mapfilefd.close()

def _convert(ui, src, dest=None, mapfile=None, **opts):
    '''Convert a foreign SCM repository to a Mercurial one.

    Accepted source formats:
    - GIT
    - CVS

    Accepted destination formats:
    - Mercurial

    If destination isn't given, a new Mercurial repo named <src>-hg will
    be created. If <mapfile> isn't given, it will be put in a default
    location (<dest>/.hg/shamap by default)

    The <mapfile> is a simple text file that maps each source commit ID to
    the destination ID for that revision, like so:
    <source ID> <destination ID>

    If the file doesn't exist, it's automatically created.  It's updated
    on each commit copied, so convert-repo can be interrupted and can
    be run repeatedly to copy new commits.

    The [username mapping] file is a simple text file that maps each source
    commit author to a destination commit author. It is handy for source SCMs
    that use unix logins to identify authors (eg: CVS). One line per author
    mapping and the line format is:
    srcauthor=whatever string you want
    '''

    srcc = converter(ui, src)
    if not hasattr(srcc, "getcommit"):
        raise util.Abort("%s: can't read from this repo type" % src)

    if not dest:
        dest = src + "-hg"
        ui.status("assuming destination %s\n" % dest)

    # Try to be smart and initalize things when required
    if os.path.isdir(dest):
        if len(os.listdir(dest)) > 0:
            try:
                hg.repository(ui, dest)
                ui.status("destination %s is a Mercurial repository\n" % dest)
            except hg.RepoError:
                raise util.Abort(
                    "destination directory %s is not empty.\n"
                    "Please specify an empty directory to be initialized\n"
                    "or an already initialized mercurial repository"
                    % dest)
        else:
            ui.status("initializing destination %s repository\n" % dest)
            hg.repository(ui, dest, create=True)
    elif os.path.exists(dest):
        raise util.Abort("destination %s exists and is not a directory" % dest)
    else:
        ui.status("initializing destination %s repository\n" % dest)
        hg.repository(ui, dest, create=True)

    destc = converter(ui, dest)
    if not hasattr(destc, "putcommit"):
        raise util.Abort("%s: can't write to this repo type" % src)

    if not mapfile:
        try:
            mapfile = destc.mapfile()
        except:
            mapfile = os.path.join(destc, "map")

    c = convert(ui, srcc, destc, mapfile, opts)
    c.convert()

cmdtable = {
    "convert":
        (_convert,
         [('A', 'authors', '', 'username mapping filename'),
          ('', 'datesort', None, 'try to sort changesets by date')],
         'hg convert [OPTION]... SOURCE [DEST [MAPFILE]]'),
}
