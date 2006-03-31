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
demandload(globals(), "cStringIO errno popen2 re shutil sys tempfile")
demandload(globals(), "threading time")

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
        if code: raise Abort(_("command '%s' failed: %s") %
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
        raise Abort(_("patch command failed: %s") % explain_exit(code)[0])
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

def patkind(name, dflt_pat='glob'):
    """Split a string into an optional pattern kind prefix and the
    actual pattern."""
    for prefix in 're', 'glob', 'path', 'relglob', 'relpath', 'relre':
        if name.startswith(prefix + ':'): return name.split(':', 1)
    return dflt_pat, name

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

_globchars = {'[': 1, '{': 1, '*': 1, '?': 1}

def pathto(n1, n2):
    '''return the relative path from one place to another.
    this returns a path in the form used by the local filesystem, not hg.'''
    if not n1: return localpath(n2)
    a, b = n1.split('/'), n2.split('/')
    a.reverse()
    b.reverse()
    while a and b and a[-1] == b[-1]:
        a.pop()
        b.pop()
    b.reverse()
    return os.sep.join((['..'] * len(a)) + b)

def canonpath(root, cwd, myname):
    """return the canonical path of myname, given cwd and root"""
    if root == os.sep:
        rootsep = os.sep
    else:
        rootsep = root + os.sep
    name = myname
    if not name.startswith(os.sep):
        name = os.path.join(root, cwd, name)
    name = os.path.normpath(name)
    if name.startswith(rootsep):
        name = name[len(rootsep):]
        audit_path(name)
        return pconvert(name)
    elif name == root:
        return ''
    else:
        raise Abort('%s not under root' % myname)

def matcher(canonroot, cwd='', names=['.'], inc=[], exc=[], head='', src=None):
    return _matcher(canonroot, cwd, names, inc, exc, head, 'glob', src)

def cmdmatcher(canonroot, cwd='', names=['.'], inc=[], exc=[], head='', src=None):
    if os.name == 'nt':
        dflt_pat = 'glob'
    else:
        dflt_pat = 'relpath'
    return _matcher(canonroot, cwd, names, inc, exc, head, dflt_pat, src)

def _matcher(canonroot, cwd, names, inc, exc, head, dflt_pat, src):
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

    def contains_glob(name):
        for c in name:
            if c in _globchars: return True
        return False

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
        if not pats:
            return
        matches = []
        for k, p in pats:
            try:
                pat = '(?:%s)' % regex(k, p, tail)
                matches.append(re.compile(pat).match)
            except re.error:
                if src: raise Abort("%s: invalid pattern (%s): %s" % (src, k, p))
                else: raise Abort("invalid pattern (%s): %s" % (k, p))

        def buildfn(text):
            for m in matches:
                r = m(text)
                if r:
                    return r

        return buildfn

    def globprefix(pat):
        '''return the non-glob prefix of a path, e.g. foo/* -> foo'''
        root = []
        for p in pat.split(os.sep):
            if contains_glob(p): break
            root.append(p)
        return '/'.join(root)

    pats = []
    files = []
    roots = []
    for kind, name in [patkind(p, dflt_pat) for p in names]:
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

def system(cmd, environ={}, cwd=None, onerr=None, errprefix=None):
    '''enhanced shell command execution.
    run with environment maybe modified, maybe in different dir.

    if command fails and onerr is None, return status.  if ui object,
    print error message and return status, else raise onerr object as
    exception.'''
    oldenv = {}
    for k in environ:
        oldenv[k] = os.environ.get(k)
    if cwd is not None:
        oldcwd = os.getcwd()
    try:
        for k, v in environ.iteritems():
            os.environ[k] = str(v)
        if cwd is not None and oldcwd != cwd:
            os.chdir(cwd)
        rc = os.system(cmd)
        if rc and onerr:
            errmsg = '%s %s' % (os.path.basename(cmd.split(None, 1)[0]),
                                explain_exit(rc)[0])
            if errprefix:
                errmsg = '%s: %s' % (errprefix, errmsg)
            try:
                onerr.warn(errmsg + '\n')
            except AttributeError:
                raise onerr(errmsg)
        return rc
    finally:
        for k, v in oldenv.iteritems():
            if v is None:
                del os.environ[k]
            else:
                os.environ[k] = v
        if cwd is not None and oldcwd != cwd:
            os.chdir(oldcwd)

def rename(src, dst):
    """forcibly rename a file"""
    try:
        os.rename(src, dst)
    except:
        os.unlink(dst)
        os.rename(src, dst)

def unlink(f):
    """unlink and remove the directory if it is empty"""
    os.unlink(f)
    # try removing directories that might now be empty
    try: os.removedirs(os.path.dirname(f))
    except: pass

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
                shutil.copy(src, dst)
        else:
            shutil.copy(src, dst)

def audit_path(path):
    """Abort if path contains dangerous components"""
    parts = os.path.normcase(path).split(os.sep)
    if (os.path.splitdrive(path)[0] or parts[0] in ('.hg', '')
        or os.pardir in parts):
        raise Abort(_("path contains illegal component: %s\n") % path)

def opener(base, audit=True):
    """
    return a function that opens files relative to base

    this function is used to hide the details of COW semantics and
    remote file access from higher level code.
    """
    p = base
    audit_p = audit

    def mktempcopy(name):
        d, fn = os.path.split(name)
        fd, temp = tempfile.mkstemp(prefix=fn, dir=d)
        fp = os.fdopen(fd, "wb")
        try:
            fp.write(file(name, "rb").read())
        except:
            try: os.unlink(temp)
            except: pass
            raise
        fp.close()
        st = os.lstat(name)
        os.chmod(temp, st.st_mode)
        return temp

    class atomicfile(file):
        """the file will only be copied on close"""
        def __init__(self, name, mode, atomic=False):
            self.__name = name
            self.temp = mktempcopy(name)
            file.__init__(self, self.temp, mode)
        def close(self):
            if not self.closed:
                file.close(self)
                rename(self.temp, self.__name)
        def __del__(self):
            self.close()

    def o(path, mode="r", text=False, atomic=False):
        if audit_p:
            audit_path(path)
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
                if atomic:
                    return atomicfile(f, mode)
                if nlink > 1:
                    rename(mktempcopy(f), f)
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
        raise OSError(0, _("Hardlinks not supported"))

# Platform specific variants
if os.name == 'nt':
    demandload(globals(), "msvcrt")
    nulldev = 'NUL:'

    class winstdout:
        '''stdout on windows misbehaves if sent through a pipe'''

        def __init__(self, fp):
            self.fp = fp

        def __getattr__(self, key):
            return getattr(self.fp, key)

        def close(self):
            try:
                self.fp.close()
            except: pass

        def write(self, s):
            try:
                return self.fp.write(s)
            except IOError, inst:
                if inst.errno != 0: raise
                self.close()
                raise IOError(errno.EPIPE, 'Broken pipe')

    sys.stdout = winstdout(sys.stdout)

    def os_rcpath():
        '''return default os-specific hgrc search path'''
        try:
            import win32api, win32process
            proc = win32api.GetCurrentProcess()
            filename = win32process.GetModuleFileNameEx(proc, 0)
            systemrc = os.path.join(os.path.dirname(filename), 'mercurial.ini')
        except ImportError:
            systemrc = r'c:\mercurial\mercurial.ini'

        return [systemrc,
                os.path.join(os.path.expanduser('~'), 'mercurial.ini')]

    def parse_patch_output(output_line):
        """parses the output produced by patch and returns the file name"""
        pf = output_line[14:]
        if pf[0] == '`':
            pf = pf[1:-1] # Remove the quotes
        return pf

    try: # Mark Hammond's win32all package allows better functionality on Windows
        import win32api, win32con, win32file, pywintypes

        # create hard links using win32file module
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

        def testpid(pid):
            '''return True if pid is still running or unable to determine, False otherwise'''
            try:
                handle = win32api.OpenProcess(win32con.PROCESS_QUERY_INFORMATION, False, pid) 
                if handle:
                    status = win32process.GetExitCodeProcess(handle)
                    if status == win32con.STILL_ACTIVE:
                        return True
                    else:
                        return False
            except pywintypes.error, details:
                if details[0] == 87: # ERROR_INVALID_PARAMETER
                    return False
            return True

    except ImportError:
        def testpid(pid):
            '''return False if pid dead, True if running or not known'''
            return True

    def is_exec(f, last):
        return last

    def set_exec(f, mode):
        pass

    def set_binary(fd):
        msvcrt.setmode(fd.fileno(), os.O_BINARY)

    def pconvert(path):
        return path.replace("\\", "/")

    def localpath(path):
        return path.replace('/', '\\')

    def normpath(path):
        return pconvert(os.path.normpath(path))

    makelock = _makelock_file
    readlock = _readlock_file

    def explain_exit(code):
        return _("exited with status %d") % code, code

else:
    nulldev = '/dev/null'

    def rcfiles(path):
        rcs = [os.path.join(path, 'hgrc')]
        rcdir = os.path.join(path, 'hgrc.d')
        try:
            rcs.extend([os.path.join(rcdir, f) for f in os.listdir(rcdir)
                        if f.endswith(".rc")])
        except OSError, inst: pass
        return rcs

    def os_rcpath():
        '''return default os-specific hgrc search path'''
        path = []
        if len(sys.argv) > 0:
            path.extend(rcfiles(os.path.dirname(sys.argv[0]) +
                                  '/../etc/mercurial'))
        path.extend(rcfiles('/etc/mercurial'))
        path.append(os.path.expanduser('~/.hgrc'))
        path = [os.path.normpath(f) for f in path]
        return path

    def parse_patch_output(output_line):
        """parses the output produced by patch and returns the file name"""
        pf = output_line[14:]
        if pf.startswith("'") and pf.endswith("'") and pf.find(" ") >= 0:
            pf = pf[1:-1] # Remove the quotes
        return pf

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

    def set_binary(fd):
        pass

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

    def testpid(pid):
        '''return False if pid dead, True if running or not sure'''
        try:
            os.kill(pid, 0)
            return True
        except OSError, inst:
            return inst.errno != errno.ESRCH

    def explain_exit(code):
        """return a 2-tuple (desc, code) describing a process's status"""
        if os.WIFEXITED(code):
            val = os.WEXITSTATUS(code)
            return _("exited with status %d") % val, val
        elif os.WIFSIGNALED(code):
            val = os.WTERMSIG(code)
            return _("killed by signal %d") % val, val
        elif os.WIFSTOPPED(code):
            val = os.WSTOPSIG(code)
            return _("stopped by signal %d") % val, val
        raise ValueError(_("invalid exit code"))

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
            raise ValueError(_("targetsize must be greater than 0, was %d") %
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
    lt = time.localtime()
    if lt[8] == 1 and time.daylight:
        tz = time.altzone
    else:
        tz = time.timezone
    return time.mktime(lt), tz

def datestr(date=None, format='%a %b %d %H:%M:%S %Y', timezone=True):
    """represent a (unixtime, offset) tuple as a localized time.
    unixtime is seconds since the epoch, and offset is the time zone's
    number of seconds away from UTC. if timezone is false, do not
    append time zone to string."""
    t, tz = date or makedate()
    s = time.strftime(format, time.gmtime(float(t) - tz))
    if timezone:
        s += " %+03d%02d" % (-tz / 3600, ((-tz % 3600) / 60))
    return s

def shortuser(user):
    """Return a short representation of a user name or email address."""
    f = user.find('@')
    if f >= 0:
        user = user[:f]
    f = user.find('<')
    if f >= 0:
        user = user[f+1:]
    return user

def walkrepos(path):
    '''yield every hg repository under path, recursively.'''
    def errhandler(err):
        if err.filename == path:
            raise err

    for root, dirs, files in os.walk(path, onerror=errhandler):
        for d in dirs:
            if d == '.hg':
                yield root
                dirs[:] = []
                break

_rcpath = None

def rcpath():
    '''return hgrc search path. if env var HGRCPATH is set, use it.
    for each item in path, if directory, use files ending in .rc,
    else use item.
    make HGRCPATH empty to only look in .hg/hgrc of current repo.
    if no HGRCPATH, use default os-specific path.'''
    global _rcpath
    if _rcpath is None:
        if 'HGRCPATH' in os.environ:
            _rcpath = []
            for p in os.environ['HGRCPATH'].split(os.pathsep):
                if not p: continue
                if os.path.isdir(p):
                    for f in os.listdir(p):
                        if f.endswith('.rc'):
                            _rcpath.append(os.path.join(p, f))
                else:
                    _rcpath.append(p)
        else:
            _rcpath = os_rcpath()
    return _rcpath
