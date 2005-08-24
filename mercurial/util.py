# util.py - utility functions and platform specfic implementations
#
# Copyright 2005 K. Thananchayan <thananck@yahoo.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, errno
from demandload import *
demandload(globals(), "re")

def binary(s):
    if s and '\0' in s[:4096]:
        return True
    return False

def unique(g):
    seen = {}
    for f in g:
        if f not in seen:
            seen[f] = 1
            yield f

class Abort(Exception):
    """Raised if a command needs to print an error and exit."""

def always(fn): return True
def never(fn): return False

def globre(pat, head = '^', tail = '$'):
    "convert a glob pattern into a regexp"
    i, n = 0, len(pat)
    res = ''
    group = False
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
            group = True
            res += '(?:'
        elif c == '}' and group:
            res += ')'
            group = False
        elif c == ',' and group:
            res += '|'
        else:
            res += re.escape(c)
    return head + res + tail

_globchars = {'[': 1, '{': 1, '*': 1, '?': 1}

def pathto(n1, n2):
    '''return the relative path from one place to another.
    this returns a path in the form used by the local filesystem, not hg.'''
    if not n1: return localpath(n2)
    a, b = n1.split('/'), n2.split('/')
    a.reverse(), b.reverse()
    while a and b and a[-1] == b[-1]:
        a.pop(), b.pop()
    b.reverse()
    return os.sep.join((['..'] * len(a)) + b)

def canonpath(repo, cwd, myname):
    rootsep = repo.root + os.sep
    name = myname
    if not name.startswith(os.sep):
        name = os.path.join(repo.root, cwd, name)
    name = os.path.normpath(name)
    if name.startswith(rootsep):
        return pconvert(name[len(rootsep):])
    elif name == repo.root:
        return ''
    else:
        raise Abort('%s not under repository root' % myname)

def matcher(repo, cwd, names, inc, exc, head = ''):
    def patkind(name):
        for prefix in 're:', 'glob:', 'path:', 'relpath:':
            if name.startswith(prefix): return name.split(':', 1)
        for c in name:
            if c in _globchars: return 'glob', name
        return 'relpath', name

    def regex(kind, name, tail):
        '''convert a pattern into a regular expression'''
        if kind == 're':
            return name
        elif kind == 'path':
            return '^' + re.escape(name) + '(?:/|$)'
        elif kind == 'relpath':
            return head + re.escape(name) + tail
        return head + globre(name, '', tail)

    def matchfn(pats, tail):
        """build a matching function from a set of patterns"""
        if pats:
            pat = '(?:%s)' % '|'.join([regex(k, p, tail) for (k, p) in pats])
            return re.compile(pat).match

    def globprefix(pat):
        '''return the non-glob prefix of a path, e.g. foo/* -> foo'''
        root = []
        for p in pat.split(os.sep):
            if patkind(p)[0] == 'glob': break
            root.append(p)
        return '/'.join(root)

    pats = []
    files = []
    roots = []
    for kind, name in map(patkind, names):
        if kind in ('glob', 'relpath'):
            name = canonpath(repo, cwd, name)
            if name == '':
                kind, name = 'glob', '**'
        if kind in ('glob', 'path', 're'):
            pats.append((kind, name))
        if kind == 'glob':
            root = globprefix(name)
            if root: roots.append(root)
        elif kind == 'relpath':
            files.append((kind, name))
            roots.append(name)

    patmatch = matchfn(pats, '$') or always
    filematch = matchfn(files, '(?:/|$)') or always
    incmatch = always
    if inc:
        incmatch = matchfn(map(patkind, inc), '(?:/|$)')
    excmatch = lambda fn: False
    if exc:
        excmatch = matchfn(map(patkind, exc), '(?:/|$)')

    return roots, lambda fn: (incmatch(fn) and not excmatch(fn) and
                              (fn.endswith('/') or
                               (not pats and not files) or
                               (pats and patmatch(fn)) or
                               (files and filematch(fn))))

def system(cmd, errprefix=None):
    """execute a shell command that must succeed"""
    rc = os.system(cmd)
    if rc:
        errmsg = "%s %s" % (os.path.basename(cmd.split(None, 1)[0]),
                            explain_exit(rc)[0])
        if errprefix:
            errmsg = "%s: %s" % (errprefix, errmsg)
        raise Abort(errmsg)

def rename(src, dst):
    try:
        os.rename(src, dst)
    except:
        os.unlink(dst)
        os.rename(src, dst)

def copytree(src, dst, copyfile):
    """Copy a directory tree, files are copied using 'copyfile'."""
    names = os.listdir(src)
    os.mkdir(dst)

    for name in names:
        srcname = os.path.join(src, name)
        dstname = os.path.join(dst, name)
        if os.path.isdir(srcname):
            copytree(srcname, dstname, copyfile)
        elif os.path.isfile(srcname):
            copyfile(srcname, dstname)
        else:
            pass

def _makelock_file(info, pathname):
    ld = os.open(pathname, os.O_CREAT | os.O_WRONLY | os.O_EXCL)
    os.write(ld, info)
    os.close(ld)

def _readlock_file(pathname):
    return file(pathname).read()

# Platfor specific varients
if os.name == 'nt':
    nulldev = 'NUL:'

    def is_exec(f, last):
        return last

    def set_exec(f, mode):
        pass

    def pconvert(path):
        return path.replace("\\", "/")

    def localpath(path):
        return path.replace('/', '\\')

    def normpath(path):
        return pconvert(os.path.normpath(path))

    makelock = _makelock_file
    readlock = _readlock_file

    def explain_exit(code):
        return "exited with status %d" % code, code

else:
    nulldev = '/dev/null'

    def is_exec(f, last):
        return (os.stat(f).st_mode & 0100 != 0)

    def set_exec(f, mode):
        s = os.stat(f).st_mode
        if (s & 0100 != 0) == mode:
            return
        if mode:
            # Turn on +x for every +r bit when making a file executable
            # and obey umask.
            umask = os.umask(0)
            os.umask(umask)
            os.chmod(f, s | (s & 0444) >> 2 & ~umask)
        else:
            os.chmod(f, s & 0666)

    def pconvert(path):
        return path

    def localpath(path):
        return path

    normpath = os.path.normpath

    def makelock(info, pathname):
        try:
            os.symlink(info, pathname)
        except OSError, why:
            if why.errno == errno.EEXIST:
                raise
            else:
                _makelock_file(info, pathname)

    def readlock(pathname):
        try:
            return os.readlink(pathname)
        except OSError, why:
            if why.errno == errno.EINVAL:
                return _readlock_file(pathname)
            else:
                raise

    def explain_exit(code):
        """return a 2-tuple (desc, code) describing a process's status"""
        if os.WIFEXITED(code):
            val = os.WEXITSTATUS(code)
            return "exited with status %d" % val, val
        elif os.WIFSIGNALED(code):
            val = os.WTERMSIG(code)
            return "killed by signal %d" % val, val
        elif os.WIFSTOPPED(code):
            val = os.WSTOPSIG(code)
            return "stopped by signal %d" % val, val
        raise ValueError("invalid exit code")
