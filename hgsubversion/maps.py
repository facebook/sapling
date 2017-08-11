''' Module for self-contained maps. '''

import collections
import contextlib
import errno
import os
import re
import sqlite3
import sys
import weakref
from mercurial import error
from mercurial import util as hgutil
from mercurial.node import bin, hex, nullid

import subprocess
import util

class BaseMap(dict):
    '''A base class for the different type of mappings: author, branch, and
    tags.'''
    def __init__(self, ui, filepath):
        super(BaseMap, self).__init__()
        self._ui = ui

        self._commentre = re.compile(r'((^|[^\\])(\\\\)*)#.*')
        self.syntaxes = ('re', 'glob')

        self._filepath = filepath
        self.load(filepath)

        # Append mappings specified from the commandline. A little
        # magic here: our name in the config mapping is the same as
        # the class name lowercased.
        clmap = util.configpath(self._ui, self.mapname())
        if clmap:
            self.load(clmap)

    @classmethod
    def mapname(cls):
        return cls.__name__.lower()

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

        self._ui.debug('reading %s from %s\n' % (self.mapname() , path))
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
                self._ui.status(msg % (number, self.mapname(), path,
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
                self._ui.debug('adding %s to %s\n' % (src, self.mapname()))
            elif dst != self[src]:
                msg = 'overriding %s: "%s" to "%s" (%s)\n'
                self._ui.status(msg % (self.mapname(), self[src], dst, src))
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

    def __init__(self, ui, filepath, defaulthost, caseignoreauthors,
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

        super(AuthorMap, self).__init__(ui, filepath)

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

    def __init__(self, ui, filepath, endrev=None):
        dict.__init__(self)
        self._filepath = filepath
        self._ui = ui
        self.endrev = endrev
        if os.path.isfile(self._filepath):
            self._load()
        else:
            self._write()

    def _load(self):
        f = open(self._filepath)
        ver = int(f.readline())
        if ver < self.VERSION:
            raise error.Abort(
                'tag map outdated, please run `hg svn rebuildmeta`')
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

    lastpulled = util.fileproperty('_lastpulled', lambda x: x._lastpulled_file,
                                   default=0, deserializer=int)

    def __init__(self, revmap_path, lastpulled_path):
        dict.__init__(self)
        self._filepath = revmap_path
        self._lastpulled_file = lastpulled_path
        self._hashes = None
        # disable iteration to have a consistent interface with SqliteRevMap
        # it's less about performance since RevMap needs iteration internally
        self._allowiter = False

        self.firstpulled = 0
        if os.path.isfile(self._filepath):
            self._load()
        else:
            self._write()

    def hashes(self):
        if self._hashes is None:
            self._hashes = dict((v, k) for (k, v) in self._origiteritems())
        return self._hashes

    def branchedits(self, branch, revnum):
        check = lambda x: x[0][1] == branch and x[0][0] < revnum
        return sorted(filter(check, self._origiteritems()), reverse=True)

    def branchmaxrevnum(self, branch, maxrevnum):
        result = 0
        for num, br in self._origiterkeys():
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
        for key, value in self._origiteritems():
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
        self.lastpulled = lastpulled

    def _readmapfile(self):
        path = self._filepath
        try:
            f = open(path)
        except IOError, err:
            if err.errno != errno.ENOENT:
                raise
            return iter([])
        ver = int(f.readline())
        if ver == SqliteRevMap.VERSION:
            revmap = SqliteRevMap(self._filepath, self._lastpulled_file)
            tmppath = '%s.tmp' % self._filepath
            revmap.exportrevmapv1(tmppath)
            os.rename(tmppath, self._filepath)
            hgutil.unlinkpath(revmap._dbpath)
            hgutil.unlinkpath(revmap._rowcountpath, ignoremissing=True)
            return self._readmapfile()
        if ver != self.VERSION:
            raise hgutil.Abort('revmap too new -- please upgrade')
        return f

    @util.gcdisable
    def _load(self):
        lastpulled = self.lastpulled
        firstpulled = self.firstpulled
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

    @classmethod
    def _wrapitermethods(cls):
        def wrap(orig):
            def wrapper(self, *args, **kwds):
                if not self._allowiter:
                    raise NotImplementedError(
                        'Iteration methods on RevMap are disabled ' +
                        'to avoid performance issues on SqliteRevMap')
                return orig(self, *args, **kwds)
            return wrapper
        methodre = re.compile(r'^_*(?:iter|view)?(?:keys|items|values)?_*$')
        for name in filter(methodre.match, dir(cls)):
            orig = getattr(cls, name)
            setattr(cls, '_orig%s' % name, orig)
            setattr(cls, name, wrap(orig))

RevMap._wrapitermethods()


class SqliteRevMap(collections.MutableMapping):
    """RevMap backed by sqlite3.

    It tries to address performance issues for a very large rev map.
    As such iteration is unavailable for both the map itself and the
    reverse map (self.hashes).

    It migrates from the old RevMap upon first use. Then it will bump the
    version of revmap so RevMap no longer works. The real database is a
    separated file which has a ".db" suffix.
    """

    VERSION = 2

    TABLESCHEMA = [
        '''CREATE TABLE IF NOT EXISTS revmap (
               rev INTEGER NOT NULL,
               branch TEXT NOT NULL DEFAULT '',
               hash BLOB NOT NULL)''',
    ]

    INDEXSCHEMA = [
        'CREATE UNIQUE INDEX IF NOT EXISTS revbranch ON revmap (rev,branch);',
        'CREATE INDEX IF NOT EXISTS hash ON revmap (hash);',
    ]

    # "bytes" in Python 2 will get truncated at '\0' when storing as sqlite
    # blobs. "buffer" does not have this issue. Python 3 does not have "buffer"
    # but "bytes" won't get truncated.
    sqlblobtype = bytes if sys.version_info >= (3, 0) else buffer

    class ReverseRevMap(object):
        # collections.Mapping is not suitable since we don't want 2/3 of
        # its required interfaces: __iter__, __len__.
        def __init__(self, revmap):
            self.revmap = weakref.proxy(revmap)
            self._cache = {}

        def get(self, key, default=None):
            if key not in self._cache:
                result = None
                for row in self.revmap._query(
                    'SELECT rev, branch FROM revmap WHERE hash=?',
                    (SqliteRevMap.sqlblobtype(key),)):
                    result = (row[0], row[1] or None)
                    break
                self._cache[key] = result
            return self._cache[key] or default

        def __contains__(self, key):
            return self.get(key) != None

        def __getitem__(self, key):
            dummy = self._cache
            item = self.get(key, dummy)
            if item == dummy:
                raise KeyError(key)
            else:
                return item

        def keys(self):
            for row in self.revmap._query('SELECT hash FROM revmap'):
                yield bytes(row[0])

    lastpulled = util.fileproperty('_lastpulled', lambda x: x._lastpulledpath,
                                   default=0, deserializer=int)
    rowcount = util.fileproperty('_rowcount', lambda x: x._rowcountpath,
                                 default=0, deserializer=int)

    def __init__(self, revmap_path, lastpulled_path, sqlitepragmas=None):
        self._filepath = revmap_path
        self._dbpath = revmap_path + '.db'
        self._rowcountpath = self._dbpath + '.rowcount'
        self._lastpulledpath = lastpulled_path

        self._db = None
        self._hashes = None
        self._sqlitepragmas = sqlitepragmas
        self.firstpulled = 0
        self._updatefirstlastpulled()
        # __iter__ is expensive and thus disabled by default
        # it should only be enabled for testing
        self._allowiter = False

    def hashes(self):
        if self._hashes is None:
            self._hashes = self.ReverseRevMap(self)
        return self._hashes

    def branchedits(self, branch, revnum):
        return [((r[0], r[1] or None), bytes(r[2])) for r in
                self._query('SELECT rev, branch, hash FROM revmap ' +
                                'WHERE rev < ? AND branch = ? ' +
                                'ORDER BY rev DESC, branch DESC',
                                (revnum, branch or ''))]

    def branchmaxrevnum(self, branch, maxrev):
        for row in self._query('SELECT rev FROM revmap ' +
                               'WHERE rev <= ? AND branch = ? ' +
                               'ORDER By rev DESC LIMIT 1',
                               (maxrev, branch or '')):
            return row[0]
        return 0

    @property
    def lasthash(self):
        for row in self._query('SELECT hash FROM revmap ORDER BY rev DESC'):
            return bytes(row[0])
        return None

    def revhashes(self, revnum):
        for row in self._query('SELECT hash FROM revmap WHERE rev = ?',
                               (revnum,)):
            yield bytes(row[0])

    def clear(self):
        hgutil.unlinkpath(self._filepath, ignoremissing=True)
        hgutil.unlinkpath(self._dbpath, ignoremissing=True)
        hgutil.unlinkpath(self._rowcountpath, ignoremissing=True)
        self._db = None
        self._hashes = None
        self._firstpull = None
        self._lastpull = None

    def batchset(self, items, lastpulled):
        with self._transaction():
            self._insert(items)
        self.lastpulled = lastpulled

    def __getitem__(self, key):
        for row in self._querybykey('SELECT hash', key):
            return bytes(row[0])
        raise KeyError(key)

    def __iter__(self):
        if not self._allowiter:
            raise NotImplementedError(
                'SqliteRevMap.__iter__ is not implemented intentionally ' +
                'to avoid performance issues')
        # collect result to avoid nested transaction issues
        rows = []
        for row in self._query('SELECT rev, branch FROM revmap'):
            rows.append((row[0], row[1] or None))
        return iter(rows)

    def __len__(self):
        # rowcount is faster than "SELECT COUNT(1)". the latter is not O(1)
        return self.rowcount

    def __setitem__(self, key, binha):
        revnum, branch = key
        with self._transaction():
            self._insert([(revnum, branch, binha)])
        if revnum < self.firstpulled or not self.firstpulled:
            self.firstpulled = revnum
        if revnum > self.lastpulled or not self.lastpulled:
            self.lastpulled = revnum
        if self._hashes is not None:
            self._hashes._cache[binha] = key

    def __delitem__(self, key):
        for row in self._querybykey('DELETE', key):
            if self.rowcount > 0:
                self.rowcount -= 1
            return
        # For performance reason, self._hashes is not updated
        raise KeyError(key)

    @contextlib.contextmanager
    def _transaction(self, mode='IMMEDIATE'):
        if self._db is None:
            self._opendb()
        with self._db as db:
            # wait indefinitely for database lock
            while True:
                try:
                    db.execute('BEGIN %s' % mode)
                    break
                except sqlite3.OperationalError as ex:
                    if str(ex) != 'database is locked':
                        raise
            yield db

    def _query(self, sql, params=()):
        with self._transaction() as db:
            cur = db.execute(sql, params)
            try:
                for row in cur:
                    yield row
            finally:
                cur.close()

    def _querybykey(self, prefix, key):
        revnum, branch = key
        return self._query(
            '%s FROM revmap WHERE rev=? AND branch=?'
            % prefix, (revnum, branch or ''))

    def _insert(self, rows):
        # convert to a safe type so '\0' does not truncate the blob
        if rows and type(rows[0][-1]) is not self.sqlblobtype:
            rows = [(r, b, self.sqlblobtype(h)) for r, b, h in rows]
        self._db.executemany(
            'INSERT OR REPLACE INTO revmap (rev, branch, hash) ' +
            'VALUES (?, ?, ?)', rows)
        # If REPLACE happens, rowcount can be wrong. But it is only used to
        # calculate how many revisions pulled, and during pull we don't
        # replace rows. So it is fine.
        self.rowcount += len(rows)

    def _opendb(self):
        '''Open the database and make sure the table is created on demand.'''
        version = None
        try:
            version = int(open(self._filepath).read(2))
        except (ValueError, IOError):
            pass
        if version and version not in [RevMap.VERSION, self.VERSION]:
            raise error.Abort('revmap too new -- please upgrade')

        if self._db:
            self._db.close()

        # if version mismatch, the database is considered invalid
        if version != self.VERSION:
            hgutil.unlinkpath(self._dbpath, ignoremissing=True)

        self._db = sqlite3.connect(self._dbpath)
        self._db.text_factory = bytes

        # cache size affects random accessing (e.g. index building)
        # performance greatly. default is 2MB (2000 KB), we want to have
        # a big enough cache that can hold the entire map.
        cachesize = 2000
        for path, ratio in [(self._filepath, 1.7), (self._dbpath, 1)]:
            if os.path.exists(path):
                cachesize += os.stat(path).st_size * ratio // 1000

        # disable auto-commit. everything is inside a transaction
        self._db.isolation_level = 'DEFERRED'

        with self._transaction('EXCLUSIVE'):
            self._db.execute('PRAGMA cache_size=%d' % (-cachesize))

            # PRAGMA statements provided by the user
            for pragma in (self._sqlitepragmas or []):
                # drop malicious ones
                if re.match(r'\A\w+=\w+\Z', pragma):
                    self._db.execute('PRAGMA %s' % pragma)

            map(self._db.execute, self.TABLESCHEMA)
            if version == RevMap.VERSION:
                self.rowcount = 0
                self._importrevmapv1()
            elif not self.rowcount:
                self.rowcount = self._db.execute(
                    'SELECT COUNT(1) FROM revmap').fetchone()[0]

            # "bulk insert; then create index" is about 2.4x as fast as
            # "create index; then bulk insert" on a large repo
            map(self._db.execute, self.INDEXSCHEMA)

        # write a dummy rev map file with just the revision number
        if version != self.VERSION:
            f = open(self._filepath, 'w')
            f.write('%s\n' % self.VERSION)
            f.close()

    def _updatefirstlastpulled(self):
        sql = 'SELECT rev FROM revmap ORDER BY rev %s LIMIT 1'
        for row in self._query(sql % 'ASC'):
            self.firstpulled = row[0]
        for row in self._query(sql % 'DESC'):
            if row[0] > self.lastpulled:
                self.lastpulled = row[0]

    @util.gcdisable
    def _importrevmapv1(self):
        with open(self._filepath, 'r') as f:
            # 1st line is version
            assert(int(f.readline())) == RevMap.VERSION
            data = {}
            for line in f:
                revnum, ha, branch = line[:-1].split(' ', 2)
                # ignore malicious lines
                if len(ha) != 40:
                    continue
                data[revnum, branch or None] = bin(ha)
            self._insert([(r, b, h) for (r, b), h in data.iteritems()])

    @util.gcdisable
    def exportrevmapv1(self, path):
        with open(path, 'w') as f:
            f.write('%s\n' % RevMap.VERSION)
            for row in self._query('SELECT rev, branch, hash FROM revmap'):
                rev, br, ha = row
                f.write('%s %s %s\n' % (rev, hex(ha), br))


class FileMap(object):

    VERSION = 1

    def __init__(self, ui, filepath):
        '''Initialise a new FileMap.

        The ui argument is used to print diagnostic messages.

        The path argument is the location of the backing store,
        typically .hg/svn/filemap.
        '''
        self._filename = filepath
        self._ui = ui
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
