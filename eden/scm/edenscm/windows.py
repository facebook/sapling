# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# windows.py - Windows utility function implementations for Mercurial
#
#  Copyright 2005-2009 Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import io
import msvcrt
import os
import re
import stat
import sys
import tempfile
import time
from typing import IO, Optional

import bindings

from edenscmnative import osutil

from . import encoding, error, pycompat, win32, winutil
from .i18n import _


try:
    # pyre-fixme[21]: Could not find `_winreg`.
    import _winreg as winreg

    winreg.CloseKey
except ImportError:
    import winreg


getfstype = bindings.fs.fstype

executablepath = win32.executablepath
getmaxrss = win32.getmaxmemoryusage
getuser = win32.getuser
hidewindow = win32.hidewindow
makedir = win32.makedir
nlinks = win32.nlinks
oslink = win32.oslink
samedevice = win32.samedevice
samefile = win32.samefile
setsignalhandler = win32.setsignalhandler
split = os.path.split
testpid = win32.testpid
unlink = win32.unlink
checkosfilename = winutil.checkwinfilename

umask = 0o022


class mixedfilemodewrapper(object):
    """Wraps a file handle when it is opened in read/write mode.

    fopen() and fdopen() on Windows have a specific-to-Windows requirement
    that files opened with mode r+, w+, or a+ make a call to a file positioning
    function when switching between reads and writes. Without this extra call,
    Python will raise a not very intuitive "IOError: [Errno 0] Error."

    This class wraps posixfile instances when the file is opened in read/write
    mode and automatically adds checks or inserts appropriate file positioning
    calls when necessary.
    """

    OPNONE = 0
    OPREAD = 1
    OPWRITE = 2

    def __init__(self, fp):
        object.__setattr__(self, r"_fp", fp)
        object.__setattr__(self, r"_lastop", 0)

    def __enter__(self):
        return self._fp.__enter__()

    def __exit__(self, exc_type, exc_val, exc_tb):
        self._fp.__exit__(exc_type, exc_val, exc_tb)

    def __getattr__(self, name):
        return getattr(self._fp, name)

    def __setattr__(self, name, value):
        return self._fp.__setattr__(name, value)

    def _noopseek(self):
        self._fp.seek(0, os.SEEK_CUR)

    def seek(self, *args, **kwargs):
        object.__setattr__(self, r"_lastop", self.OPNONE)
        return self._fp.seek(*args, **kwargs)

    def write(self, d):
        if self._lastop == self.OPREAD:
            self._noopseek()

        object.__setattr__(self, r"_lastop", self.OPWRITE)
        return self._fp.write(d)

    def writelines(self, *args, **kwargs):
        if self._lastop == self.OPREAD:
            self._noopeseek()

        object.__setattr__(self, r"_lastop", self.OPWRITE)
        return self._fp.writelines(*args, **kwargs)

    def read(self, *args, **kwargs):
        if self._lastop == self.OPWRITE:
            self._noopseek()

        object.__setattr__(self, r"_lastop", self.OPREAD)
        return self._fp.read(*args, **kwargs)

    def readline(self, *args, **kwargs):
        if self._lastop == self.OPWRITE:
            self._noopseek()

        object.__setattr__(self, r"_lastop", self.OPREAD)
        return self._fp.readline(*args, **kwargs)

    def readlines(self, *args, **kwargs):
        if self._lastop == self.OPWRITE:
            self._noopseek()

        object.__setattr__(self, r"_lastop", self.OPREAD)
        return self._fp.readlines(*args, **kwargs)


class fdproxy(object):
    """Wraps osutil.posixfile() to override the name attribute to reflect the
    underlying file name.
    """

    def __init__(self, name, fp):
        self.name = name
        self._fp = fp

    def __enter__(self):
        self._fp.__enter__()
        # Return this wrapper for the context manager so that the name is
        # still available.
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        self._fp.__exit__(exc_type, exc_value, traceback)

    def __iter__(self):
        return iter(self._fp)

    def __getattr__(self, name):
        return getattr(self._fp, name)


def posixfile(name: str, mode: str = "r", buffering: int = -1) -> "IO":
    """Open a file with even more POSIX-like semantics"""
    try:
        fp = osutil.posixfile(name, mode, buffering)  # may raise WindowsError

        # PyFile_FromFd() ignores the name, and seems to report fp.name as the
        # underlying file descriptor.
        if sys.version_info[0] >= 3:
            fp = fdproxy(name, fp)

        return _fixseek(fp, mode)
    # pyre-fixme[10]: Name `WindowsError` is used but not defined.
    except WindowsError as err:
        # convert to a friendlier exception
        raise IOError(err.errno, "%s: %s" % (name, encoding.strtolocal(err.strerror)))


def fdopen(fd, mode="r", bufsize=-1, **kwargs):
    fp = os.fdopen(fd, mode, bufsize, **kwargs)
    return _fixseek(fp, mode)


def _fixseek(fp, mode):
    """Fix seek related issues for files with read+write mode on Windows,
    by wrapping it in mixedfilemodewrapper.
    """
    # The position when opening in append mode is implementation defined, so
    # make it consistent with other platforms, which position at EOF.
    if "a" in mode:
        fp.seek(0, os.SEEK_END)

    if "+" in mode:
        return mixedfilemodewrapper(fp)

    return fp


# may be wrapped by win32mbcs extension
listdir = osutil.listdir


class winstdout(object):
    """stdout on windows misbehaves if sent through a pipe"""

    def __init__(self, fp):
        self.fp = fp

    def __getattr__(self, key):
        return getattr(self.fp, key)

    def close(self):
        try:
            self.fp.close()
        except IOError:
            pass

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
        except IOError as inst:
            if inst.errno != 0:
                raise
            self.close()
            raise IOError(errno.EPIPE, "Broken pipe")

    def flush(self):
        try:
            return self.fp.flush()
        except IOError as inst:
            if inst.errno != errno.EINVAL:
                raise
            raise IOError(errno.EPIPE, "Broken pipe")


def _is_win_9x():
    """return true if run on windows 95, 98 or me."""
    try:
        return sys.getwindowsversion()[3] == 1
    except AttributeError:
        return "command" in encoding.environ.get("comspec", "")


def openhardlinks():
    return not _is_win_9x()


def parsepatchoutput(output_line):
    """parses the output produced by patch and returns the filename"""
    pf = output_line[14:]
    if pf[0] == "`":
        pf = pf[1:-1]  # Remove the quotes
    return pf


def sshargs(sshcmd, host, user, port):
    """Build argument list for ssh or Plink"""
    pflag = "plink" in sshcmd.lower() and "-P" or "-p"
    args = user and ("%s@%s" % (user, host)) or host
    if args.startswith("-") or args.startswith("/"):
        raise error.Abort(
            _("illegal ssh hostname or username starting with - or /: %s") % args
        )
    args = shellquote(args)
    if port:
        args = "%s %s %s" % (pflag, shellquote(port), args)
    return args


def setflags(f: str, l: bool, x: bool) -> None:
    pass


def copymode(src, dst, mode=None):
    pass


def checkexec(path: str) -> bool:
    return False


def checklink(path: str) -> bool:
    return False


def setbinary(fd):
    # When run without console, pipes may expose invalid
    # fileno(), usually set to -1.
    fno = getattr(fd, "fileno", None)
    try:
        if fno is not None and fno() >= 0:
            msvcrt.setmode(fno(), os.O_BINARY)
    except io.UnsupportedOperation:
        # fno() might raise this exception
        pass


def pconvert(path):
    return path.replace(pycompat.ossep, "/")


def localpath(path):
    return path.replace("/", "\\")


def normpath(path):
    return pconvert(os.path.normpath(path))


def normcase(path):
    return encoding.upper(path)  # NTFS compares via upper()


# see posix.py for definitions
normcasespec = encoding.normcasespecs.upper
normcasefallback = encoding.upperfallback


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
# the number of backslashes that precede double quotes and add another
# backslash before every double quote (being careful with the double
# quote we've appended to the end)
_quotere = None
_needsshellquote = None


def shellquote(s):
    r"""
    >>> shellquote(r'C:\Users\xyz')
    '"C:\\Users\\xyz"'
    >>> shellquote(r'C:\Users\xyz/mixed')
    '"C:\\Users\\xyz/mixed"'
    >>> # Would be safe not to quote too, since it is all double backslashes
    >>> shellquote(r'C:\\Users\\xyz')
    '"C:\\\\Users\\\\xyz"'
    >>> # But this must be quoted
    >>> shellquote(r'C:\\Users\\xyz/abc')
    '"C:\\\\Users\\\\xyz/abc"'
    """
    global _quotere
    if _quotere is None:
        _quotere = re.compile(r'(\\*)("|\\$)')
    global _needsshellquote
    if _needsshellquote is None:
        # ":" is also treated as "safe character", because it is used as a part
        # of path name on Windows.  "\" is also part of a path name, but isn't
        # safe because shlex.split() (kind of) treats it as an escape char and
        # drops it.  It will leave the next character, even if it is another
        # "\".
        _needsshellquote = re.compile(r"[^a-zA-Z0-9._:/-]").search
    if s and not _needsshellquote(s) and not _quotere.search(s):
        # "s" shouldn't have to be quoted
        return s
    return '"%s"' % _quotere.sub(r"\1\1\\\2", s)


def popen(command, mode="r"):
    # Work around "popen spawned process may not write to stdout
    # under windows"
    # http://bugs.python.org/issue1366
    command += " 2> %s" % os.devnull
    return os.popen(command, mode)


def explainexit(code):
    return _("exited with status %d") % code, code


# if you change this stub into a real check, please try to implement the
# username and groupname functions above, too.
def isowner(st):
    return True


def findexe(command):
    """Find executable for command searching like cmd.exe does.
    If command is a basename then PATH is searched for command.
    PATH isn't searched if command is an absolute or relative path.
    An extension from PATHEXT is found and added if not present.
    If command isn't found None is returned."""
    pathext = encoding.environ.get("PATHEXT", ".COM;.EXE;.BAT;.CMD")
    pathexts = [ext for ext in pathext.lower().split(pycompat.ospathsep)]
    if os.path.splitext(command)[1].lower() in pathexts:
        pathexts = [""]

    def findexisting(pathcommand):
        "Will append extension (if needed) and return existing file"
        for ext in pathexts:
            executable = pathcommand + ext
            if os.path.exists(executable):
                return executable
        return None

    if pycompat.ossep in command:
        return findexisting(command)

    for path in encoding.environ.get("PATH", "").split(pycompat.ospathsep):
        executable = findexisting(os.path.join(path, command))
        if executable is not None:
            return executable
    return findexisting(os.path.expanduser(os.path.expandvars(command)))


_wantedkinds = {stat.S_IFREG, stat.S_IFLNK}


def statfiles(files):
    """Stat each file in files. Yield each stat, or None if a file
    does not exist or has a type we don't care about.

    Cluster and cache stat per directory to minimize number of OS stat calls."""
    dircache = {}  # dirname -> filename -> status | None if file does not exist
    getkind = stat.S_IFMT
    for nf in files:
        nf = normcase(nf)
        dir, base = os.path.split(nf)
        if not dir:
            dir = "."
        cache = dircache.get(dir, None)
        if cache is None:
            try:
                dmap = dict(
                    [
                        (normcase(n), s)
                        for n, k, s in listdir(dir, True)
                        if getkind(s.st_mode) in _wantedkinds
                    ]
                )
            except OSError as err:
                # Python >= 2.5 returns ENOENT and adds winerror field
                # EINVAL is raised if dir is not a directory.
                if err.errno not in (errno.ENOENT, errno.EINVAL, errno.ENOTDIR):
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


def removedirs(name: str) -> None:
    """special version of os.removedirs that does not remove symlinked
    directories or junction points if they actually contain files"""
    if listdir(name):
        return
    os.rmdir(name)
    head, tail = os.path.split(name)
    if not tail:
        head, tail = os.path.split(head)
    while head and tail:
        try:
            if listdir(head):
                return
            os.rmdir(head)
        except (ValueError, OSError):
            break
        head, tail = os.path.split(head)


def rename(src: str, dst: str) -> None:
    """Atomically rename file src to dst, replacing dst if it exists

    Note that this is only really atomic for files (not dirs) on the
    same volume"""
    try:
        win32.movefileex(src, dst)
    except OSError as e:
        if e.errno != errno.EEXIST and e.errno != errno.EACCES:
            raise
        unlink(dst)
        os.rename(src, dst)


def syncfile(fp):
    """Makes best effort attempt to make sure all contents previously written
    to the fp is persisted to a permanent storage device."""
    # See comments in posix implementation of syncdir for discussion on this
    # topic.
    try:
        fp.flush()
        os.fsync(fp.fileno())
    except (OSError, IOError):
        # do nothing since this is just best effort
        pass


def syncdir(dirpath):
    """Makes best effort attempt to make sure previously issued
    renames where target is a file immediately inside the specified
    dirpath is persisted to a permanent storage device."""
    # See comments in posix implementation for discussion on this topic.
    # Do nothing.


def groupmembers(name):
    # Don't support groups on Windows for now
    raise KeyError


def isexec(f):
    return False


class cachestat(object):
    def __init__(self, path):
        if path is None:
            self.fi = None
        else:
            try:
                self.fi = win32.getfileinfo(path)
            except OSError as ex:
                if ex.errno == errno.ENOENT:
                    self.fi = None
                else:
                    raise

    __hash__ = object.__hash__

    def __eq__(self, other):
        try:
            lhs = self.fi
            rhs = other.fi
            if lhs is None or rhs is None:
                return lhs is None and rhs is None
            return (
                lhs.dwFileAttributes == rhs.dwFileAttributes
                and lhs.ftCreationTime.dwLowDateTime == rhs.ftCreationTime.dwLowDateTime
                and lhs.ftCreationTime.dwHighDateTime
                == rhs.ftCreationTime.dwHighDateTime
                and lhs.ftLastWriteTime.dwLowDateTime
                == rhs.ftLastWriteTime.dwLowDateTime
                and lhs.ftLastWriteTime.dwHighDateTime
                == rhs.ftLastWriteTime.dwHighDateTime
                and lhs.dwVolumeSerialNumber == rhs.dwVolumeSerialNumber
                and lhs.nFileSizeHigh == rhs.nFileSizeHigh
                and lhs.nFileSizeLow == rhs.nFileSizeLow
                and lhs.nFileIndexHigh == rhs.nFileIndexHigh
                and lhs.nFileIndexLow == rhs.nFileIndexLow
            )
        except AttributeError:
            return False

    def __ne__(self, other):
        return not self == other


def lookupreg(key, valname=None, scope=None):
    """Look up a key/value name in the Windows registry.

    valname: value name. If unspecified, the default value for the key
    is used.
    scope: optionally specify scope for registry lookup, this can be
    a sequence of scopes to look up in order. Default (CURRENT_USER,
    LOCAL_MACHINE).
    """
    if scope is None:
        scope = (winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE)
    elif not isinstance(scope, (list, tuple)):
        scope = (scope,)
    for s in scope:
        try:
            val = winreg.QueryValueEx(winreg.OpenKey(s, key), valname)[0]
            # never let a Unicode string escape into the wild
            return encoding.unitolocal(val)
        except EnvironmentError:
            pass


expandglobs = True


def statislink(st):
    """check whether a stat result is a symlink"""
    return False


def statisexec(st):
    """check whether a stat result is an executable file"""
    return False


def bindunixsocket(sock, path):
    raise NotImplementedError("unsupported platform")


def _cleanuptemplockfiles(dirname: str, basename: str) -> None:
    for susp in os.listdir(dirname):
        if not susp.startswith(basename) or not susp.endswith(".tmplock"):
            continue

        # Multiple processes might be trying to take the lock at the  same
        # time, they will all create a .tmplock file, let's not remove a file
        # that was just created to let the other process continue.
        try:
            stat = os.lstat(susp)
        except OSError:
            continue

        now = time.mktime(time.gmtime())
        filetime = time.mktime(time.gmtime(stat.st_mtime))
        if now > filetime + 10:
            continue

        try:
            os.unlink(os.path.join(dirname, susp))
        except WindowsError:
            pass


# pyre-fixme[9]: checkdeadlock has type `bool`; used as `None`.
def makelock(info: str, pathname: str, checkdeadlock: bool = None) -> "Optional[int]":
    dirname = os.path.dirname(pathname)
    basename = os.path.basename(pathname)
    _cleanuptemplockfiles(dirname, basename)
    fd, tname = tempfile.mkstemp(
        suffix=".tmplock", prefix="%s.%i." % (basename, os.getpid()), dir=dirname
    )
    os.write(fd, pycompat.encodeutf8(info))
    os.fsync(fd)
    os.close(fd)
    try:
        os.rename(tname, pathname)
    except WindowsError:
        os.unlink(tname)
        raise


def readlock(pathname: str) -> str:
    try:
        return os.readlink(pathname)
    except OSError as why:
        if why.errno not in (errno.EINVAL, errno.ENOSYS):
            raise
    except AttributeError:  # no symlink in os
        pass
    fp = posixfile(pathname)
    r = fp.read()
    fp.close()
    return r


def releaselock(_lockfd: "Optional[int]", pathname: str) -> None:
    os.unlink(pathname)


def unixsocket():
    # Defer import since this isn't present in OSS build yet.
    # pyre-fixme[21]: Could not find a module corresponding to import `eden.thrift.windows_thrift`.
    from eden.thrift.windows_thrift import WindowsSocketHandle

    return WindowsSocketHandle()


# Set outputencoding to UTF-8
if not encoding.outputencoding:
    # The Rust IO requires UTF-8 output.
    encoding.outputencoding = "utf-8"
