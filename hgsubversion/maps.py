''' Module for self-contained maps. '''

import errno
import os
import re
from mercurial import util as hgutil
from mercurial.node import bin, hex, nullid

import subprocess
import svncommands
import util

class BaseMap(dict):
    '''A base class for the different type of mappings: author, branch, and
    tags.'''
    def __init__(self, meta):
        super(BaseMap, self).__init__()
        self._ui = meta.ui

        self._commentre = re.compile(r'((^|[^\\])(\\\\)*)#.*')
        self.syntaxes = ('re', 'glob')

        # trickery: all subclasses have the same name as their file and config
        # names, e.g. AuthorMap is meta.authormap_file for the filename and
        # 'authormap' for the config option
        self.mapname = self.__class__.__name__.lower()
        self.mapfilename = self.mapname + '_file'
        self._filepath = meta.__getattribute__(self.mapfilename)
        self.load(self._filepath)

        # append mappings specified from the commandline
        clmap = util.configpath(self._ui, self.mapname)
        if clmap:
            self.load(clmap)

    def _findkey(self, key):
        '''Takes a string and finds the first corresponding key that matches
        via regex'''
        if not key:
            return None

        # compile a new regex key if we're given a string; can't use
        # hgutil.compilere since we need regex.sub
        k = key
        if isinstance(key, str):
            k = re.compile(re.escape(key))

        # preference goes to matching the exact pattern, i.e. 'foo' should
        # first match 'foo' before trying regexes
        for regex in self:
            if regex.pattern == k.pattern:
                return regex

        # if key isn't a string, then we are done; nothing matches
        if not isinstance(key, str):
            return None

        # now we test the regex; the above loop will be faster and is
        # equivalent to not having regexes (i.e. just doing string compares)
        for regex in self:
            if regex.search(key):
                return regex
        return None

    def get(self, key, default=None):
        '''Similar to dict.get, except we use our own matcher, _findkey.'''
        if self._findkey(key):
            return self[key]
        return default

    def __getitem__(self, key):
        '''Similar to dict.get, except we use our own matcher, _findkey. If the key is
        a string, then we can use our regex matching to map its value.
        '''
        k = self._findkey(key)
        val = super(BaseMap, self).__getitem__(k)

        # if key is a string then we can transform it using our regex, else we
        # don't have enough information, so we just return the val
        if isinstance(key, str):
            val = k.sub(val, key)

        return val

    def __setitem__(self, key, value):
        '''Similar to dict.__setitem__, except we compile the string into a regex, if
        need be.
        '''
        # try to find the regex already in the map
        k = self._findkey(key)
        # if we found one, then use it
        if k:
            key = k
        # else make a new regex
        if isinstance(key, str):
            key = re.compile(re.escape(key))
        super(BaseMap, self).__setitem__(key, value)

    def __contains__(self, key):
        '''Similar to dict.get, except we use our own matcher, _findkey.'''
        return self._findkey(key) is not None

    def load(self, path):
        '''Load mappings from a file at the specified path.'''
        path = os.path.expandvars(path)
        if not os.path.exists(path):
            return

        writing = False
        mapfile = self._filepath
        if path != mapfile:
            writing = open(mapfile, 'a')

        self._ui.debug('reading %s from %s\n' % (self.mapname , path))
        f = open(path, 'r')
        syntax = ''
        for number, line in enumerate(f):

            if writing:
                writing.write(line)

            # strip out comments
            if "#" in line:
                # remove comments prefixed by an even number of escapes
                line = self._commentre.sub(r'\1', line)
                # fixup properly escaped comments that survived the above
                line = line.replace("\\#", "#")
            line = line.rstrip()
            if not line:
                continue

            if line.startswith('syntax:'):
                s = line[7:].strip()
                syntax = ''
                if s in self.syntaxes:
                    syntax = s
                continue
            pat = syntax
            for s in self.syntaxes:
                if line.startswith(s + ':'):
                    pat = s
                    line = line[len(s) + 1:]
                    break

            # split on the first '='
            try:
                src, dst = line.split('=', 1)
            except (IndexError, ValueError):
                msg = 'ignoring line %i in %s %s: %s\n'
                self._ui.status(msg % (number, self.mapname, path,
                                           line.rstrip()))
                continue

            src = src.strip()
            dst = dst.strip()

            if pat != 're':
                src = re.escape(src)
            if pat == 'glob':
                src = src.replace('\\*', '.*')
            src = re.compile(src)

            if src not in self:
                self._ui.debug('adding %s to %s\n' % (src, self.mapname))
            elif dst != self[src]:
                msg = 'overriding %s: "%s" to "%s" (%s)\n'
                self._ui.status(msg % (self.mapname, self[src], dst, src))
            self[src] = dst

        f.close()
        if writing:
            writing.close()

class AuthorMap(BaseMap):
    '''A mapping from Subversion-style authors to Mercurial-style
    authors, and back. The data is stored persistently on disk.

    If the 'hgsubversion.defaultauthors' configuration option is set to false,
    attempting to obtain an unknown author will fail with an Abort.

    If the 'hgsubversion.caseignoreauthors' configuration option is set to true,
    the userid from Subversion is always compared lowercase.
    '''

    def __init__(self, meta, defaulthost, caseignoreauthors,
                 mapauthorscmd, defaultauthors):
        '''Initialise a new AuthorMap.

        The ui argument is used to print diagnostic messages.

        The path argument is the location of the backing store,
        typically .hg/svn/authors.
        '''
        if defaulthost:
            self.defaulthost = '@%s' % defaulthost.lstrip('@')
        else:
            self.defaulthost = ''
        self._caseignoreauthors = caseignoreauthors
        self._mapauthorscmd = mapauthorscmd
        self._defaulthost = defaulthost
        self._defaultauthors = defaultauthors

        super(AuthorMap, self).__init__(meta)

    def _lowercase(self, key):
        '''Determine whether or not to lowercase a str or regex using the
        meta.caseignoreauthors.'''
        k = key
        if self._caseignoreauthors:
            if isinstance(key, str):
                k = key.lower()
            else:
                k = re.compile(key.pattern.lower())
        return k

    def __setitem__(self, key, value):
        '''Similar to dict.__setitem__, except we check caseignoreauthors to
        use lowercase string or not
        '''
        super(AuthorMap, self).__setitem__(self._lowercase(key), value)

    def __contains__(self, key):
        '''Similar to dict.__contains__, except we check caseignoreauthors to
        use lowercase string or not
        '''
        return super(AuthorMap, self).__contains__(self._lowercase(key))

    def __getitem__(self, author):
        ''' Similar to dict.__getitem__, except in case of an unknown author.
        In such cases, a new value is generated and added to the dictionary
        as well as the backing store. '''
        if author is None:
            author = '(no author)'

        if not isinstance(author, str):
            return super(AuthorMap, self).__getitem__(author)

        search_author = author
        if self._caseignoreauthors:
            search_author = author.lower()

        result = None
        if search_author in self:
            result = super(AuthorMap, self).__getitem__(search_author)
        elif self._mapauthorscmd:
            cmd = self._mapauthorscmd % author
            process = subprocess.Popen(cmd, shell=True, stdout=subprocess.PIPE)
            output, err = process.communicate()
            retcode = process.poll()
            if retcode:
                msg = 'map author command "%s" exited with error'
                raise hgutil.Abort(msg % cmd)
            self[author] = result = output.strip()
        if not result:
            if self._defaultauthors:
                self[author] = result = '%s%s' % (author, self.defaulthost)
                msg = 'substituting author "%s" for default "%s"\n'
                self._ui.debug(msg % (author, result))
            else:
                msg = 'author %s has no entry in the author map!'
                raise hgutil.Abort(msg % author)
        self._ui.debug('mapping author "%s" to "%s"\n' % (author, result))
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
        self._filepath = meta.tagfile
        self._ui = meta.ui
        self.endrev = endrev
        if os.path.isfile(self._filepath):
            self._load()
        else:
            self._write()

    def _load(self):
        f = open(self._filepath)
        ver = int(f.readline())
        if ver < self.VERSION:
            self._ui.status('tag map outdated, running rebuildmeta...\n')
            f.close()
            os.unlink(self._filepath)
            svncommands.rebuildmeta(self._ui, self.meta.repo, ())
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
        f = open(self._filepath, 'w')
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
        f = open(self._filepath, 'a')
        f.write('%s %s %s\n' % (hex(ha), revision, tag))
        f.close()
        dict.__setitem__(self, tag, ha)


class RevMap(dict):

    VERSION = 1

    def __init__(self, meta):
        dict.__init__(self)
        self._filepath = meta.revmap_file
        self._lastpulled_file = os.path.join(meta.metapath, 'lastpulled')
        self._hashes = None

        self.firstpulled = 0
        if os.path.exists(self._lastpulled_file):
            with open(self._lastpulled_file) as f:
                self._lastpulled = int(f.read())
        else:
            self._lastpulled = 0

        if os.path.isfile(self._filepath):
            self._load()
        else:
            self._write()

    def _writelastpulled(self):
        with open(self._lastpulled_file, 'w') as f:
            f.write('%d\n' % self.lastpulled)

    @property
    def lastpulled(self):
        return self._lastpulled

    @lastpulled.setter
    def lastpulled(self, value):
        self._lastpulled = value
        self._writelastpulled()

    def hashes(self):
        if self._hashes is None:
            self._hashes = dict((v, k) for (k, v) in self.iteritems())
        return self._hashes

    def branchedits(self, branch, rev):
        check = lambda x: x[0][1] == branch and x[0][0] < rev.revnum
        return sorted(filter(check, self.iteritems()), reverse=True)

    def branchmaxrevnum(self, branch, maxrevnum):
        result = 0
        for num, br in self.iterkeys():
            if br == branch and num <= maxrevnum and num > result:
                result = num
        return result

    @property
    def lasthash(self):
        lines = list(self._readmapfile())
        if not lines:
            return None
        return bin(lines[-1].split(' ', 2)[1])

    def revhashes(self, revnum):
        for key, value in self.iteritems():
            if key[0] == revnum:
                yield value

    def clear(self):
        self._write()
        dict.clear(self)
        self._hashes = None

    def batchset(self, items, lastpulled):
        '''Set items in batches

        items is an array of (rev num, branch, binary hash)

        For performance reason, internal in-memory state is not updated.
        To get an up-to-date RevMap, reconstruct the object.
        '''
        with open(self._filepath, 'a') as f:
            f.write(''.join('%s %s %s\n' % (revnum, hex(binhash), br or '')
                            for revnum, br, binhash in items))
        with open(self._lastpulled_file, 'w') as f:
            f.write('%s\n' % lastpulled)

    def _readmapfile(self):
        path = self._filepath
        try:
            f = open(path)
        except IOError, err:
            if err.errno != errno.ENOENT:
                raise
            return iter([])
        ver = int(f.readline())
        if ver != self.VERSION:
            raise hgutil.Abort('revmap too new -- please upgrade')
        return f

    @classmethod
    def exists(cls, meta):
        return os.path.exists(meta.revmap_file)

    @util.gcdisable
    def _load(self):
        lastpulled = self.lastpulled
        firstpulled = self.firstpulled
        if os.path.exists(self._lastpulled_file):
            with open(self._lastpulled_file) as f:
                lastpulled = int(f.read())
        setitem = dict.__setitem__
        for l in self._readmapfile():
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
        if self.lastpulled != lastpulled:
            self.lastpulled = lastpulled
        self.firstpulled = firstpulled

    def _write(self):
        with open(self._filepath, 'w') as f:
            f.write('%s\n' % self.VERSION)
        self._writelastpulled()

    def __setitem__(self, key, ha):
        revnum, branch = key
        b = branch or ''
        with open(self._filepath, 'a') as f:
            f.write(str(revnum) + ' ' + hex(ha) + ' ' + b + '\n')
        if revnum > self.lastpulled or not self.lastpulled:
            self.lastpulled = revnum
        if revnum < self.firstpulled or not self.firstpulled:
            self.firstpulled = revnum
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
        self._filename = meta.filemap_file
        self._ui = meta.ui
        self.include = {}
        self.exclude = {}
        if os.path.isfile(self._filename):
            self._load()
        else:
            self._write()

        # append file mapping specified from the commandline
        clmap = util.configpath(self._ui, 'filemap')
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
            self._ui.status(msg % (m, fn, path))
            return
        bits = m.rstrip('e'), path
        self._ui.debug('%sing %s\n' % bits)
        # respect rule order
        mapping[path] = len(self)
        if fn != self._filename:
            with open(self._filename, 'a') as f:
                f.write(m + ' ' + path + '\n')

    def load(self, fn):
        self._ui.debug('reading file map from %s\n' % fn)
        with open(fn, 'r') as f:
            self.load_fd(f, fn)

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
                self._ui.warn('unknown filemap command %s\n' % cmd)
            except IndexError:
                msg = 'ignoring bad line in filemap %s: %s\n'
                self._ui.warn(msg % (fn, line.rstrip()))

    def _load(self):
        self._ui.debug('reading in-repo file map from %s\n' % self._filename)
        with open(self._filename) as f:
            ver = int(f.readline())
            if ver != self.VERSION:
                raise hgutil.Abort('filemap too new -- please upgrade')
            self.load_fd(f, self._filename)

    def _write(self):
        with open(self._filename, 'w') as f:
            f.write('%s\n' % self.VERSION)

class BranchMap(BaseMap):
    '''Facility for controlled renaming of branch names. Example:

    oldname = newname
    other = default

    All changes on the oldname branch will now be on the newname branch; all
    changes on other will now be on default (have no branch name set).
    '''

class TagMap(BaseMap):
    '''Facility for controlled renaming of tags. Example:

    oldname = newname
    other =

        The oldname tag from SVN will be represented as newname in the hg tags;
        the other tag will not be reflected in the hg repository.
    '''
