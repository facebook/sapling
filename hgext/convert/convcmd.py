# convcmd - convert extension commands definition
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from common import NoRepo, MissingTool, SKIPREV, mapfile
from cvs import convert_cvs
from darcs import darcs_source
from git import convert_git
from hg import mercurial_source, mercurial_sink
from subversion import debugsvnlog, svn_source, svn_sink
from monotone import monotone_source
from gnuarch import gnuarch_source
import filemap

import os, shutil
from mercurial import hg, util
from mercurial.i18n import _

orig_encoding = 'ascii'

def recode(s):
    if isinstance(s, unicode):
        return s.encode(orig_encoding, 'replace')
    else:
        return s.decode('utf-8').encode(orig_encoding, 'replace')

source_converters = [
    ('cvs', convert_cvs),
    ('git', convert_git),
    ('svn', svn_source),
    ('hg', mercurial_source),
    ('darcs', darcs_source),
    ('mtn', monotone_source),
    ('gnuarch', gnuarch_source),
    ]

sink_converters = [
    ('hg', mercurial_sink),
    ('svn', svn_sink),
    ]

def convertsource(ui, path, type, rev):
    exceptions = []
    for name, source in source_converters:
        try:
            if not type or name == type:
                return source(ui, path, rev)
        except (NoRepo, MissingTool), inst:
            exceptions.append(inst)
    if not ui.quiet:
        for inst in exceptions:
            ui.write(_("%s\n") % inst)
    raise util.Abort('%s: unknown repository type' % path)

def convertsink(ui, path, type):
    for name, sink in sink_converters:
        try:
            if not type or name == type:
                return sink(ui, path)
        except NoRepo, inst:
            ui.note(_("convert: %s\n") % inst)
    raise util.Abort('%s: unknown repository type' % path)

class converter(object):
    def __init__(self, ui, source, dest, revmapfile, opts):

        self.source = source
        self.dest = dest
        self.ui = ui
        self.opts = opts
        self.commitcache = {}
        self.authors = {}
        self.authorfile = None

        self.map = mapfile(ui, revmapfile)

        # Read first the dst author map if any
        authorfile = self.dest.authorfile()
        if authorfile and os.path.exists(authorfile):
            self.readauthormap(authorfile)
        # Extend/Override with new author map if necessary
        if opts.get('authors'):
            self.readauthormap(opts.get('authors'))
            self.authorfile = self.dest.authorfile()

        self.splicemap = mapfile(ui, opts.get('splicemap'))

    def walktree(self, heads):
        '''Return a mapping that identifies the uncommitted parents of every
        uncommitted changeset.'''
        visit = heads
        known = {}
        parents = {}
        while visit:
            n = visit.pop(0)
            if n in known or n in self.map: continue
            known[n] = 1
            commit = self.cachecommit(n)
            parents[n] = []
            for p in commit.parents:
                parents[n].append(p)
                visit.append(p)

        return parents

    def toposort(self, parents):
        '''Return an ordering such that every uncommitted changeset is
        preceeded by all its uncommitted ancestors.'''
        visit = parents.keys()
        seen = {}
        children = {}
        actives = []

        while visit:
            n = visit.pop(0)
            if n in seen: continue
            seen[n] = 1
            # Ensure that nodes without parents are present in the 'children'
            # mapping.
            children.setdefault(n, [])
            hasparent = False
            for p in parents[n]:
                if not p in self.map:
                    visit.append(p)
                    hasparent = True
                children.setdefault(p, []).append(n)
            if not hasparent:
                actives.append(n)

        del seen
        del visit

        if self.opts.get('datesort'):
            dates = {}
            def getdate(n):
                if n not in dates:
                    dates[n] = util.parsedate(self.commitcache[n].date)
                return dates[n]

            def picknext(nodes):
                return min([(getdate(n), n) for n in nodes])[1]
        else:
            prev = [None]
            def picknext(nodes):
                # Return the first eligible child of the previously converted
                # revision, or any of them.
                next = nodes[0]
                for n in nodes:
                    if prev[0] in parents[n]:
                        next = n
                        break
                prev[0] = next
                return next

        s = []
        pendings = {}
        while actives:
            n = picknext(actives)
            actives.remove(n)
            s.append(n)

            # Update dependents list
            for c in children.get(n, []):
                if c not in pendings:
                    pendings[c] = [p for p in parents[c] if p not in self.map]
                try:
                    pendings[c].remove(n)
                except ValueError:
                    raise util.Abort(_('cycle detected between %s and %s')
                                       % (recode(c), recode(n)))
                if not pendings[c]:
                    # Parents are converted, node is eligible
                    actives.insert(0, c)
                    pendings[c] = None

        if len(s) != len(parents):
            raise util.Abort(_("not all revisions were sorted"))

        return s

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
            if line.strip() == '':
                continue
            try:
                srcauthor, dstauthor = line.split('=', 1)
                srcauthor = srcauthor.strip()
                dstauthor = dstauthor.strip()
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
                    'Ignoring bad line in author map file %s: %s\n'
                    % (authorfile, line.rstrip()))
        afile.close()

    def cachecommit(self, rev):
        commit = self.source.getcommit(rev)
        commit.author = self.authors.get(commit.author, commit.author)
        self.commitcache[rev] = commit
        return commit

    def copy(self, rev):
        commit = self.commitcache[rev]
        do_copies = hasattr(self.dest, 'copyfile')
        filenames = []

        changes = self.source.getchanges(rev)
        if isinstance(changes, basestring):
            if changes == SKIPREV:
                dest = SKIPREV
            else:
                dest = self.map[changes]
            self.map[rev] = dest
            return
        files, copies = changes
        pbranches = []
        if commit.parents:
            for prev in commit.parents:
                if prev not in self.commitcache:
                    self.cachecommit(prev)
                pbranches.append((self.map[prev],
                                  self.commitcache[prev].branch))
        self.dest.setbranch(commit.branch, pbranches)
        for f, v in files:
            filenames.append(f)
            try:
                data = self.source.getfile(f, v)
            except IOError, inst:
                self.dest.delfile(f)
            else:
                e = self.source.getmode(f, v)
                self.dest.putfile(f, e, data)
                if do_copies:
                    if f in copies:
                        copyf = copies[f]
                        # Merely marks that a copy happened.
                        self.dest.copyfile(copyf, f)

        try:
            parents = self.splicemap[rev].replace(',', ' ').split()
            self.ui.status('spliced in %s as parents of %s\n' %
                           (parents, rev))
            parents = [self.map.get(p, p) for p in parents]
        except KeyError:
            parents = [b[0] for b in pbranches]
        newnode = self.dest.putcommit(filenames, parents, commit)
        self.source.converted(rev, newnode)
        self.map[rev] = newnode

    def convert(self):

        try:
            self.source.before()
            self.dest.before()
            self.source.setrevmap(self.map)
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
                # convert log message to local encoding without using
                # tolocal() because util._encoding conver() use it as
                # 'utf-8'
                self.ui.status("%d %s\n" % (num, recode(desc)))
                self.ui.note(_("source: %s\n" % recode(c)))
                self.copy(c)

            tags = self.source.gettags()
            ctags = {}
            for k in tags:
                v = tags[k]
                if self.map.get(v, SKIPREV) != SKIPREV:
                    ctags[k] = self.map[v]

            if c and ctags:
                nrev = self.dest.puttags(ctags)
                # write another hash correspondence to override the previous
                # one so we don't end up with extra tag heads
                if nrev:
                    self.map[c] = nrev

            self.writeauthormap()
        finally:
            self.cleanup()

    def cleanup(self):
        try:
            self.dest.after()
        finally:
            self.source.after()
        self.map.close()

def convert(ui, src, dest=None, revmapfile=None, **opts):
    global orig_encoding
    orig_encoding = util._encoding
    util._encoding = 'UTF-8'

    if not dest:
        dest = hg.defaultdest(src) + "-hg"
        ui.status("assuming destination %s\n" % dest)

    destc = convertsink(ui, dest, opts.get('dest_type'))

    try:
        srcc = convertsource(ui, src, opts.get('source_type'),
                             opts.get('rev'))
    except Exception:
        for path in destc.created:
            shutil.rmtree(path, True)
        raise

    fmap = opts.get('filemap')
    if fmap:
        srcc = filemap.filemap_source(ui, srcc, fmap)
        destc.setfilemapmode(True)

    if not revmapfile:
        try:
            revmapfile = destc.revmapfile()
        except:
            revmapfile = os.path.join(destc, "map")

    c = converter(ui, srcc, destc, revmapfile, opts)
    c.convert()

