# store.py - repository store handling for Mercurial
#
# Copyright 2008 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import osutil, util
import os, stat

_sha = util.sha1

# This avoids a collision between a file named foo and a dir named
# foo.i or foo.d
def encodedir(path):
    if not path.startswith('data/'):
        return path
    return (path
            .replace(".hg/", ".hg.hg/")
            .replace(".i/", ".i.hg/")
            .replace(".d/", ".d.hg/"))

def decodedir(path):
    if not path.startswith('data/') or ".hg/" not in path:
        return path
    return (path
            .replace(".d.hg/", ".d/")
            .replace(".i.hg/", ".i/")
            .replace(".hg.hg/", ".hg/"))

def _buildencodefun():
    e = '_'
    win_reserved = [ord(x) for x in '\\:*?"<>|']
    cmap = dict([(chr(x), chr(x)) for x in xrange(127)])
    for x in (range(32) + range(126, 256) + win_reserved):
        cmap[chr(x)] = "~%02x" % x
    for x in range(ord("A"), ord("Z")+1) + [ord(e)]:
        cmap[chr(x)] = e + chr(x).lower()
    dmap = {}
    for k, v in cmap.iteritems():
        dmap[v] = k
    def decode(s):
        i = 0
        while i < len(s):
            for l in xrange(1, 4):
                try:
                    yield dmap[s[i:i + l]]
                    i += l
                    break
                except KeyError:
                    pass
            else:
                raise KeyError
    return (lambda s: "".join([cmap[c] for c in encodedir(s)]),
            lambda s: decodedir("".join(list(decode(s)))))

encodefilename, decodefilename = _buildencodefun()

def _build_lower_encodefun():
    win_reserved = [ord(x) for x in '\\:*?"<>|']
    cmap = dict([(chr(x), chr(x)) for x in xrange(127)])
    for x in (range(32) + range(126, 256) + win_reserved):
        cmap[chr(x)] = "~%02x" % x
    for x in range(ord("A"), ord("Z")+1):
        cmap[chr(x)] = chr(x).lower()
    return lambda s: "".join([cmap[c] for c in s])

lowerencode = _build_lower_encodefun()

_windows_reserved_filenames = '''con prn aux nul
    com1 com2 com3 com4 com5 com6 com7 com8 com9
    lpt1 lpt2 lpt3 lpt4 lpt5 lpt6 lpt7 lpt8 lpt9'''.split()
def _auxencode(path, dotencode):
    res = []
    for n in path.split('/'):
        if n:
            base = n.split('.')[0]
            if base and (base in _windows_reserved_filenames):
                # encode third letter ('aux' -> 'au~78')
                ec = "~%02x" % ord(n[2])
                n = n[0:2] + ec + n[3:]
            if n[-1] in '. ':
                # encode last period or space ('foo...' -> 'foo..~2e')
                n = n[:-1] + "~%02x" % ord(n[-1])
            if dotencode and n[0] in '. ':
                n = "~%02x" % ord(n[0]) + n[1:]
        res.append(n)
    return '/'.join(res)

MAX_PATH_LEN_IN_HGSTORE = 120
DIR_PREFIX_LEN = 8
_MAX_SHORTENED_DIRS_LEN = 8 * (DIR_PREFIX_LEN + 1) - 4
def _hybridencode(path, auxencode):
    '''encodes path with a length limit

    Encodes all paths that begin with 'data/', according to the following.

    Default encoding (reversible):

    Encodes all uppercase letters 'X' as '_x'. All reserved or illegal
    characters are encoded as '~xx', where xx is the two digit hex code
    of the character (see encodefilename).
    Relevant path components consisting of Windows reserved filenames are
    masked by encoding the third character ('aux' -> 'au~78', see auxencode).

    Hashed encoding (not reversible):

    If the default-encoded path is longer than MAX_PATH_LEN_IN_HGSTORE, a
    non-reversible hybrid hashing of the path is done instead.
    This encoding uses up to DIR_PREFIX_LEN characters of all directory
    levels of the lowerencoded path, but not more levels than can fit into
    _MAX_SHORTENED_DIRS_LEN.
    Then follows the filler followed by the sha digest of the full path.
    The filler is the beginning of the basename of the lowerencoded path
    (the basename is everything after the last path separator). The filler
    is as long as possible, filling in characters from the basename until
    the encoded path has MAX_PATH_LEN_IN_HGSTORE characters (or all chars
    of the basename have been taken).
    The extension (e.g. '.i' or '.d') is preserved.

    The string 'data/' at the beginning is replaced with 'dh/', if the hashed
    encoding was used.
    '''
    if not path.startswith('data/'):
        return path
    # escape directories ending with .i and .d
    path = encodedir(path)
    ndpath = path[len('data/'):]
    res = 'data/' + auxencode(encodefilename(ndpath))
    if len(res) > MAX_PATH_LEN_IN_HGSTORE:
        digest = _sha(path).hexdigest()
        aep = auxencode(lowerencode(ndpath))
        _root, ext = os.path.splitext(aep)
        parts = aep.split('/')
        basename = parts[-1]
        sdirs = []
        for p in parts[:-1]:
            d = p[:DIR_PREFIX_LEN]
            if d[-1] in '. ':
                # Windows can't access dirs ending in period or space
                d = d[:-1] + '_'
            t = '/'.join(sdirs) + '/' + d
            if len(t) > _MAX_SHORTENED_DIRS_LEN:
                break
            sdirs.append(d)
        dirs = '/'.join(sdirs)
        if len(dirs) > 0:
            dirs += '/'
        res = 'dh/' + dirs + digest + ext
        space_left = MAX_PATH_LEN_IN_HGSTORE - len(res)
        if space_left > 0:
            filler = basename[:space_left]
            res = 'dh/' + dirs + filler + digest + ext
    return res

def _calcmode(path):
    try:
        # files in .hg/ will be created using this mode
        mode = os.stat(path).st_mode
            # avoid some useless chmods
        if (0777 & ~util.umask) == (0777 & mode):
            mode = None
    except OSError:
        mode = None
    return mode

_data = 'data 00manifest.d 00manifest.i 00changelog.d 00changelog.i'

class basicstore(object):
    '''base class for local repository stores'''
    def __init__(self, path, opener):
        self.path = path
        self.createmode = _calcmode(path)
        op = opener(self.path)
        op.createmode = self.createmode
        self.opener = lambda f, *args, **kw: op(encodedir(f), *args, **kw)

    def join(self, f):
        return self.path + '/' + encodedir(f)

    def _walk(self, relpath, recurse):
        '''yields (unencoded, encoded, size)'''
        path = self.path
        if relpath:
            path += '/' + relpath
        striplen = len(self.path) + 1
        l = []
        if os.path.isdir(path):
            visit = [path]
            while visit:
                p = visit.pop()
                for f, kind, st in osutil.listdir(p, stat=True):
                    fp = p + '/' + f
                    if kind == stat.S_IFREG and f[-2:] in ('.d', '.i'):
                        n = util.pconvert(fp[striplen:])
                        l.append((decodedir(n), n, st.st_size))
                    elif kind == stat.S_IFDIR and recurse:
                        visit.append(fp)
        return sorted(l)

    def datafiles(self):
        return self._walk('data', True)

    def walk(self):
        '''yields (unencoded, encoded, size)'''
        # yield data files first
        for x in self.datafiles():
            yield x
        # yield manifest before changelog
        for x in reversed(self._walk('', False)):
            yield x

    def copylist(self):
        return ['requires'] + _data.split()

    def write(self):
        pass

class encodedstore(basicstore):
    def __init__(self, path, opener):
        self.path = path + '/store'
        self.createmode = _calcmode(self.path)
        op = opener(self.path)
        op.createmode = self.createmode
        self.opener = lambda f, *args, **kw: op(encodefilename(f), *args, **kw)

    def datafiles(self):
        for a, b, size in self._walk('data', True):
            try:
                a = decodefilename(a)
            except KeyError:
                a = None
            yield a, b, size

    def join(self, f):
        return self.path + '/' + encodefilename(f)

    def copylist(self):
        return (['requires', '00changelog.i'] +
                ['store/' + f for f in _data.split()])

class fncache(object):
    # the filename used to be partially encoded
    # hence the encodedir/decodedir dance
    def __init__(self, opener):
        self.opener = opener
        self.entries = None
        self._dirty = False

    def _load(self):
        '''fill the entries from the fncache file'''
        self.entries = set()
        self._dirty = False
        try:
            fp = self.opener('fncache', mode='rb')
        except IOError:
            # skip nonexistent file
            return
        for n, line in enumerate(fp):
            if (len(line) < 2) or (line[-1] != '\n'):
                t = _('invalid entry in fncache, line %s') % (n + 1)
                raise util.Abort(t)
            self.entries.add(decodedir(line[:-1]))
        fp.close()

    def rewrite(self, files):
        fp = self.opener('fncache', mode='wb')
        for p in files:
            fp.write(encodedir(p) + '\n')
        fp.close()
        self.entries = set(files)
        self._dirty = False

    def write(self):
        if not self._dirty:
            return
        fp = self.opener('fncache', mode='wb', atomictemp=True)
        for p in self.entries:
            fp.write(encodedir(p) + '\n')
        fp.rename()
        self._dirty = False

    def add(self, fn):
        if self.entries is None:
            self._load()
        if fn not in self.entries:
            self._dirty = True
            self.entries.add(fn)

    def __contains__(self, fn):
        if self.entries is None:
            self._load()
        return fn in self.entries

    def __iter__(self):
        if self.entries is None:
            self._load()
        return iter(self.entries)

class fncachestore(basicstore):
    def __init__(self, path, opener, encode):
        self.encode = encode
        self.path = path + '/store'
        self.createmode = _calcmode(self.path)
        op = opener(self.path)
        op.createmode = self.createmode
        fnc = fncache(op)
        self.fncache = fnc

        def fncacheopener(path, mode='r', *args, **kw):
            if mode not in ('r', 'rb') and path.startswith('data/'):
                fnc.add(path)
            return op(self.encode(path), mode, *args, **kw)
        self.opener = fncacheopener

    def join(self, f):
        return self.path + '/' + self.encode(f)

    def datafiles(self):
        rewrite = False
        existing = []
        spath = self.path
        for f in self.fncache:
            ef = self.encode(f)
            try:
                st = os.stat(spath + '/' + ef)
                yield f, ef, st.st_size
                existing.append(f)
            except OSError:
                # nonexistent entry
                rewrite = True
        if rewrite:
            # rewrite fncache to remove nonexistent entries
            # (may be caused by rollback / strip)
            self.fncache.rewrite(existing)

    def copylist(self):
        d = ('data dh fncache'
             ' 00manifest.d 00manifest.i 00changelog.d 00changelog.i')
        return (['requires', '00changelog.i'] +
                ['store/' + f for f in d.split()])

    def write(self):
        self.fncache.write()

def store(requirements, path, opener):
    if 'store' in requirements:
        if 'fncache' in requirements:
            auxencode = lambda f: _auxencode(f, 'dotencode' in requirements)
            encode = lambda f: _hybridencode(f, auxencode)
            return fncachestore(path, opener, encode)
        return encodedstore(path, opener)
    return basicstore(path, opener)
