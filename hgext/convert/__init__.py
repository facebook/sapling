# convert.py Foreign SCM converter
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from common import NoRepo, converter_source, converter_sink
from cvs import convert_cvs
from git import convert_git
from hg import mercurial_source, mercurial_sink
from subversion import convert_svn, debugsvnlog

import os, shlex, shutil
from mercurial import hg, ui, util, commands
from mercurial.i18n import _

commands.norepo += " convert debugsvnlog"

converters = [convert_cvs, convert_git, convert_svn, mercurial_source,
              mercurial_sink]

def convertsource(ui, path, **opts):
    for c in converters:
        try:
            return c.getcommit and c(ui, path, **opts)
        except (AttributeError, NoRepo):
            pass
    raise util.Abort('%s: unknown repository type' % path)

def convertsink(ui, path):
    if not os.path.isdir(path):
        raise util.Abort("%s: not a directory" % path)
    for c in converters:
        try:
            return c.putcommit and c(ui, path)
        except (AttributeError, NoRepo):
            pass
    raise util.Abort('%s: unknown repository type' % path)

class converter(object):
    def __init__(self, ui, source, dest, revmapfile, filemapper, opts):

        self.source = source
        self.dest = dest
        self.ui = ui
        self.opts = opts
        self.commitcache = {}
        self.revmapfile = revmapfile
        self.revmapfilefd = None
        self.authors = {}
        self.authorfile = None
        self.mapfile = filemapper

        self.map = {}
        try:
            origrevmapfile = open(self.revmapfile, 'r')
            for l in origrevmapfile:
                sv, dv = l[:-1].split()
                self.map[sv] = dv
            origrevmapfile.close()
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

        while visit:
            n = visit.pop(0)
            if n in seen: continue
            seen[n] = 1
            # Ensure that nodes without parents are present in the 'children'
            # mapping.
            children.setdefault(n, [])
            for p in parents[n]:
                if not p in self.map:
                    visit.append(p)
                children.setdefault(p, []).append(n)

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
        if self.revmapfilefd is None:
            try:
                self.revmapfilefd = open(self.revmapfile, "a")
            except IOError, (errno, strerror):
                raise util.Abort("Could not open map file %s: %s, %s\n" % (self.revmapfile, errno, strerror))
        self.map[src] = dst
        self.revmapfilefd.write("%s %s\n" % (src, dst))
        self.revmapfilefd.flush()

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

    def cachecommit(self, rev):
        commit = self.source.getcommit(rev)
        commit.author = self.authors.get(commit.author, commit.author)
        self.commitcache[rev] = commit
        return commit

    def copy(self, rev):
        commit = self.commitcache[rev]
        do_copies = hasattr(self.dest, 'copyfile')
        filenames = []

        files, copies = self.source.getchanges(rev)
        parents = [self.map[r] for r in commit.parents]
        if commit.parents:
            prev = commit.parents[0]
            if prev not in self.commitcache:
                self.cachecommit(prev)
            pbranch = self.commitcache[prev].branch
        else:
            pbranch = None
        self.dest.setbranch(commit.branch, pbranch, parents)
        for f, v in files:
            newf = self.mapfile(f)
            if not newf:
                continue
            filenames.append(newf)
            try:
                data = self.source.getfile(f, v)
            except IOError, inst:
                self.dest.delfile(newf)
            else:
                e = self.source.getmode(f, v)
                self.dest.putfile(newf, e, data)
                if do_copies:
                    if f in copies:
                        copyf = self.mapfile(copies[f])
                        if copyf:
                            # Merely marks that a copy happened.
                            self.dest.copyfile(copyf, newf)

        if not filenames and self.mapfile.active():
            newnode = parents[0]
        else:
            newnode = self.dest.putcommit(filenames, parents, commit)
        self.mapentry(rev, newnode)

    def convert(self):
        try:
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
        self.dest.after()
        if self.revmapfilefd:
            self.revmapfilefd.close()

def rpairs(name):
    e = len(name)
    while e != -1:
        yield name[:e], name[e+1:]
        e = name.rfind('/', 0, e)

class filemapper(object):
    '''Map and filter filenames when importing.
    A name can be mapped to itself, a new name, or None (omit from new
    repository).'''

    def __init__(self, ui, path=None):
        self.ui = ui
        self.include = {}
        self.exclude = {}
        self.rename = {}
        if path:
            if self.parse(path):
                raise util.Abort(_('errors in filemap'))

    def parse(self, path):
        errs = 0
        def check(name, mapping, listname):
            if name in mapping:
                self.ui.warn(_('%s:%d: %r already in %s list\n') %
                             (lex.infile, lex.lineno, name, listname))
                return 1
            return 0
        lex = shlex.shlex(open(path), path, True)
        lex.wordchars += '!@#$%^&*()-=+[]{}|;:,./<>?'
        cmd = lex.get_token()
        while cmd:
            if cmd == 'include':
                name = lex.get_token()
                errs += check(name, self.exclude, 'exclude')
                self.include[name] = name
            elif cmd == 'exclude':
                name = lex.get_token()
                errs += check(name, self.include, 'include')
                errs += check(name, self.rename, 'rename')
                self.exclude[name] = name
            elif cmd == 'rename':
                src = lex.get_token()
                dest = lex.get_token()
                errs += check(src, self.exclude, 'exclude')
                self.rename[src] = dest
            elif cmd == 'source':
                errs += self.parse(lex.get_token())
            else:
                self.ui.warn(_('%s:%d: unknown directive %r\n') %
                             (lex.infile, lex.lineno, cmd))
                errs += 1
            cmd = lex.get_token()
        return errs

    def lookup(self, name, mapping):
        for pre, suf in rpairs(name):
            try:
                return mapping[pre], pre, suf
            except KeyError, err:
                pass
        return '', name, ''

    def __call__(self, name):
        if self.include:
            inc = self.lookup(name, self.include)[0]
        else:
            inc = name
        if self.exclude:
            exc = self.lookup(name, self.exclude)[0]
        else:
            exc = ''
        if not inc or exc:
            return None
        newpre, pre, suf = self.lookup(name, self.rename)
        if newpre:
            if newpre == '.':
                return suf
            if suf:
                return newpre + '/' + suf
            return newpre
        return name

    def active(self):
        return bool(self.include or self.exclude or self.rename)

def convert(ui, src, dest=None, revmapfile=None, **opts):
    """Convert a foreign SCM repository to a Mercurial one.

    Accepted source formats:
    - GIT
    - CVS
    - SVN

    Accepted destination formats:
    - Mercurial

    If no revision is given, all revisions will be converted. Otherwise,
    convert will only import up to the named revision (given in a format
    understood by the source).

    If no destination directory name is specified, it defaults to the
    basename of the source with '-hg' appended.  If the destination
    repository doesn't exist, it will be created.

    If <revmapfile> isn't given, it will be put in a default location
    (<dest>/.hg/shamap by default).  The <revmapfile> is a simple text
    file that maps each source commit ID to the destination ID for
    that revision, like so:
    <source ID> <destination ID>

    If the file doesn't exist, it's automatically created.  It's updated
    on each commit copied, so convert-repo can be interrupted and can
    be run repeatedly to copy new commits.

    The [username mapping] file is a simple text file that maps each source
    commit author to a destination commit author. It is handy for source SCMs
    that use unix logins to identify authors (eg: CVS). One line per author
    mapping and the line format is:
    srcauthor=whatever string you want

    The filemap is a file that allows filtering and remapping of files
    and directories.  Comment lines start with '#'.  Each line can
    contain one of the following directives:

      include path/to/file

      exclude path/to/file

      rename from/file to/file
    
    The 'include' directive causes a file, or all files under a
    directory, to be included in the destination repository.  The
    'exclude' directive causes files or directories to be omitted.
    The 'rename' directive renames a file or directory.  To rename
    from a subdirectory into the root of the repository, use '.' as
    the path to rename to.
    """

    util._encoding = 'UTF-8'

    if not dest:
        dest = hg.defaultdest(src) + "-hg"
        ui.status("assuming destination %s\n" % dest)

    # Try to be smart and initalize things when required
    created = False
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
            created = True
    elif os.path.exists(dest):
        raise util.Abort("destination %s exists and is not a directory" % dest)
    else:
        ui.status("initializing destination %s repository\n" % dest)
        hg.repository(ui, dest, create=True)
        created = True

    destc = convertsink(ui, dest)

    try:
        srcc = convertsource(ui, src, rev=opts.get('rev'))
    except Exception:
        if created:
            shutil.rmtree(dest, True)
        raise

    if not revmapfile:
        try:
            revmapfile = destc.revmapfile()
        except:
            revmapfile = os.path.join(destc, "map")


    c = converter(ui, srcc, destc, revmapfile, filemapper(ui, opts['filemap']),
                  opts)
    c.convert()


cmdtable = {
    "convert":
        (convert,
         [('A', 'authors', '', 'username mapping filename'),
          ('', 'filemap', '', 'remap file names using contents of file'),
          ('r', 'rev', '', 'import up to target revision REV'),
          ('', 'datesort', None, 'try to sort changesets by date')],
         'hg convert [OPTION]... SOURCE [DEST [MAPFILE]]'),
    "debugsvnlog":
        (debugsvnlog,
         [],
         'hg debugsvnlog'),
}

