# util.py - utility functions and platform specfic implementations
#
# Copyright 2005 K. Thananchayan <thananck@yahoo.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import os, errno
from demandload import *
demandload(globals(), "re")

def unique(g):
    seen = {}
    for f in g:
        if f not in seen:
            seen[f] = 1
            yield f

class CommandError(Exception): pass

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

def matcher(cwd, names, inc, exc, head = ''):
    def patkind(name):
        for prefix in 're:', 'glob:', 'path:':
            if name.startswith(prefix): return name.split(':', 1)
        for c in name:
            if c in _globchars: return 'glob', name
        return 'relpath', name

    cwdsep = cwd + os.sep

    def regex(name, tail):
        '''convert a pattern into a regular expression'''
        kind, name = patkind(name)
        if kind == 're':
            return name
        elif kind == 'path':
            return '^' + re.escape(name) + '$'
        if cwd: name = os.path.join(cwdsep, name)
        name = os.path.normpath(name)
        if name == '.': name = '**'
        return head + globre(name, '', tail)

    def matchfn(pats, tail):
        """build a matching function from a set of patterns"""
        if pats:
            pat = '(?:%s)' % '|'.join([regex(p, tail) for p in pats])
            return re.compile(pat).match

    def globprefix(pat):
        '''return the non-glob prefix of a path, e.g. foo/* -> foo'''
        root = []
        for p in pat.split(os.sep):
            if patkind(p)[0] == 'glob': break
            root.append(p)
        return os.sep.join(root)

    patkinds = map(patkind, names)
    pats = [name for (kind, name) in patkinds if kind != 'relpath']
    files = [name for (kind, name) in patkinds if kind == 'relpath']
    roots = filter(None, map(globprefix, pats)) + files
    if cwd: roots = [cwdsep + r for r in roots]
        
    patmatch = matchfn(pats, '$') or always
    filematch = matchfn(files, '(?:/|$)') or always
    incmatch = matchfn(inc, '(?:/|$)') or always
    excmatch = matchfn(exc, '(?:/|$)') or (lambda fn: False)

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
        raise CommandError(errmsg)

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
            raise IOError("Not a regular file: %r" % srcname)

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
            val = os.STOPSIG(code)
            return "stopped by signal %d" % val, val
        raise ValueError("invalid exit code")
