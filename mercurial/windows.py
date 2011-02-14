# windows.py - Windows utility function implementations for Mercurial
#
#  Copyright 2005-2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import osutil, error
import errno, msvcrt, os, re, sys, random, subprocess

nulldev = 'NUL:'
umask = 002

# wrap osutil.posixfile to provide friendlier exceptions
def posixfile(name, mode='r', buffering=-1):
    try:
        return osutil.posixfile(name, mode, buffering)
    except WindowsError, err:
        raise IOError(err.errno, '%s: %s' % (name, err.strerror))
posixfile.__doc__ = osutil.posixfile.__doc__

class winstdout(object):
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
            # This is workaround for "Not enough space" error on
            # writing large size of data to console.
            limit = 16000
            l = len(s)
            start = 0
            self.softspace = 0
            while start < l:
                end = start + limit
                self.fp.write(s[start:end])
                start = end
        except IOError, inst:
            if inst.errno != 0:
                raise
            self.close()
            raise IOError(errno.EPIPE, 'Broken pipe')

    def flush(self):
        try:
            return self.fp.flush()
        except IOError, inst:
            if inst.errno != errno.EINVAL:
                raise
            self.close()
            raise IOError(errno.EPIPE, 'Broken pipe')

sys.stdout = winstdout(sys.stdout)

def _is_win_9x():
    '''return true if run on windows 95, 98 or me.'''
    try:
        return sys.getwindowsversion()[3] == 1
    except AttributeError:
        return 'command' in os.environ.get('comspec', '')

def openhardlinks():
    return not _is_win_9x()

_HKEY_LOCAL_MACHINE = 0x80000002L

def system_rcpath():
    '''return default os-specific hgrc search path'''
    rcpath = []
    filename = executable_path()
    # Use mercurial.ini found in directory with hg.exe
    progrc = os.path.join(os.path.dirname(filename), 'mercurial.ini')
    if os.path.isfile(progrc):
        rcpath.append(progrc)
        return rcpath
    # Use hgrc.d found in directory with hg.exe
    progrcd = os.path.join(os.path.dirname(filename), 'hgrc.d')
    if os.path.isdir(progrcd):
        for f, kind in osutil.listdir(progrcd):
            if f.endswith('.rc'):
                rcpath.append(os.path.join(progrcd, f))
        return rcpath
    # else look for a system rcpath in the registry
    value = lookup_reg('SOFTWARE\\Mercurial', None, _HKEY_LOCAL_MACHINE)
    if not isinstance(value, str) or not value:
        return rcpath
    value = value.replace('/', os.sep)
    for p in value.split(os.pathsep):
        if p.lower().endswith('mercurial.ini'):
            rcpath.append(p)
        elif os.path.isdir(p):
            for f, kind in osutil.listdir(p):
                if f.endswith('.rc'):
                    rcpath.append(os.path.join(p, f))
    return rcpath

def user_rcpath():
    '''return os-specific hgrc search path to the user dir'''
    home = os.path.expanduser('~')
    path = [os.path.join(home, 'mercurial.ini'),
            os.path.join(home, '.hgrc')]
    userprofile = os.environ.get('USERPROFILE')
    if userprofile:
        path.append(os.path.join(userprofile, 'mercurial.ini'))
        path.append(os.path.join(userprofile, '.hgrc'))
    return path

def parse_patch_output(output_line):
    """parses the output produced by patch and returns the filename"""
    pf = output_line[14:]
    if pf[0] == '`':
        pf = pf[1:-1] # Remove the quotes
    return pf

def sshargs(sshcmd, host, user, port):
    '''Build argument list for ssh or Plink'''
    pflag = 'plink' in sshcmd.lower() and '-P' or '-p'
    args = user and ("%s@%s" % (user, host)) or host
    return port and ("%s %s %s" % (args, pflag, port)) or args

def set_flags(f, l, x):
    pass

def set_binary(fd):
    # When run without console, pipes may expose invalid
    # fileno(), usually set to -1.
    if hasattr(fd, 'fileno') and fd.fileno() >= 0:
        msvcrt.setmode(fd.fileno(), os.O_BINARY)

def pconvert(path):
    return '/'.join(path.split(os.sep))

def localpath(path):
    return path.replace('/', '\\')

def normpath(path):
    return pconvert(os.path.normpath(path))

def realpath(path):
    '''
    Returns the true, canonical file system path equivalent to the given
    path.
    '''
    # TODO: There may be a more clever way to do this that also handles other,
    # less common file systems.
    return os.path.normpath(os.path.normcase(os.path.realpath(path)))

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

def quotecommand(cmd):
    """Build a command string suitable for os.popen* calls."""
    if sys.version_info < (2, 7, 1):
        # Python versions since 2.7.1 do this extra quoting themselves
        return '"' + cmd + '"'
    return cmd

def popen(command, mode='r'):
    # Work around "popen spawned process may not write to stdout
    # under windows"
    # http://bugs.python.org/issue1366
    command += " 2> %s" % nulldev
    return os.popen(quotecommand(command), mode)

def explain_exit(code):
    return _("exited with status %d") % code, code

# if you change this stub into a real check, please try to implement the
# username and groupname functions above, too.
def isowner(st):
    return True

def find_exe(command):
    '''Find executable for command searching like cmd.exe does.
    If command is a basename then PATH is searched for command.
    PATH isn't searched if command is an absolute or relative path.
    An extension from PATHEXT is found and added if not present.
    If command isn't found None is returned.'''
    pathext = os.environ.get('PATHEXT', '.COM;.EXE;.BAT;.CMD')
    pathexts = [ext for ext in pathext.lower().split(os.pathsep)]
    if os.path.splitext(command)[1].lower() in pathexts:
        pathexts = ['']

    def findexisting(pathcommand):
        'Will append extension (if needed) and return existing file'
        for ext in pathexts:
            executable = pathcommand + ext
            if os.path.exists(executable):
                return executable
        return None

    if os.sep in command:
        return findexisting(command)

    for path in os.environ.get('PATH', '').split(os.pathsep):
        executable = findexisting(os.path.join(path, command))
        if executable is not None:
            return executable
    return findexisting(os.path.expanduser(os.path.expandvars(command)))

def statfiles(files):
    '''Stat each file in files and yield stat or None if file does not exist.
    Cluster and cache stat per directory to minimize number of OS stat calls.'''
    ncase = os.path.normcase
    dircache = {} # dirname -> filename -> status | None if file does not exist
    for nf in files:
        nf  = ncase(nf)
        dir, base = os.path.split(nf)
        if not dir:
            dir = '.'
        cache = dircache.get(dir, None)
        if cache is None:
            try:
                dmap = dict([(ncase(n), s)
                    for n, k, s in osutil.listdir(dir, True)])
            except OSError, err:
                # handle directory not found in Python version prior to 2.5
                # Python <= 2.4 returns native Windows code 3 in errno
                # Python >= 2.5 returns ENOENT and adds winerror field
                # EINVAL is raised if dir is not a directory.
                if err.errno not in (3, errno.ENOENT, errno.EINVAL,
                                     errno.ENOTDIR):
                    raise
                dmap = {}
            cache = dircache.setdefault(dir, dmap)
        yield cache.get(base, None)

def username(uid=None):
    """Return the name of the user with the given uid.

    If uid is None, return the name of the current user."""
    return None

def groupname(gid=None):
    """Return the name of the group with the given gid.

    If gid is None, return the name of the current group."""
    return None

def _removedirs(name):
    """special version of os.removedirs that does not remove symlinked
    directories or junction points if they actually contain files"""
    if osutil.listdir(name):
        return
    os.rmdir(name)
    head, tail = os.path.split(name)
    if not tail:
        head, tail = os.path.split(head)
    while head and tail:
        try:
            if osutil.listdir(head):
                return
            os.rmdir(head)
        except:
            break
        head, tail = os.path.split(head)

def unlinkpath(f):
    """unlink and remove the directory if it is empty"""
    os.unlink(f)
    # try removing directories that might now be empty
    try:
        _removedirs(os.path.dirname(f))
    except OSError:
        pass

def unlink(f):
    '''try to implement POSIX' unlink semantics on Windows'''

    # POSIX allows to unlink and rename open files. Windows has serious
    # problems with doing that:
    # - Calling os.unlink (or os.rename) on a file f fails if f or any
    #   hardlinked copy of f has been opened with Python's open(). There is no
    #   way such a file can be deleted or renamed on Windows (other than
    #   scheduling the delete or rename for the next reboot).
    # - Calling os.unlink on a file that has been opened with Mercurial's
    #   posixfile (or comparable methods) will delay the actual deletion of
    #   the file for as long as the file is held open. The filename is blocked
    #   during that time and cannot be used for recreating a new file under
    #   that same name ("zombie file"). Directories containing such zombie files
    #   cannot be removed or moved.
    # A file that has been opened with posixfile can be renamed, so we rename
    # f to a random temporary name before calling os.unlink on it. This allows
    # callers to recreate f immediately while having other readers do their
    # implicit zombie filename blocking on a temporary name.

    for tries in xrange(10):
        temp = '%s-%08x' % (f, random.randint(0, 0xffffffff))
        try:
            os.rename(f, temp)  # raises OSError EEXIST if temp exists
            break
        except OSError, e:
            if e.errno != errno.EEXIST:
                raise
    else:
        raise IOError, (errno.EEXIST, "No usable temporary filename found")

    try:
        os.unlink(temp)
    except:
        # Some very rude AV-scanners on Windows may cause this unlink to fail.
        # Not aborting here just leaks the temp file, whereas aborting at this
        # point may leave serious inconsistencies. Ideally, we would notify
        # the user in this case here.
        pass

def rename(src, dst):
    '''atomically rename file src to dst, replacing dst if it exists'''
    try:
        os.rename(src, dst)
    except OSError, e:
        if e.errno != errno.EEXIST:
            raise
        unlink(dst)
        os.rename(src, dst)

def gethgcmd():
    return [sys.executable] + sys.argv[:1]

def termwidth():
    # cmd.exe does not handle CR like a unix console, the CR is
    # counted in the line length. On 80 columns consoles, if 80
    # characters are written, the following CR won't apply on the
    # current line but on the new one. Keep room for it.
    return 79

def groupmembers(name):
    # Don't support groups on Windows for now
    raise KeyError()

from win32 import *

expandglobs = True
