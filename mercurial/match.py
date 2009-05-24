# match.py - file name matching
#
#  Copyright 2008, 2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

import util, re

class _match(object):
    def __init__(self, root, cwd, files, mf, ap):
        self._root = root
        self._cwd = cwd
        self._files = files
        self._fmap = set(files)
        self.matchfn = mf
        self._anypats = ap
    def __call__(self, fn):
        return self.matchfn(fn)
    def __iter__(self):
        for f in self._files:
            yield f
    def bad(self, f, msg):
        return True
    def dir(self, f):
        pass
    def missing(self, f):
        pass
    def exact(self, f):
        return f in self._fmap
    def rel(self, f):
        return util.pathto(self._root, self._cwd, f)
    def files(self):
        return self._files
    def anypats(self):
        return self._anypats

class always(_match):
    def __init__(self, root, cwd):
        _match.__init__(self, root, cwd, [], lambda f: True, False)

class never(_match):
    def __init__(self, root, cwd):
        _match.__init__(self, root, cwd, [], lambda f: False, False)

class exact(_match):
    def __init__(self, root, cwd, files):
        _match.__init__(self, root, cwd, files, self.exact, False)

class match(_match):
    def __init__(self, root, cwd, patterns, include=[], exclude=[],
                 default='glob'):
        f, mf, ap = _matcher(root, cwd, patterns, include, exclude, default)
        _match.__init__(self, root, cwd, f, mf, ap)

def patkind(pat):
    return _patsplit(pat, None)[0]

def _patsplit(pat, default):
    """Split a string into an optional pattern kind prefix and the
    actual pattern."""
    for prefix in 're', 'glob', 'path', 'relglob', 'relpath', 'relre':
        if pat.startswith(prefix + ':'): return pat.split(':', 1)
    return default, pat

_globchars = set('[{*?')

def _globre(pat, head='^', tail='$'):
    "convert a glob pattern into a regexp"
    i, n = 0, len(pat)
    res = ''
    group = 0
    def peek(): return i < n and pat[i]
    while i < n:
        c = pat[i]
        i = i+1
        if c == '*':
            if peek() == '*':
                i += 1
                res += '.*'
            else:
                res += '[^/]*'
        elif c == '?':
            res += '.'
        elif c == '[':
            j = i
            if j < n and pat[j] in '!]':
                j += 1
            while j < n and pat[j] != ']':
                j += 1
            if j >= n:
                res += '\\['
            else:
                stuff = pat[i:j].replace('\\','\\\\')
                i = j + 1
                if stuff[0] == '!':
                    stuff = '^' + stuff[1:]
                elif stuff[0] == '^':
                    stuff = '\\' + stuff
                res = '%s[%s]' % (res, stuff)
        elif c == '{':
            group += 1
            res += '(?:'
        elif c == '}' and group:
            res += ')'
            group -= 1
        elif c == ',' and group:
            res += '|'
        elif c == '\\':
            p = peek()
            if p:
                i += 1
                res += re.escape(p)
            else:
                res += re.escape(c)
        else:
            res += re.escape(c)
    return head + res + tail

def _matcher(canonroot, cwd='', names=[], inc=[], exc=[], dflt_pat='glob'):
    """build a function to match a set of file patterns

    arguments:
    canonroot - the canonical root of the tree you're matching against
    cwd - the current working directory, if relevant
    names - patterns to find
    inc - patterns to include
    exc - patterns to exclude
    dflt_pat - if a pattern in names has no explicit type, assume this one

    a pattern is one of:
    'glob:<glob>' - a glob relative to cwd
    're:<regexp>' - a regular expression
    'path:<path>' - a path relative to canonroot
    'relglob:<glob>' - an unrooted glob (*.c matches C files in all dirs)
    'relpath:<path>' - a path relative to cwd
    'relre:<regexp>' - a regexp that doesn't have to match the start of a name
    '<something>' - one of the cases above, selected by the dflt_pat argument

    returns:
    a 3-tuple containing
    - list of roots (places where one should start a recursive walk of the fs);
      this often matches the explicit non-pattern names passed in, but also
      includes the initial part of glob: patterns that has no glob characters
    - a bool match(filename) function
    - a bool indicating if any patterns were passed in
    """

    # a common case: no patterns at all
    if not names and not inc and not exc:
        return [], lambda f: True, False

    def contains_glob(name):
        for c in name:
            if c in _globchars: return True
        return False

    def regex(kind, name, tail):
        '''convert a pattern into a regular expression'''
        if not name:
            return ''
        if kind == 're':
            return name
        elif kind == 'path':
            return '^' + re.escape(name) + '(?:/|$)'
        elif kind == 'relglob':
            return _globre(name, '(?:|.*/)', tail)
        elif kind == 'relpath':
            return re.escape(name) + '(?:/|$)'
        elif kind == 'relre':
            if name.startswith('^'):
                return name
            return '.*' + name
        return _globre(name, '', tail)

    def matchfn(pats, tail):
        """build a matching function from a set of patterns"""
        try:
            pat = '(?:%s)' % '|'.join([regex(k, p, tail) for (k, p) in pats])
            if len(pat) > 20000:
                raise OverflowError()
            return re.compile(pat).match
        except OverflowError:
            # We're using a Python with a tiny regex engine and we
            # made it explode, so we'll divide the pattern list in two
            # until it works
            l = len(pats)
            if l < 2:
                raise
            a, b = matchfn(pats[:l//2], tail), matchfn(pats[l//2:], tail)
            return lambda s: a(s) or b(s)
        except re.error:
            for k, p in pats:
                try:
                    re.compile('(?:%s)' % regex(k, p, tail))
                except re.error:
                    raise util.Abort("invalid pattern (%s): %s" % (k, p))
            raise util.Abort("invalid pattern")

    def globprefix(pat):
        '''return the non-glob prefix of a path, e.g. foo/* -> foo'''
        root = []
        for p in pat.split('/'):
            if contains_glob(p): break
            root.append(p)
        return '/'.join(root) or '.'

    def normalizepats(names, default):
        pats = []
        roots = []
        anypats = False
        for kind, name in [_patsplit(p, default) for p in names]:
            if kind in ('glob', 'relpath'):
                name = util.canonpath(canonroot, cwd, name)
            elif kind in ('relglob', 'path'):
                name = util.normpath(name)

            pats.append((kind, name))

            if kind in ('glob', 're', 'relglob', 'relre'):
                anypats = True

            if kind == 'glob':
                root = globprefix(name)
                roots.append(root)
            elif kind in ('relpath', 'path'):
                roots.append(name or '.')
            elif kind == 'relglob':
                roots.append('.')
        return roots, pats, anypats

    roots, pats, anypats = normalizepats(names, dflt_pat)

    if names:
        patmatch = matchfn(pats, '$')
    if inc:
        dummy, inckinds, dummy = normalizepats(inc, 'glob')
        incmatch = matchfn(inckinds, '(?:/|$)')
    if exc:
        dummy, exckinds, dummy = normalizepats(exc, 'glob')
        excmatch = matchfn(exckinds, '(?:/|$)')

    if names:
        if inc:
            if exc:
                m = lambda f: incmatch(f) and not excmatch(f) and patmatch(f)
            else:
                m = lambda f: incmatch(f) and patmatch(f)
        else:
            if exc:
                m = lambda f: not excmatch(f) and patmatch(f)
            else:
                m = patmatch
    else:
        if inc:
            if exc:
                m = lambda f: incmatch(f) and not excmatch(f)
            else:
                m = incmatch
        else:
            if exc:
                m = lambda f: not excmatch(f)
            else:
                m = lambda f: True

    return (roots, m, (inc or exc or anypats) and True)
