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
        """build an object to match a set of file patterns

        arguments:
        root - the canonical root of the tree you're matching against
        cwd - the current working directory, if relevant
        patterns - patterns to find
        include - patterns to include
        exclude - patterns to exclude
        default - if a pattern in names has no explicit type, assume this one

        a pattern is one of:
        'glob:<glob>' - a glob relative to cwd
        're:<regexp>' - a regular expression
        'path:<path>' - a path relative to canonroot
        'relglob:<glob>' - an unrooted glob (*.c matches C files in all dirs)
        'relpath:<path>' - a path relative to cwd
        'relre:<regexp>' - a regexp that doesn't have to match the start of a name
        '<something>' - one of the cases above, selected by the dflt_pat argument
        """

        roots = []
        anypats = bool(include or exclude)

        if patterns:
            pats = _normalize(patterns, default, root, cwd)
            roots = _roots(pats)
            anypats = anypats or _anypats(pats)
            pm = _buildmatch(pats, '$')
        if include:
            im = _buildmatch(_normalize(include, 'glob', root, cwd), '(?:/|$)')
        if exclude:
            em = _buildmatch(_normalize(exclude, 'glob', root, cwd), '(?:/|$)')

        if patterns:
            if include:
                if exclude:
                    m = lambda f: im(f) and not em(f) and pm(f)
                else:
                    m = lambda f: im(f) and pm(f)
            else:
                if exclude:
                    m = lambda f: not em(f) and pm(f)
                else:
                    m = pm
        else:
            if include:
                if exclude:
                    m = lambda f: im(f) and not em(f)
                else:
                    m = im
            else:
                if exclude:
                    m = lambda f: not em(f)
                else:
                    m = lambda f: True

        _match.__init__(self, root, cwd, roots, m, anypats)

def patkind(pat):
    return _patsplit(pat, None)[0]

def _patsplit(pat, default):
    """Split a string into an optional pattern kind prefix and the
    actual pattern."""
    if ':' in pat:
        pat, val = pat.split(':', 1)
        if pat in ('re', 'glob', 'path', 'relglob', 'relpath', 'relre'):
            return pat, val
    return default, pat

def _globre(pat):
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
    return res

def _regex(kind, name, tail):
    '''convert a pattern into a regular expression'''
    if not name:
        return ''
    if kind == 're':
        return name
    elif kind == 'path':
        return '^' + re.escape(name) + '(?:/|$)'
    elif kind == 'relglob':
        return '(?:|.*/)' + _globre(name) + tail
    elif kind == 'relpath':
        return re.escape(name) + '(?:/|$)'
    elif kind == 'relre':
        if name.startswith('^'):
            return name
        return '.*' + name
    return _globre(name) + tail

def _buildmatch(pats, tail):
    """build a matching function from a set of patterns"""
    try:
        pat = '(?:%s)' % '|'.join([_regex(k, p, tail) for (k, p) in pats])
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
        a, b = _buildmatch(pats[:l//2], tail), _buildmatch(pats[l//2:], tail)
        return lambda s: a(s) or b(s)
    except re.error:
        for k, p in pats:
            try:
                re.compile('(?:%s)' % _regex(k, p, tail))
            except re.error:
                raise util.Abort("invalid pattern (%s): %s" % (k, p))
        raise util.Abort("invalid pattern")

def _globprefix(pat):
    '''return the non-glob prefix of a path, e.g. foo/* -> foo'''
    root = []
    for p in pat.split('/'):
        if '[' in p or '{' in p or '*' in p or '?' in p:
            break
        root.append(p)
    return '/'.join(root) or '.'

def _normalize(names, default, root, cwd):
    pats = []
    for kind, name in [_patsplit(p, default) for p in names]:
        if kind in ('glob', 'relpath'):
            name = util.canonpath(root, cwd, name)
        elif kind in ('relglob', 'path'):
            name = util.normpath(name)

        pats.append((kind, name))
    return pats

def _roots(patterns):
    r = []
    for kind, name in patterns:
        if kind == 'glob':
            r.append(_globprefix(name))
        elif kind in ('relpath', 'path'):
            r.append(name or '.')
        elif kind == 'relglob':
            r.append('.')
    return r

def _anypats(patterns):
    for kind, name in patterns:
        if kind in ('glob', 're', 'relglob', 'relre'):
            return True
