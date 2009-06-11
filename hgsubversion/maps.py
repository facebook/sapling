''' Module for self-contained maps. '''

import os
from mercurial import util as hgutil
from mercurial import node

class AuthorMap(dict):
    '''A mapping from Subversion-style authors to Mercurial-style
    authors, and back. The data is stored persistently on disk.

    If the 'hgsubversion.defaultauthors' configuration option is set to false,
    attempting to obtain an unknown author will fail with an Abort.
    '''

    def __init__(self, ui, path, defaulthost=None):
        '''Initialise a new AuthorMap.

        The ui argument is used to print diagnostic messages.

        The path argument is the location of the backing store,
        typically .hg/authormap.
        '''
        self.ui = ui
        self.path = path
        if defaulthost:
            self.defaulthost = '@%s' % defaulthost.lstrip('@')
        else:
            self.defaulthost = ''
        self.super = super(AuthorMap, self)
        self.super.__init__()
        self.load(path)

    def load(self, path):
        ''' Load mappings from a file at the specified path. '''
        if not os.path.exists(path):
            return
        self.ui.note('reading authormap from %s\n' % path)
        f = open(path, 'r')
        for number, line in enumerate(f):

            line = line.split('#')[0]
            if not line.strip():
                continue

            try:
                src, dst = line.split('=', 1)
            except (IndexError, ValueError):
                msg = 'ignoring line %i in author map %s: %s\n'
                self.ui.warn(msg % (number, path, line.rstrip()))
                continue

            src = src.strip()
            dst = dst.strip()
            if src in self and dst != self[src]:
                msg = 'overriding author: "%s" to "%s" (%s)\n'
                self.ui.warn(msg % (self[src], dst, src))
            else:
                self[src] = dst

        f.close()

    def __setitem__(self, key, value):
        ''' Similar to dict.__setitem__, but also updates the new mapping in the
        backing store. '''
        self.super.__setitem__(key, value)
        self.ui.debug('adding author %s to author map\n' % self.path)
        f = open(self.path, 'w+')
        for k, v in self.iteritems():
            f.write("%s=%s\n" % (k, v))
        f.close()

    def __getitem__(self, author):
        ''' Similar to dict.__getitem__, except in case of an unknown author.
        In such cases, a new value is generated and added to the dictionary
        as well as the backing store. '''
        if author in self:
            result = self.super.__getitem__(author)
        elif self.ui.configbool('hgsubversion', 'defaultauthors', True):
            self[author] = result = '%s%s' % (author, self.defaulthost)
            msg = 'substituting author "%s" for default "%s"\n'
            self.ui.note(msg % (author, result))
        else:
            msg = 'author %s has no entry in the author map!'
            raise hgutil.Abort(msg % author)
        self.ui.debug('mapping author "%s" to "%s"\n' % (author, result))
        return result

    def reverselookup(self, author):
        for svnauthor, hgauthor in self.iteritems():
            if author == hgauthor:
                return svnauthor
        else:
            # Mercurial incorrectly splits at e.g. '.', so we roll our own.
            return author.rsplit('@', 1)[0]


class RevMap(dict):

    VERSION = 1

    def __init__(self, repo):
        dict.__init__(self)
        self.path = os.path.join(repo.path, 'svn', 'rev_map')
        if os.path.isfile(self.path):
            self._load()
        else:
            self._write()

    def _load(self):
        f = open(self.path)
        ver = int(f.readline())
        if ver != self.VERSION:
            print 'revmap too new -- please upgrade'
            raise NotImplementedError
        for l in f:
            revnum, hash, branch = l.split(' ', 2)
            if branch == '\n':
                branch = None
            else:
                branch = branch[:-1]
            dict.__setitem__(self, (int(revnum), branch), node.bin(hash))
        f.close()

    def _write(self):
        f = open(self.path, 'w')
        f.write('%s\n' % self.VERSION)
        f.flush()
        f.close()

    def __setitem__(self, key, hash):
        revnum, branch = key
        f = open(self.path, 'a')
        b = branch or ''
        f.write(str(revnum) + ' ' + node.hex(hash) + ' ' + b + '\n')
        f.flush()
        f.close()
        dict.__setitem__(self, (revnum, branch), hash)


class FileMap(object):

    def __init__(self, repo):
        self.ui = repo.ui
        self.include = {}
        self.exclude = {}
        filemap = repo.ui.config('hgsubversion', 'filemap')
        if filemap and os.path.exists(filemap):
            self.load(filemap)

    def _rpairs(self, name):
        yield '.', name
        e = len(name)
        while e != -1:
            yield name[:e], name[e+1:]
            e = name.rfind('/', 0, e)

    def check(self, map, path):
        map = getattr(self, map)
        for pre, suf in self._rpairs(path):
            if pre not in map:
                continue
            return map[pre]
        return None

    def __contains__(self, path):
        if len(self.include) and len(path):
            inc = self.check('include', path)
        else:
            inc = path
        if len(self.exclude) and len(path):
            exc = self.check('exclude', path)
        else:
            exc = None
        if inc is None or exc is not None:
            return False
        return True

    def add(self, fn, map, path):
        mapping = getattr(self, map)
        if path in mapping:
            msg = 'duplicate %s entry in %s: "%d"\n'
            self.ui.warn(msg % (map, fn, path))
            return
        bits = map.strip('e'), path
        self.ui.debug('%sing %s\n' % bits)
        mapping[path] = path

    def load(self, fn):
        self.ui.note('reading file map from %s\n' % fn)
        f = open(fn, 'r')
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
                self.ui.warn('unknown filemap command %s\n' % cmd)
            except IndexError:
                msg = 'ignoring bad line in filemap %s: %s\n'
                self.ui.warn(msg % (fn, line.rstrip()))
        f.close()
