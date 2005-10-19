"""
util.py - Mercurial utility functions and platform specfic implementations

 Copyright 2005 K. Thananchayan <thananck@yahoo.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.

This contains helper routines that are independent of the SCM core and hide
platform-specific details from the core.
"""

import os, errno
from i18n import gettext as _
from demandload import *
demandload(globals(), "re cStringIO shutil popen2 tempfile threading time")

def pipefilter(s, cmd):
    '''filter string S through command CMD, returning its output'''
    (pout, pin) = popen2.popen2(cmd, -1, 'b')
    def writer():
        pin.write(s)
        pin.close()

    # we should use select instead on UNIX, but this will work on most
    # systems, including Windows
    w = threading.Thread(target=writer)
    w.start()
    f = pout.read()
    pout.close()
    w.join()
    return f

def tempfilter(s, cmd):
    '''filter string S through a pair of temporary files with CMD.
    CMD is used as a template to create the real command to be run,
    with the strings INFILE and OUTFILE replaced by the real names of
    the temporary files generated.'''
    inname, outname = None, None
    try:
        infd, inname = tempfile.mkstemp(prefix='hgfin')
        fp = os.fdopen(infd, 'wb')
        fp.write(s)
        fp.close()
        outfd, outname = tempfile.mkstemp(prefix='hgfout')
        os.close(outfd)
        cmd = cmd.replace('INFILE', inname)
        cmd = cmd.replace('OUTFILE', outname)
        code = os.system(cmd)
        if code: raise Abort("command '%s' failed: %s" %
                             (cmd, explain_exit(code)))
        return open(outname, 'rb').read()
    finally:
        try:
            if inname: os.unlink(inname)
        except: pass
        try:
            if outname: os.unlink(outname)
        except: pass

filtertable = {
    'tempfile:': tempfilter,
    'pipe:': pipefilter,
    }

def filter(s, cmd):
    "filter a string through a command that transforms its input to its output"
    for name, fn in filtertable.iteritems():
        if cmd.startswith(name):
            return fn(s, cmd[len(name):].lstrip())
    return pipefilter(s, cmd)

def patch(strip, patchname, ui):
    """apply the patch <patchname> to the working directory.
    a list of patched files is returned"""
    fp = os.popen('patch -p%d < "%s"' % (strip, patchname))
    files = {}
    for line in fp:
        line = line.rstrip()
        ui.status("%s\n" % line)
        if line.startswith('patching file '):
            pf = parse_patch_output(line)
            files.setdefault(pf, 1)
    code = fp.close()
    if code:
        raise Abort("patch command failed: %s" % explain_exit(code)[0])
    return files.keys()

def binary(s):
    """return true if a string is binary data using diff's heuristic"""
    if s and '\0' in s[:4096]:
        return True
    return False

def unique(g):
    """return the uniq elements of iterable g"""
    seen = {}
    for f in g:
        if f not in seen:
            seen[f] = 1
            yield f

class Abort(Exception):
    """Raised if a command needs to print an error and exit."""

def always(fn): return True
def never(fn): return False

def globre(pat, head='^', tail='$'):
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

def canonpath(root, cwd, myname):
    """return the canonical path of myname, given cwd and root"""
    rootsep = root + os.sep
    name = myname
    if not name.startswith(os.sep):
        name = os.path.join(root, cwd, name)
    name = os.path.normpath(name)
    if name.startswith(rootsep):
        return pconvert(name[len(rootsep):])
    elif name == root:
        return ''
    else:
        raise Abort('%s not under root' % myname)

def matcher(canonroot, cwd='', names=['.'], inc=[], exc=[], head=''):
    """build a function to match a set of file patterns

    arguments:
    canonroot - the canonical root of the tree you're matching against
    cwd - the current working directory, if relevant
    names - patterns to find
    inc - patterns to include
    exc - patterns to exclude
    head - a regex to prepend to patterns to control whether a match is rooted

    a pattern is one of:
    'glob:<rooted glob>'
    're:<rooted regexp>'
    'path:<rooted path>'
    'relglob:<relative glob>'
    'relpath:<relative path>'
    'relre:<relative regexp>'
    '<rooted path or regexp>'

    returns:
    a 3-tuple containing
    - list of explicit non-pattern names passed in
    - a bool match(filename) function
    - a bool indicating if any patterns were passed in

    todo:
    make head regex a rooted bool
    """

    def patkind(name):
        for prefix in 're', 'glob', 'path', 'relglob', 'relpath', 'relre':
            if name.startswith(prefix + ':'): return name.split(':', 1)
        for c in name:
            if c in _globchars: return 'glob', name
        return 'relpath', name

    def regex(kind, name, tail):
        '''convert a pattern into a regular expression'''
        if kind == 're':
            return name
        elif kind == 'path':
            return '^' + re.escape(name) + '(?:/|$)'
        elif kind == 'relglob':
            return head + globre(name, '(?:|.*/)', tail)
        elif kind == 'relpath':
            return head + re.escape(name) + tail
        elif kind == 'relre':
            if name.startswith('^'):
                return name
            return '.*' + name
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
            name = canonpath(canonroot, cwd, name)
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

    return (roots,
            lambda fn: (incmatch(fn) and not excmatch(fn) and
                        (fn.endswith('/') or
                         (not pats and not files) or
                         (pats and patmatch(fn)) or
                         (files and filematch(fn)))),
            (inc or exc or (pats and pats != [('glob', '**')])) and True)

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
    """forcibly rename a file"""
    try:
        os.rename(src, dst)
    except:
        os.unlink(dst)
        os.rename(src, dst)

def copyfiles(src, dst, hardlink=None):
    """Copy a directory tree using hardlinks if possible"""

    if hardlink is None:
        hardlink = (os.stat(src).st_dev ==
                    os.stat(os.path.dirname(dst)).st_dev)

    if os.path.isdir(src):
        os.mkdir(dst)
        for name in os.listdir(src):
            srcname = os.path.join(src, name)
            dstname = os.path.join(dst, name)
            copyfiles(srcname, dstname, hardlink)
    else:
        if hardlink:
            try:
                os_link(src, dst)
            except:
                hardlink = False
                shutil.copy2(src, dst)
        else:
            shutil.copy2(src, dst)

def opener(base):
    """
    return a function that opens files relative to base

    this function is used to hide the details of COW semantics and
    remote file access from higher level code.
    """
    p = base
    def o(path, mode="r", text=False):
        f = os.path.join(p, path)

        if not text:
            mode += "b" # for that other OS

        if mode[0] != "r":
            try:
                nlink = nlinks(f)
            except OSError:
                d = os.path.dirname(f)
                if not os.path.isdir(d):
                    os.makedirs(d)
            else:
                if nlink > 1:
                    file(f + ".tmp", "wb").write(file(f, "rb").read())
                    rename(f+".tmp", f)

        return file(f, mode)

    return o

def _makelock_file(info, pathname):
    ld = os.open(pathname, os.O_CREAT | os.O_WRONLY | os.O_EXCL)
    os.write(ld, info)
    os.close(ld)

def _readlock_file(pathname):
    return file(pathname).read()

def nlinks(pathname):
    """Return number of hardlinks for the given file."""
    return os.stat(pathname).st_nlink

if hasattr(os, 'link'):
    os_link = os.link
else:
    def os_link(src, dst):
        raise OSError(0, "Hardlinks not supported")

# Platform specific variants
if os.name == 'nt':
    nulldev = 'NUL:'
    
    try:
        import win32api, win32process
        filename = win32process.GetModuleFileNameEx(win32api.GetCurrentProcess(), 0)
        systemrc = os.path.join(os.path.dirname(filename), 'mercurial.ini')
        
    except ImportError:
        systemrc = r'c:\mercurial\mercurial.ini'
        pass

    rcpath = (systemrc,
              os.path.join(os.path.expanduser('~'), 'mercurial.ini'))

    def parse_patch_output(output_line):
        """parses the output produced by patch and returns the file name"""
        pf = output_line[14:]
        if pf[0] == '`':
            pf = pf[1:-1] # Remove the quotes
        return pf

    try: # ActivePython can create hard links using win32file module
        import win32file

        def os_link(src, dst): # NB will only succeed on NTFS
            win32file.CreateHardLink(dst, src)

        def nlinks(pathname):
            """Return number of hardlinks for the given file."""
            try:
                fh = win32file.CreateFile(pathname,
                    win32file.GENERIC_READ, win32file.FILE_SHARE_READ,
                    None, win32file.OPEN_EXISTING, 0, None)
                res = win32file.GetFileInformationByHandle(fh)
                fh.Close()
                return res[7]
            except:
                return os.stat(pathname).st_nlink

    except ImportError:
        pass

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

    hgrcd = '/etc/mercurial/hgrc.d'
    hgrcs = []
    if os.path.isdir(hgrcd):
        hgrcs = [f for f in os.listdir(hgrcd) if f.endswith(".rc")]
    rcpath = map(os.path.normpath, hgrcs +
                 ['/etc/mercurial/hgrc', os.path.expanduser('~/.hgrc')])

    def parse_patch_output(output_line):
        """parses the output produced by patch and returns the file name"""
        return output_line[14:]

    def is_exec(f, last):
        """check whether a file is executable"""
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

class chunkbuffer(object):
    """Allow arbitrary sized chunks of data to be efficiently read from an
    iterator over chunks of arbitrary size."""

    def __init__(self, in_iter, targetsize = 2**16):
        """in_iter is the iterator that's iterating over the input chunks.
        targetsize is how big a buffer to try to maintain."""
        self.in_iter = iter(in_iter)
        self.buf = ''
        self.targetsize = int(targetsize)
        if self.targetsize <= 0:
            raise ValueError("targetsize must be greater than 0, was %d" %
                             targetsize)
        self.iterempty = False

    def fillbuf(self):
        """Ignore target size; read every chunk from iterator until empty."""
        if not self.iterempty:
            collector = cStringIO.StringIO()
            collector.write(self.buf)
            for ch in self.in_iter:
                collector.write(ch)
            self.buf = collector.getvalue()
            self.iterempty = True

    def read(self, l):
        """Read L bytes of data from the iterator of chunks of data.
        Returns less than L bytes if the iterator runs dry."""
        if l > len(self.buf) and not self.iterempty:
            # Clamp to a multiple of self.targetsize
            targetsize = self.targetsize * ((l // self.targetsize) + 1)
            collector = cStringIO.StringIO()
            collector.write(self.buf)
            collected = len(self.buf)
            for chunk in self.in_iter:
                collector.write(chunk)
                collected += len(chunk)
                if collected >= targetsize:
                    break
            if collected < targetsize:
                self.iterempty = True
            self.buf = collector.getvalue()
        s, self.buf = self.buf[:l], buffer(self.buf, l)
        return s

def filechunkiter(f, size = 65536):
    """Create a generator that produces all the data in the file size
    (default 65536) bytes at a time.  Chunks may be less than size
    bytes if the chunk is the last chunk in the file, or the file is a
    socket or some other type of file that sometimes reads less data
    than is requested."""
    s = f.read(size)
    while len(s) > 0:
        yield s
        s = f.read(size)

def makedate():
    t = time.time()
    if time.daylight: tz = time.altzone
    else: tz = time.timezone
    return t, tz

def datestr(date=None, format='%c'):
    """represent a (unixtime, offset) tuple as a localized time.
    unixtime is seconds since the epoch, and offset is the time zone's
    number of seconds away from UTC."""
    t, tz = date or makedate()
    return ("%s %+03d%02d" %
            (time.strftime(format, time.gmtime(float(t) - tz)),
             -tz / 3600,
             ((-tz % 3600) / 60)))
