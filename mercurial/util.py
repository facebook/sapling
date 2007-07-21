"""
util.py - Mercurial utility functions and platform specfic implementations

 Copyright 2005 K. Thananchayan <thananck@yahoo.com>
 Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
 Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.

This contains helper routines that are independent of the SCM core and hide
platform-specific details from the core.
"""

from i18n import _
import cStringIO, errno, getpass, popen2, re, shutil, sys, tempfile
import os, threading, time, calendar, ConfigParser, locale, glob

try:
    set = set
    frozenset = frozenset
except NameError:
    from sets import Set as set, ImmutableSet as frozenset

try:
    _encoding = os.environ.get("HGENCODING")
    if sys.platform == 'darwin' and not _encoding:
        # On darwin, getpreferredencoding ignores the locale environment and
        # always returns mac-roman. We override this if the environment is
        # not C (has been customized by the user).
        locale.setlocale(locale.LC_CTYPE, '')
        _encoding = locale.getlocale()[1]
    if not _encoding:
        _encoding = locale.getpreferredencoding() or 'ascii'
except locale.Error:
    _encoding = 'ascii'
_encodingmode = os.environ.get("HGENCODINGMODE", "strict")
_fallbackencoding = 'ISO-8859-1'

def tolocal(s):
    """
    Convert a string from internal UTF-8 to local encoding

    All internal strings should be UTF-8 but some repos before the
    implementation of locale support may contain latin1 or possibly
    other character sets. We attempt to decode everything strictly
    using UTF-8, then Latin-1, and failing that, we use UTF-8 and
    replace unknown characters.
    """
    for e in ('UTF-8', _fallbackencoding):
        try:
            u = s.decode(e) # attempt strict decoding
            return u.encode(_encoding, "replace")
        except LookupError, k:
            raise Abort(_("%s, please check your locale settings") % k)
        except UnicodeDecodeError:
            pass
    u = s.decode("utf-8", "replace") # last ditch
    return u.encode(_encoding, "replace")

def fromlocal(s):
    """
    Convert a string from the local character encoding to UTF-8

    We attempt to decode strings using the encoding mode set by
    HGENCODINGMODE, which defaults to 'strict'. In this mode, unknown
    characters will cause an error message. Other modes include
    'replace', which replaces unknown characters with a special
    Unicode character, and 'ignore', which drops the character.
    """
    try:
        return s.decode(_encoding, _encodingmode).encode("utf-8")
    except UnicodeDecodeError, inst:
        sub = s[max(0, inst.start-10):inst.start+10]
        raise Abort("decoding near '%s': %s!" % (sub, inst))
    except LookupError, k:
        raise Abort(_("%s, please check your locale settings") % k)

def locallen(s):
    """Find the length in characters of a local string"""
    return len(s.decode(_encoding, "replace"))

def localsub(s, a, b=None):
    try:
        u = s.decode(_encoding, _encodingmode)
        if b is not None:
            u = u[a:b]
        else:
            u = u[:a]
        return u.encode(_encoding, _encodingmode)
    except UnicodeDecodeError, inst:
        sub = s[max(0, inst.start-10), inst.start+10]
        raise Abort(_("decoding near '%s': %s!") % (sub, inst))

# used by parsedate
defaultdateformats = (
    '%Y-%m-%d %H:%M:%S',
    '%Y-%m-%d %I:%M:%S%p',
    '%Y-%m-%d %H:%M',
    '%Y-%m-%d %I:%M%p',
    '%Y-%m-%d',
    '%m-%d',
    '%m/%d',
    '%m/%d/%y',
    '%m/%d/%Y',
    '%a %b %d %H:%M:%S %Y',
    '%a %b %d %I:%M:%S%p %Y',
    '%a, %d %b %Y %H:%M:%S',        #  GNU coreutils "/bin/date --rfc-2822"
    '%b %d %H:%M:%S %Y',
    '%b %d %I:%M:%S%p %Y',
    '%b %d %H:%M:%S',
    '%b %d %I:%M:%S%p',
    '%b %d %H:%M',
    '%b %d %I:%M%p',
    '%b %d %Y',
    '%b %d',
    '%H:%M:%S',
    '%I:%M:%SP',
    '%H:%M',
    '%I:%M%p',
)

extendeddateformats = defaultdateformats + (
    "%Y",
    "%Y-%m",
    "%b",
    "%b %Y",
    )

class SignalInterrupt(Exception):
    """Exception raised on SIGTERM and SIGHUP."""

# differences from SafeConfigParser:
# - case-sensitive keys
# - allows values that are not strings (this means that you may not
#   be able to save the configuration to a file)
class configparser(ConfigParser.SafeConfigParser):
    def optionxform(self, optionstr):
        return optionstr

    def set(self, section, option, value):
        return ConfigParser.ConfigParser.set(self, section, option, value)

    def _interpolate(self, section, option, rawval, vars):
        if not isinstance(rawval, basestring):
            return rawval
        return ConfigParser.SafeConfigParser._interpolate(self, section,
                                                          option, rawval, vars)

def cachefunc(func):
    '''cache the result of function calls'''
    # XXX doesn't handle keywords args
    cache = {}
    if func.func_code.co_argcount == 1:
        # we gain a small amount of time because
        # we don't need to pack/unpack the list
        def f(arg):
            if arg not in cache:
                cache[arg] = func(arg)
            return cache[arg]
    else:
        def f(*args):
            if args not in cache:
                cache[args] = func(*args)
            return cache[args]

    return f

def pipefilter(s, cmd):
    '''filter string S through command CMD, returning its output'''
    (pin, pout) = os.popen2(cmd, 'b')
    def writer():
        try:
            pin.write(s)
            pin.close()
        except IOError, inst:
            if inst.errno != errno.EPIPE:
                raise

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
        infd, inname = tempfile.mkstemp(prefix='hg-filter-in-')
        fp = os.fdopen(infd, 'wb')
        fp.write(s)
        fp.close()
        outfd, outname = tempfile.mkstemp(prefix='hg-filter-out-')
        os.close(outfd)
        cmd = cmd.replace('INFILE', inname)
        cmd = cmd.replace('OUTFILE', outname)
        code = os.system(cmd)
        if sys.platform == 'OpenVMS' and code & 1:
            code = 0
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

def binary(s):
    """return true if a string is binary data using diff's heuristic"""
    if s and '\0' in s[:4096]:
        return True
    return False

def unique(g):
    """return the uniq elements of iterable g"""
    seen = {}
    l = []
    for f in g:
        if f not in seen:
            seen[f] = 1
            l.append(f)
    return l

class Abort(Exception):
    """Raised if a command needs to print an error and exit."""

class UnexpectedOutput(Abort):
    """Raised to print an error with part of output and exit."""

def always(fn): return True
def never(fn): return False

def expand_glob(pats):
    '''On Windows, expand the implicit globs in a list of patterns'''
    if os.name != 'nt':
        return list(pats)
    ret = []
    for p in pats:
        kind, name = patkind(p, None)
        if kind is None:
            globbed = glob.glob(name)
            if globbed:
                ret.extend(globbed)
                continue
            # if we couldn't expand the glob, just keep it around
        ret.append(p)
    return ret

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

def pathto(root, n1, n2):
    '''return the relative path from one place to another.
    root should use os.sep to separate directories
    n1 should use os.sep to separate directories
    n2 should use "/" to separate directories
    returns an os.sep-separated path.

    If n1 is a relative path, it's assumed it's
    relative to root.
    n2 should always be relative to root.
    '''
    if not n1: return localpath(n2)
    if os.path.isabs(n1):
        if os.path.splitdrive(root)[0] != os.path.splitdrive(n1)[0]:
            return os.path.join(root, localpath(n2))
        n2 = '/'.join((pconvert(root), n2))
    a, b = n1.split(os.sep), n2.split('/')
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
    elif root.endswith(os.sep):
        rootsep = root
    else:
        rootsep = root + os.sep
    name = myname
    if not os.path.isabs(name):
        name = os.path.join(root, cwd, name)
    name = os.path.normpath(name)
    if name != rootsep and name.startswith(rootsep):
        name = name[len(rootsep):]
        audit_path(name)
        return pconvert(name)
    elif name == root:
        return ''
    else:
        # Determine whether `name' is in the hierarchy at or beneath `root',
        # by iterating name=dirname(name) until that causes no change (can't
        # check name == '/', because that doesn't work on windows).  For each
        # `name', compare dev/inode numbers.  If they match, the list `rel'
        # holds the reversed list of components making up the relative file
        # name we want.
        root_st = os.stat(root)
        rel = []
        while True:
            try:
                name_st = os.stat(name)
            except OSError:
                break
            if samestat(name_st, root_st):
                if not rel:
                    # name was actually the same as root (maybe a symlink)
                    return ''
                rel.reverse()
                name = os.path.join(*rel)
                audit_path(name)
                return pconvert(name)
            dirname, basename = os.path.split(name)
            rel.append(basename)
            if dirname == name:
                break
            name = dirname

        raise Abort('%s not under root' % myname)

def matcher(canonroot, cwd='', names=[], inc=[], exc=[], src=None):
    return _matcher(canonroot, cwd, names, inc, exc, 'glob', src)

def cmdmatcher(canonroot, cwd='', names=[], inc=[], exc=[], src=None,
               globbed=False, default=None):
    default = default or 'relpath'
    if default == 'relpath' and not globbed:
        names = expand_glob(names)
    return _matcher(canonroot, cwd, names, inc, exc, default, src)

def _matcher(canonroot, cwd, names, inc, exc, dflt_pat, src):
    """build a function to match a set of file patterns

    arguments:
    canonroot - the canonical root of the tree you're matching against
    cwd - the current working directory, if relevant
    names - patterns to find
    inc - patterns to include
    exc - patterns to exclude
    dflt_pat - if a pattern in names has no explicit type, assume this one
    src - where these patterns came from (e.g. .hgignore)

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
        return [], always, False

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
            return globre(name, '(?:|.*/)', tail)
        elif kind == 'relpath':
            return re.escape(name) + '(?:/|$)'
        elif kind == 'relre':
            if name.startswith('^'):
                return name
            return '.*' + name
        return globre(name, '', tail)

    def matchfn(pats, tail):
        """build a matching function from a set of patterns"""
        if not pats:
            return
        try:
            pat = '(?:%s)' % '|'.join([regex(k, p, tail) for (k, p) in pats])
            return re.compile(pat).match
        except re.error:
            for k, p in pats:
                try:
                    re.compile('(?:%s)' % regex(k, p, tail))
                except re.error:
                    if src:
                        raise Abort("%s: invalid pattern (%s): %s" %
                                    (src, k, p))
                    else:
                        raise Abort("invalid pattern (%s): %s" % (k, p))
            raise Abort("invalid pattern")

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
        for kind, name in [patkind(p, default) for p in names]:
            if kind in ('glob', 'relpath'):
                name = canonpath(canonroot, cwd, name)
            elif kind in ('relglob', 'path'):
                name = normpath(name)

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

    patmatch = matchfn(pats, '$') or always
    incmatch = always
    if inc:
        dummy, inckinds, dummy = normalizepats(inc, 'glob')
        incmatch = matchfn(inckinds, '(?:/|$)')
    excmatch = lambda fn: False
    if exc:
        dummy, exckinds, dummy = normalizepats(exc, 'glob')
        excmatch = matchfn(exckinds, '(?:/|$)')

    if not names and inc and not exc:
        # common case: hgignore patterns
        match = incmatch
    else:
        match = lambda fn: incmatch(fn) and not excmatch(fn) and patmatch(fn)

    return (roots, match, (inc or exc or anypats) and True)

_hgexecutable = 'hg'

def set_hgexecutable(path):
    """remember location of the 'hg' executable if easily possible

    path might be None or empty if hg was loaded as a module,
    fall back to 'hg' in this case.
    """
    global _hgexecutable
    if path:
        _hgexecutable = os.path.abspath(path)

def system(cmd, environ={}, cwd=None, onerr=None, errprefix=None):
    '''enhanced shell command execution.
    run with environment maybe modified, maybe in different dir.

    if command fails and onerr is None, return status.  if ui object,
    print error message and return status, else raise onerr object as
    exception.'''
    def py2shell(val):
        'convert python object into string that is useful to shell'
        if val in (None, False):
            return '0'
        if val == True:
            return '1'
        return str(val)
    oldenv = {}
    for k in environ:
        oldenv[k] = os.environ.get(k)
    if cwd is not None:
        oldcwd = os.getcwd()
    origcmd = cmd
    if os.name == 'nt':
        cmd = '"%s"' % cmd
    try:
        for k, v in environ.iteritems():
            os.environ[k] = py2shell(v)
        if 'HG' not in os.environ:
            os.environ['HG'] = _hgexecutable
        if cwd is not None and oldcwd != cwd:
            os.chdir(cwd)
        rc = os.system(cmd)
        if sys.platform == 'OpenVMS' and rc & 1:
            rc = 0
        if rc and onerr:
            errmsg = '%s %s' % (os.path.basename(origcmd.split(None, 1)[0]),
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

# os.path.lexists is not available on python2.3
def lexists(filename):
    "test whether a file with this name exists. does not follow symlinks"
    try:
        os.lstat(filename)
    except:
        return False
    return True

def rename(src, dst):
    """forcibly rename a file"""
    try:
        os.rename(src, dst)
    except OSError, err: # FIXME: check err (EEXIST ?)
        # on windows, rename to existing file is not allowed, so we
        # must delete destination first. but if file is open, unlink
        # schedules it for delete but does not delete it. rename
        # happens immediately even for open files, so we create
        # temporary file, delete it, rename destination to that name,
        # then delete that. then rename is safe to do.
        fd, temp = tempfile.mkstemp(dir=os.path.dirname(dst) or '.')
        os.close(fd)
        os.unlink(temp)
        os.rename(dst, temp)
        os.unlink(temp)
        os.rename(src, dst)

def unlink(f):
    """unlink and remove the directory if it is empty"""
    os.unlink(f)
    # try removing directories that might now be empty
    try:
        os.removedirs(os.path.dirname(f))
    except OSError:
        pass

def copyfile(src, dest):
    "copy a file, preserving mode"
    if os.path.islink(src):
        try:
            os.unlink(dest)
        except:
            pass
        os.symlink(os.readlink(src), dest)
    else:
        try:
            shutil.copyfile(src, dest)
            shutil.copymode(src, dest)
        except shutil.Error, inst:
            raise Abort(str(inst))

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
            except (IOError, OSError):
                hardlink = False
                shutil.copy(src, dst)
        else:
            shutil.copy(src, dst)

def audit_path(path):
    """Abort if path contains dangerous components"""
    parts = os.path.normcase(path).split(os.sep)
    if (os.path.splitdrive(path)[0] or parts[0] in ('.hg', '')
        or os.pardir in parts):
        raise Abort(_("path contains illegal component: %s") % path)

def _makelock_file(info, pathname):
    ld = os.open(pathname, os.O_CREAT | os.O_WRONLY | os.O_EXCL)
    os.write(ld, info)
    os.close(ld)

def _readlock_file(pathname):
    return posixfile(pathname).read()

def nlinks(pathname):
    """Return number of hardlinks for the given file."""
    return os.lstat(pathname).st_nlink

if hasattr(os, 'link'):
    os_link = os.link
else:
    def os_link(src, dst):
        raise OSError(0, _("Hardlinks not supported"))

def fstat(fp):
    '''stat file object that may not have fileno method.'''
    try:
        return os.fstat(fp.fileno())
    except AttributeError:
        return os.stat(fp.name)

posixfile = file

def is_win_9x():
    '''return true if run on windows 95, 98 or me.'''
    try:
        return sys.getwindowsversion()[3] == 1
    except AttributeError:
        return os.name == 'nt' and 'command' in os.environ.get('comspec', '')

getuser_fallback = None

def getuser():
    '''return name of current user'''
    try:
        return getpass.getuser()
    except ImportError:
        # import of pwd will fail on windows - try fallback
        if getuser_fallback:
            return getuser_fallback()
    # raised if win32api not available
    raise Abort(_('user name not available - set USERNAME '
                  'environment variable'))

def username(uid=None):
    """Return the name of the user with the given uid.

    If uid is None, return the name of the current user."""
    try:
        import pwd
        if uid is None:
            uid = os.getuid()
        try:
            return pwd.getpwuid(uid)[0]
        except KeyError:
            return str(uid)
    except ImportError:
        return None

def groupname(gid=None):
    """Return the name of the group with the given gid.

    If gid is None, return the name of the current group."""
    try:
        import grp
        if gid is None:
            gid = os.getgid()
        try:
            return grp.getgrgid(gid)[0]
        except KeyError:
            return str(gid)
    except ImportError:
        return None

# File system features

def checkfolding(path):
    """
    Check whether the given path is on a case-sensitive filesystem

    Requires a path (like /foo/.hg) ending with a foldable final
    directory component.
    """
    s1 = os.stat(path)
    d, b = os.path.split(path)
    p2 = os.path.join(d, b.upper())
    if path == p2:
        p2 = os.path.join(d, b.lower())
    try:
        s2 = os.stat(p2)
        if s2 == s1:
            return False
        return True
    except:
        return True

def checkexec(path):
    """
    Check whether the given path is on a filesystem with UNIX-like exec flags

    Requires a directory (like /foo/.hg)
    """
    fh, fn = tempfile.mkstemp("", "", path)
    os.close(fh)
    m = os.stat(fn).st_mode
    os.chmod(fn, m ^ 0111)
    r = (os.stat(fn).st_mode != m)
    os.unlink(fn)
    return r

def execfunc(path, fallback):
    '''return an is_exec() function with default to fallback'''
    if checkexec(path):
        return lambda x: is_exec(os.path.join(path, x))
    return fallback

def checklink(path):
    """check whether the given path is on a symlink-capable filesystem"""
    # mktemp is not racy because symlink creation will fail if the
    # file already exists
    name = tempfile.mktemp(dir=path)
    try:
        os.symlink(".", name)
        os.unlink(name)
        return True
    except (OSError, AttributeError):
        return False

def linkfunc(path, fallback):
    '''return an is_link() function with default to fallback'''
    if checklink(path):
        return lambda x: os.path.islink(os.path.join(path, x))
    return fallback

_umask = os.umask(0)
os.umask(_umask)

def needbinarypatch():
    """return True if patches should be applied in binary mode by default."""
    return os.name == 'nt'

# Platform specific variants
if os.name == 'nt':
    import msvcrt
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

        def flush(self):
            try:
                return self.fp.flush()
            except IOError, inst:
                if inst.errno != errno.EINVAL: raise
                self.close()
                raise IOError(errno.EPIPE, 'Broken pipe')

    sys.stdout = winstdout(sys.stdout)

    def system_rcpath():
        try:
            return system_rcpath_win32()
        except:
            return [r'c:\mercurial\mercurial.ini']

    def user_rcpath():
        '''return os-specific hgrc search path to the user dir'''
        try:
            userrc = user_rcpath_win32()
        except:
            userrc = os.path.join(os.path.expanduser('~'), 'mercurial.ini')
        path = [userrc]
        userprofile = os.environ.get('USERPROFILE')
        if userprofile:
            path.append(os.path.join(userprofile, 'mercurial.ini'))
        return path

    def parse_patch_output(output_line):
        """parses the output produced by patch and returns the file name"""
        pf = output_line[14:]
        if pf[0] == '`':
            pf = pf[1:-1] # Remove the quotes
        return pf

    def testpid(pid):
        '''return False if pid dead, True if running or not known'''
        return True

    def set_exec(f, mode):
        pass

    def set_link(f, mode):
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

    def samestat(s1, s2):
        return False

    # A sequence of backslashes is special iff it precedes a double quote:
    # - if there's an even number of backslashes, the double quote is not
    #   quoted (i.e. it ends the quoted region)
    # - if there's an odd number of backslashes, the double quote is quoted
    # - in both cases, every pair of backslashes is unquoted into a single
    #   backslash
    # (See http://msdn2.microsoft.com/en-us/library/a1y7w461.aspx )
    # So, to quote a string, we must surround it in double quotes, double
    # the number of backslashes that preceed double quotes and add another
    # backslash before every double quote (being careful with the double
    # quote we've appended to the end)
    _quotere = None
    def shellquote(s):
        global _quotere
        if _quotere is None:
            _quotere = re.compile(r'(\\*)("|\\$)')
        return '"%s"' % _quotere.sub(r'\1\1\\\2', s)

    def explain_exit(code):
        return _("exited with status %d") % code, code

    # if you change this stub into a real check, please try to implement the
    # username and groupname functions above, too.
    def isowner(fp, st=None):
        return True

    def find_in_path(name, path, default=None):
        '''find name in search path. path can be string (will be split
        with os.pathsep), or iterable thing that returns strings.  if name
        found, return path to name. else return default. name is looked up
        using cmd.exe rules, using PATHEXT.'''
        if isinstance(path, str):
            path = path.split(os.pathsep)

        pathext = os.environ.get('PATHEXT', '.COM;.EXE;.BAT;.CMD')
        pathext = pathext.lower().split(os.pathsep)
        isexec = os.path.splitext(name)[1].lower() in pathext

        for p in path:
            p_name = os.path.join(p, name)

            if isexec and os.path.exists(p_name):
                return p_name

            for ext in pathext:
                p_name_ext = p_name + ext
                if os.path.exists(p_name_ext):
                    return p_name_ext
        return default

    def set_signal_handler():
        try:
            set_signal_handler_win32()
        except NameError:
            pass

    try:
        # override functions with win32 versions if possible
        from util_win32 import *
        if not is_win_9x():
            posixfile = posixfile_nt
    except ImportError:
        pass

else:
    nulldev = '/dev/null'

    def rcfiles(path):
        rcs = [os.path.join(path, 'hgrc')]
        rcdir = os.path.join(path, 'hgrc.d')
        try:
            rcs.extend([os.path.join(rcdir, f) for f in os.listdir(rcdir)
                        if f.endswith(".rc")])
        except OSError:
            pass
        return rcs

    def system_rcpath():
        path = []
        # old mod_python does not set sys.argv
        if len(getattr(sys, 'argv', [])) > 0:
            path.extend(rcfiles(os.path.dirname(sys.argv[0]) +
                                  '/../etc/mercurial'))
        path.extend(rcfiles('/etc/mercurial'))
        return path

    def user_rcpath():
        return [os.path.expanduser('~/.hgrc')]

    def parse_patch_output(output_line):
        """parses the output produced by patch and returns the file name"""
        pf = output_line[14:]
        if os.sys.platform == 'OpenVMS':
            if pf[0] == '`':
                pf = pf[1:-1] # Remove the quotes
        else:
           if pf.startswith("'") and pf.endswith("'") and " " in pf:
                pf = pf[1:-1] # Remove the quotes
        return pf

    def is_exec(f):
        """check whether a file is executable"""
        return (os.lstat(f).st_mode & 0100 != 0)

    def set_exec(f, mode):
        s = os.lstat(f).st_mode
        if (s & 0100 != 0) == mode:
            return
        if mode:
            # Turn on +x for every +r bit when making a file executable
            # and obey umask.
            os.chmod(f, s | (s & 0444) >> 2 & ~_umask)
        else:
            os.chmod(f, s & 0666)

    def set_link(f, mode):
        """make a file a symbolic link/regular file

        if a file is changed to a link, its contents become the link data
        if a link is changed to a file, its link data become its contents
        """

        m = os.path.islink(f)
        if m == bool(mode):
            return

        if mode: # switch file to link
            data = file(f).read()
            os.unlink(f)
            os.symlink(data, f)
        else:
            data = os.readlink(f)
            os.unlink(f)
            file(f, "w").write(data)

    def set_binary(fd):
        pass

    def pconvert(path):
        return path

    def localpath(path):
        return path

    normpath = os.path.normpath
    samestat = os.path.samestat

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
            if why.errno in (errno.EINVAL, errno.ENOSYS):
                return _readlock_file(pathname)
            else:
                raise

    def shellquote(s):
        if os.sys.platform == 'OpenVMS':
            return '"%s"' % s
        else:
            return "'%s'" % s.replace("'", "'\\''")

    def testpid(pid):
        '''return False if pid dead, True if running or not sure'''
        if os.sys.platform == 'OpenVMS':
            return True
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

    def isowner(fp, st=None):
        """Return True if the file object f belongs to the current user.

        The return value of a util.fstat(f) may be passed as the st argument.
        """
        if st is None:
            st = fstat(fp)
        return st.st_uid == os.getuid()

    def find_in_path(name, path, default=None):
        '''find name in search path. path can be string (will be split
        with os.pathsep), or iterable thing that returns strings.  if name
        found, return path to name. else return default.'''
        if isinstance(path, str):
            path = path.split(os.pathsep)
        for p in path:
            p_name = os.path.join(p, name)
            if os.path.exists(p_name):
                return p_name
        return default

    def set_signal_handler():
        pass

def find_exe(name, default=None):
    '''find path of an executable.
    if name contains a path component, return it as is.  otherwise,
    use normal executable search path.'''

    if os.sep in name or sys.platform == 'OpenVMS':
        # don't check the executable bit.  if the file isn't
        # executable, whoever tries to actually run it will give a
        # much more useful error message.
        return name
    return find_in_path(name, os.environ.get('PATH', ''), default=default)

def _buildencodefun():
    e = '_'
    win_reserved = [ord(x) for x in '\\:*?"<>|']
    cmap = dict([ (chr(x), chr(x)) for x in xrange(127) ])
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
                    yield dmap[s[i:i+l]]
                    i += l
                    break
                except KeyError:
                    pass
            else:
                raise KeyError
    return (lambda s: "".join([cmap[c] for c in s]),
            lambda s: "".join(list(decode(s))))

encodefilename, decodefilename = _buildencodefun()

def encodedopener(openerfn, fn):
    def o(path, *args, **kw):
        return openerfn(fn(path), *args, **kw)
    return o

def mktempcopy(name, emptyok=False):
    """Create a temporary file with the same contents from name

    The permission bits are copied from the original file.

    If the temporary file is going to be truncated immediately, you
    can use emptyok=True as an optimization.

    Returns the name of the temporary file.
    """
    d, fn = os.path.split(name)
    fd, temp = tempfile.mkstemp(prefix='.%s-' % fn, dir=d)
    os.close(fd)
    # Temporary files are created with mode 0600, which is usually not
    # what we want.  If the original file already exists, just copy
    # its mode.  Otherwise, manually obey umask.
    try:
        st_mode = os.lstat(name).st_mode
    except OSError, inst:
        if inst.errno != errno.ENOENT:
            raise
        st_mode = 0666 & ~_umask
    os.chmod(temp, st_mode)
    if emptyok:
        return temp
    try:
        try:
            ifp = posixfile(name, "rb")
        except IOError, inst:
            if inst.errno == errno.ENOENT:
                return temp
            if not getattr(inst, 'filename', None):
                inst.filename = name
            raise
        ofp = posixfile(temp, "wb")
        for chunk in filechunkiter(ifp):
            ofp.write(chunk)
        ifp.close()
        ofp.close()
    except:
        try: os.unlink(temp)
        except: pass
        raise
    return temp

class atomictempfile(posixfile):
    """file-like object that atomically updates a file

    All writes will be redirected to a temporary copy of the original
    file.  When rename is called, the copy is renamed to the original
    name, making the changes visible.
    """
    def __init__(self, name, mode):
        self.__name = name
        self.temp = mktempcopy(name, emptyok=('w' in mode))
        posixfile.__init__(self, self.temp, mode)

    def rename(self):
        if not self.closed:
            posixfile.close(self)
            rename(self.temp, localpath(self.__name))

    def __del__(self):
        if not self.closed:
            try:
                os.unlink(self.temp)
            except: pass
            posixfile.close(self)

class opener(object):
    """Open files relative to a base directory

    This class is used to hide the details of COW semantics and
    remote file access from higher level code.
    """
    def __init__(self, base, audit=True):
        self.base = base
        self.audit = audit

    def __getattr__(self, name):
        if name == '_can_symlink':
            self._can_symlink = checklink(self.base)
            return self._can_symlink
        raise AttributeError(name)

    def __call__(self, path, mode="r", text=False, atomictemp=False):
        if self.audit:
            audit_path(path)
        f = os.path.join(self.base, path)

        if not text and "b" not in mode:
            mode += "b" # for that other OS

        if mode[0] != "r":
            try:
                nlink = nlinks(f)
            except OSError:
                nlink = 0
                d = os.path.dirname(f)
                if not os.path.isdir(d):
                    os.makedirs(d)
            if atomictemp:
                return atomictempfile(f, mode)
            if nlink > 1:
                rename(mktempcopy(f), f)
        return posixfile(f, mode)

    def symlink(self, src, dst):
        if self.audit:
            audit_path(dst)
        linkname = os.path.join(self.base, dst)
        try:
            os.unlink(linkname)
        except OSError:
            pass

        dirname = os.path.dirname(linkname)
        if not os.path.exists(dirname):
            os.makedirs(dirname)

        if self._can_symlink:
            try:
                os.symlink(src, linkname)
            except OSError, err:
                raise OSError(err.errno, _('could not symlink to %r: %s') %
                              (src, err.strerror), linkname)
        else:
            f = self(self, dst, "w")
            f.write(src)
            f.close()

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

def filechunkiter(f, size=65536, limit=None):
    """Create a generator that produces the data in the file size
    (default 65536) bytes at a time, up to optional limit (default is
    to read all data).  Chunks may be less than size bytes if the
    chunk is the last chunk in the file, or the file is a socket or
    some other type of file that sometimes reads less data than is
    requested."""
    assert size >= 0
    assert limit is None or limit >= 0
    while True:
        if limit is None: nbytes = size
        else: nbytes = min(limit, size)
        s = nbytes and f.read(nbytes)
        if not s: break
        if limit: limit -= len(s)
        yield s

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

def strdate(string, format, defaults):
    """parse a localized time string and return a (unixtime, offset) tuple.
    if the string cannot be parsed, ValueError is raised."""
    def timezone(string):
        tz = string.split()[-1]
        if tz[0] in "+-" and len(tz) == 5 and tz[1:].isdigit():
            tz = int(tz)
            offset = - 3600 * (tz / 100) - 60 * (tz % 100)
            return offset
        if tz == "GMT" or tz == "UTC":
            return 0
        return None

    # NOTE: unixtime = localunixtime + offset
    offset, date = timezone(string), string
    if offset != None:
        date = " ".join(string.split()[:-1])

    # add missing elements from defaults
    for part in defaults:
        found = [True for p in part if ("%"+p) in format]
        if not found:
            date += "@" + defaults[part]
            format += "@%" + part[0]

    timetuple = time.strptime(date, format)
    localunixtime = int(calendar.timegm(timetuple))
    if offset is None:
        # local timezone
        unixtime = int(time.mktime(timetuple))
        offset = unixtime - localunixtime
    else:
        unixtime = localunixtime + offset
    return unixtime, offset

def parsedate(string, formats=None, defaults=None):
    """parse a localized time string and return a (unixtime, offset) tuple.
    The date may be a "unixtime offset" string or in one of the specified
    formats."""
    if not string:
        return 0, 0
    if not formats:
        formats = defaultdateformats
    string = string.strip()
    try:
        when, offset = map(int, string.split(' '))
    except ValueError:
        # fill out defaults
        if not defaults:
            defaults = {}
        now = makedate()
        for part in "d mb yY HI M S".split():
            if part not in defaults:
                if part[0] in "HMS":
                    defaults[part] = "00"
                elif part[0] in "dm":
                    defaults[part] = "1"
                else:
                    defaults[part] = datestr(now, "%" + part[0], False)

        for format in formats:
            try:
                when, offset = strdate(string, format, defaults)
            except ValueError:
                pass
            else:
                break
        else:
            raise Abort(_('invalid date: %r ') % string)
    # validate explicit (probably user-specified) date and
    # time zone offset. values must fit in signed 32 bits for
    # current 32-bit linux runtimes. timezones go from UTC-12
    # to UTC+14
    if abs(when) > 0x7fffffff:
        raise Abort(_('date exceeds 32 bits: %d') % when)
    if offset < -50400 or offset > 43200:
        raise Abort(_('impossible time zone offset: %d') % offset)
    return when, offset

def matchdate(date):
    """Return a function that matches a given date match specifier

    Formats include:

    '{date}' match a given date to the accuracy provided

    '<{date}' on or before a given date

    '>{date}' on or after a given date

    """

    def lower(date):
        return parsedate(date, extendeddateformats)[0]

    def upper(date):
        d = dict(mb="12", HI="23", M="59", S="59")
        for days in "31 30 29".split():
            try:
                d["d"] = days
                return parsedate(date, extendeddateformats, d)[0]
            except:
                pass
        d["d"] = "28"
        return parsedate(date, extendeddateformats, d)[0]

    if date[0] == "<":
        when = upper(date[1:])
        return lambda x: x <= when
    elif date[0] == ">":
        when = lower(date[1:])
        return lambda x: x >= when
    elif date[0] == "-":
        try:
            days = int(date[1:])
        except ValueError:
            raise Abort(_("invalid day spec: %s") % date[1:])
        when = makedate()[0] - days * 3600 * 24
        return lambda x: x >= when
    elif " to " in date:
        a, b = date.split(" to ")
        start, stop = lower(a), upper(b)
        return lambda x: x >= start and x <= stop
    else:
        start, stop = lower(date), upper(date)
        return lambda x: x >= start and x <= stop

def shortuser(user):
    """Return a short representation of a user name or email address."""
    f = user.find('@')
    if f >= 0:
        user = user[:f]
    f = user.find('<')
    if f >= 0:
        user = user[f+1:]
    f = user.find(' ')
    if f >= 0:
        user = user[:f]
    f = user.find('.')
    if f >= 0:
        user = user[:f]
    return user

def ellipsis(text, maxlength=400):
    """Trim string to at most maxlength (default: 400) characters."""
    if len(text) <= maxlength:
        return text
    else:
        return "%s..." % (text[:maxlength-3])

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

def os_rcpath():
    '''return default os-specific hgrc search path'''
    path = system_rcpath()
    path.extend(user_rcpath())
    path = [os.path.normpath(f) for f in path]
    return path

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

def bytecount(nbytes):
    '''return byte count formatted as readable string, with units'''

    units = (
        (100, 1<<30, _('%.0f GB')),
        (10, 1<<30, _('%.1f GB')),
        (1, 1<<30, _('%.2f GB')),
        (100, 1<<20, _('%.0f MB')),
        (10, 1<<20, _('%.1f MB')),
        (1, 1<<20, _('%.2f MB')),
        (100, 1<<10, _('%.0f KB')),
        (10, 1<<10, _('%.1f KB')),
        (1, 1<<10, _('%.2f KB')),
        (1, 1, _('%.0f bytes')),
        )

    for multiplier, divisor, format in units:
        if nbytes >= divisor * multiplier:
            return format % (nbytes / float(divisor))
    return units[-1][2] % nbytes

def drop_scheme(scheme, path):
    sc = scheme + ':'
    if path.startswith(sc):
        path = path[len(sc):]
        if path.startswith('//'):
            path = path[2:]
    return path
