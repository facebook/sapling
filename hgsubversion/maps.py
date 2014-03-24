''' Module for self-contained maps. '''

import errno
import os
from mercurial import util as hgutil
from mercurial.node import bin, hex, nullid

import subprocess
import svncommands
import util

class AuthorMap(dict):
    '''A mapping from Subversion-style authors to Mercurial-style
    authors, and back. The data is stored persistently on disk.

    If the 'hgsubversion.defaultauthors' configuration option is set to false,
    attempting to obtain an unknown author will fail with an Abort.

    If the 'hgsubversion.caseignoreauthors' configuration option is set to true,
    the userid from Subversion is always compared lowercase.
    '''

    def __init__(self, meta):
        '''Initialise a new AuthorMap.

        The ui argument is used to print diagnostic messages.

        The path argument is the location of the backing store,
        typically .hg/svn/authors.
        '''
        self.meta = meta
        self.defaulthost = ''
        if meta.defaulthost:
            self.defaulthost = '@%s' % meta.defaulthost.lstrip('@')

        self.super = super(AuthorMap, self)
        self.super.__init__()
        self.load(self.meta.authormap_file)

        # append authors specified from the commandline
        clmap = util.configpath(self.meta.ui, 'authormap')
        if clmap:
            self.load(clmap)

    def load(self, path):
        ''' Load mappings from a file at the specified path. '''

        path = os.path.expandvars(path)
        if not os.path.exists(path):
            return

        writing = False
        if path != self.meta.authormap_file:
            writing = open(self.meta.authormap_file, 'a')

        self.meta.ui.debug('reading authormap from %s\n' % path)
        f = open(path, 'r')
        for number, line_org in enumerate(f):

            line = line_org.split('#')[0]
            if not line.strip():
                continue

            try:
                src, dst = line.split('=', 1)
            except (IndexError, ValueError):
                msg = 'ignoring line %i in author map %s: %s\n'
                self.meta.ui.status(msg % (number, path, line.rstrip()))
                continue

            src = src.strip()
            dst = dst.strip()

            if self.meta.caseignoreauthors:
                src = src.lower()

            if writing:
                if not src in self:
                    self.meta.ui.debug('adding author %s to author map\n' % src)
                elif dst != self[src]:
                    msg = 'overriding author: "%s" to "%s" (%s)\n'
                    self.meta.ui.status(msg % (self[src], dst, src))
                writing.write(line_org)

            self[src] = dst

        f.close()
        if writing:
            writing.close()

    def __setitem__(self, key, value):
        '''Similar to dict.__setitem__, except we check caseignoreauthors to
        use lowercase string or not
        '''
        if self.meta.caseignoreauthors:
            key = key.lower()
        super(AuthorMap, self).__setitem__(key, value)

    def __getitem__(self, author):
        ''' Similar to dict.__getitem__, except in case of an unknown author.
        In such cases, a new value is generated and added to the dictionary
        as well as the backing store. '''
        if author is None:
            author = '(no author)'

        search_author = author
        if self.meta.caseignoreauthors:
            search_author = author.lower()

        result = None
        if search_author in self:
            result = self.super.__getitem__(search_author)
        elif self.meta.mapauthorscmd:
            cmd = self.meta.mapauthorscmd % author
            process = subprocess.Popen(cmd, shell=True, stdout=subprocess.PIPE)
            output, err = process.communicate()
            retcode = process.poll()
            if retcode:
                msg = 'map author command "%s" exited with error'
                raise hgutil.Abort(msg % cmd)
            self[author] = result = output.strip()
        if not result:
            if self.meta.defaultauthors:
                self[author] = result = '%s%s' % (author, self.defaulthost)
                msg = 'substituting author "%s" for default "%s"\n'
                self.meta.ui.debug(msg % (author, result))
            else:
                msg = 'author %s has no entry in the author map!'
                raise hgutil.Abort(msg % author)
        self.meta.ui.debug('mapping author "%s" to "%s"\n' % (author, result))
        return result

    def reverselookup(self, author):
        for svnauthor, hgauthor in self.iteritems():
            if author == hgauthor:
                return svnauthor
        else:
            # Mercurial incorrectly splits at e.g. '.', so we roll our own.
            return author.rsplit('@', 1)[0]


class Tags(dict):
    """Map tags to converted node identifier.

    tag names are non-empty strings. Tags are saved in a file
    called tagmap, for backwards compatibility reasons.
    """
    VERSION = 2

    def __init__(self, meta, endrev=None):
        dict.__init__(self)
        self.meta = meta
        self.endrev = endrev
        if os.path.isfile(self.meta.tagfile):
            self._load()
        else:
            self._write()

    def _load(self):
        f = open(self.meta.tagfile)
        ver = int(f.readline())
        if ver < self.VERSION:
            self.meta.ui.status('tag map outdated, running rebuildmeta...\n')
            f.close()
            os.unlink(self.meta.tagfile)
            svncommands.rebuildmeta(self.meta.ui, self.meta.repo, ())
            return
        elif ver != self.VERSION:
            raise hgutil.Abort('tagmap too new -- please upgrade')
        for l in f:
            ha, revision, tag = l.split(' ', 2)
            revision = int(revision)
            tag = tag[:-1]
            if self.endrev is not None and revision > self.endrev:
                break
            if not tag:
                continue
            dict.__setitem__(self, tag, bin(ha))
        f.close()

    def _write(self):
        assert self.endrev is None
        f = open(self.meta.tagfile, 'w')
        f.write('%s\n' % self.VERSION)
        f.close()

    def update(self, other):
        for k, v in other.iteritems():
            self[k] = v

    def __contains__(self, tag):
        return (tag and dict.__contains__(self, tag)
                and dict.__getitem__(self, tag) != nullid)

    def __getitem__(self, tag):
        if tag and tag in self:
            return dict.__getitem__(self, tag)
        raise KeyError()

    def __setitem__(self, tag, info):
        if not tag:
            raise hgutil.Abort('tag cannot be empty')
        ha, revision = info
        f = open(self.meta.tagfile, 'a')
        f.write('%s %s %s\n' % (hex(ha), revision, tag))
        f.close()
        dict.__setitem__(self, tag, ha)


class RevMap(dict):

    VERSION = 1

    def __init__(self, meta):
        dict.__init__(self)
        self.meta = meta
        self._hashes = None

        if os.path.isfile(self.meta.revmap_file):
            self._load()
        else:
            self._write()

    def hashes(self):
        if self._hashes is None:
            self._hashes = dict((v, k) for (k, v) in self.iteritems())
        return self._hashes

    def branchedits(self, branch, rev):
        check = lambda x: x[0][1] == branch and x[0][0] < rev.revnum
        return sorted(filter(check, self.iteritems()), reverse=True)

    @classmethod
    def readmapfile(cls, path, missingok=True):
        try:
            f = open(path)
        except IOError, err:
            if not missingok or err.errno != errno.ENOENT:
                raise
            return iter([])
        ver = int(f.readline())
        if ver != cls.VERSION:
            raise hgutil.Abort('revmap too new -- please upgrade')
        return f

    @util.gcdisable
    def _load(self):
        lastpulled = self.meta.lastpulled
        firstpulled = self.meta.firstpulled
        setitem = dict.__setitem__
        for l in self.readmapfile(self.meta.revmap_file):
            revnum, ha, branch = l.split(' ', 2)
            if branch == '\n':
                branch = None
            else:
                branch = branch[:-1]
            revnum = int(revnum)
            if revnum > lastpulled or not lastpulled:
                lastpulled = revnum
            if revnum < firstpulled or not firstpulled:
                firstpulled = revnum
            setitem(self, (revnum, branch), bin(ha))
        self.meta.lastpulled = lastpulled
        self.meta.firstpulled = firstpulled

    def _write(self):
        f = open(self.meta.revmap_file, 'w')
        f.write('%s\n' % self.VERSION)
        f.close()

    def __setitem__(self, key, ha):
        revnum, branch = key
        f = open(self.meta.revmap_file, 'a')
        b = branch or ''
        f.write(str(revnum) + ' ' + hex(ha) + ' ' + b + '\n')
        f.close()
        if revnum > self.meta.lastpulled or not self.meta.lastpulled:
            self.meta.lastpulled = revnum
        if revnum < self.meta.firstpulled or not self.meta.firstpulled:
            self.meta.firstpulled = revnum
        dict.__setitem__(self, (revnum, branch), ha)
        if self._hashes is not None:
            self._hashes[ha] = (revnum, branch)


class FileMap(object):

    VERSION = 1

    def __init__(self, meta):
        '''Initialise a new FileMap.

        The ui argument is used to print diagnostic messages.

        The path argument is the location of the backing store,
        typically .hg/svn/filemap.
        '''
        self.meta = meta
        self.include = {}
        self.exclude = {}
        if os.path.isfile(self.meta.filemap_file):
            self._load()
        else:
            self._write()

        # append file mapping specified from the commandline
        clmap = util.configpath(self.meta.ui, 'filemap')
        if clmap:
            self.load(clmap)

    def _rpairs(self, name):
        e = len(name)
        while e != -1:
            yield name[:e], name[e+1:]
            e = name.rfind('/', 0, e)
        yield '.', name

    def check(self, m, path):
        m = getattr(self, m)
        for pre, _suf in self._rpairs(path):
            if pre in m:
                return m[pre]
        return -1

    def __contains__(self, path):
        if not len(path):
            return True
        if len(self.include):
            inc = self.check('include', path)
        elif not len(self.exclude):
            return True
        else:
            inc = 0
        if len(self.exclude):
            exc = self.check('exclude', path)
        else:
            exc = -1
        # respect rule order: newer rules override older
        return inc > exc

    # Needed so empty filemaps are false
    def __len__(self):
        return len(self.include) + len(self.exclude)

    def add(self, fn, m, path):
        mapping = getattr(self, m)
        if path in mapping:
            msg = 'duplicate %s entry in %s: "%s"\n'
            self.meta.ui.status(msg % (m, fn, path))
            return
        bits = m.rstrip('e'), path
        self.meta.ui.debug('%sing %s\n' % bits)
        # respect rule order
        mapping[path] = len(self)
        if fn != self.meta.filemap_file:
            f = open(self.meta.filemap_file, 'a')
            f.write(m + ' ' + path + '\n')
            f.close()

    def load(self, fn):
        self.meta.ui.debug('reading file map from %s\n' % fn)
        f = open(fn, 'r')
        self.load_fd(f, fn)
        f.close()

    def load_fd(self, f, fn):
        for line in f:
            if line.strip() == '' or line.strip()[0] == '#':
                continue
            try:
                cmd, path = line.split(' ', 1)
                cmd = cmd.strip()
                path = path.strip()
                if cmd in ('include', 'exclude'):
                    self.add(fn, cmd, path)
                    continue
                self.meta.ui.warn('unknown filemap command %s\n' % cmd)
            except IndexError:
                msg = 'ignoring bad line in filemap %s: %s\n'
                self.meta.ui.warn(msg % (fn, line.rstrip()))

    def _load(self):
        self.meta.ui.debug('reading in-repo file map from %s\n' % self.meta.filemap_file)
        f = open(self.meta.filemap_file)
        ver = int(f.readline())
        if ver != self.VERSION:
            raise hgutil.Abort('filemap too new -- please upgrade')
        self.load_fd(f, self.meta.filemap_file)
        f.close()

    def _write(self):
        f = open(self.meta.filemap_file, 'w')
        f.write('%s\n' % self.VERSION)
        f.close()

class BranchMap(dict):
    '''Facility for controlled renaming of branch names. Example:

    oldname = newname
    other = default

    All changes on the oldname branch will now be on the newname branch; all
    changes on other will now be on default (have no branch name set).
    '''

    def __init__(self, meta):
        self.meta = meta
        self.super = super(BranchMap, self)
        self.super.__init__()
        self.load(self.meta.branchmap_file)

        # append branch mapping specified from the commandline
        clmap = util.configpath(self.meta.ui, 'branchmap')
        if clmap:
            self.load(clmap)

    def load(self, path):
        '''Load mappings from a file at the specified path.'''
        if not os.path.exists(path):
            return

        writing = False
        if path != self.meta.branchmap_file:
            writing = open(self.meta.branchmap_file, 'a')

        self.meta.ui.debug('reading branchmap from %s\n' % path)
        f = open(path, 'r')
        for number, line in enumerate(f):

            if writing:
                writing.write(line)

            line = line.split('#')[0]
            if not line.strip():
                continue

            try:
                src, dst = line.split('=', 1)
            except (IndexError, ValueError):
                msg = 'ignoring line %i in branch map %s: %s\n'
                self.meta.ui.status(msg % (number, path, line.rstrip()))
                continue

            src = src.strip()
            dst = dst.strip()
            self.meta.ui.debug('adding branch %s to branch map\n' % src)

            if dst and src in self and dst != self[src]:
                msg = 'overriding branch: "%s" to "%s" (%s)\n'
                self.meta.ui.status(msg % (self[src], dst, src))
            self[src] = dst

        f.close()
        if writing:
            writing.close()

class TagMap(dict):
    '''Facility for controlled renaming of tags. Example:

    oldname = newname
    other =

        The oldname tag from SVN will be represented as newname in the hg tags;
        the other tag will not be reflected in the hg repository.
    '''

    def __init__(self, meta):
        self.meta = meta
        self.super = super(TagMap, self)
        self.super.__init__()
        self.load(self.meta.tagmap_file)

        # append tag mapping specified from the commandline
        clmap = util.configpath(self.meta.ui, 'tagmap')
        if clmap:
            self.load(clmap)

    def load(self, path):
        '''Load mappings from a file at the specified path.'''
        if not os.path.exists(path):
            return

        writing = False
        if path != self.meta.tagmap_file:
            writing = open(self.meta.tagmap_file, 'a')

        self.meta.ui.debug('reading tag renames from %s\n' % path)
        f = open(path, 'r')
        for number, line in enumerate(f):

            if writing:
                writing.write(line)

            line = line.split('#')[0]
            if not line.strip():
                continue

            try:
                src, dst = line.split('=', 1)
            except (IndexError, ValueError):
                msg = 'ignoring line %i in tag renames %s: %s\n'
                self.meta.ui.status(msg % (number, path, line.rstrip()))
                continue

            src = src.strip()
            dst = dst.strip()
            self.meta.ui.debug('adding tag %s to tag renames\n' % src)

            if src in self and dst != self[src]:
                msg = 'overriding tag rename: "%s" to "%s" (%s)\n'
                self.meta.ui.status(msg % (self[src], dst, src))
            self[src] = dst

        f.close()
        if writing:
            writing.close()
