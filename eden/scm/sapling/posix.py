# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# posix.py - Posix utility function implementations for Mercurial
#
#  Copyright 2005-2009 Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


import errno
import fcntl
import getpass
import grp
import os
import pwd
import re
import resource
import socket
import stat
import sys
import tempfile
import unicodedata

import bindings

from . import encoding, error, fscap, identity, sysutil
from .i18n import _

osutil = bindings.cext.osutil

getfstype = bindings.fs.fstype


posixfile = open
normpath = os.path.normpath
samestat = os.path.samestat
try:
    oslink = os.link
except AttributeError:
    # Some platforms build Python without os.link on systems that are
    # vaguely unix-like but don't have hardlink support. For those
    # poor souls, just say we tried and that it failed so we fall back
    # to copies.
    def oslink(src, dst):
        raise OSError(errno.EINVAL, "hardlinks not supported: %s to %s" % (src, dst))


fdopen = os.fdopen
unlink = os.unlink
rename = os.rename
removedirs = os.removedirs
expandglobs = False

O_CLOEXEC = osutil.O_CLOEXEC

umask = os.umask(0)
os.umask(umask)


def split(p):
    """Same as posixpath.split, but faster

    >>> import posixpath
    >>> for f in ['/absolute/path/to/file',
    ...           'relative/path/to/file',
    ...           'file_alone',
    ...           'path/to/directory/',
    ...           '/multiple/path//separators',
    ...           '/file_at_root',
    ...           '///multiple_leading_separators_at_root',
    ...           '']:
    ...     assert split(f) == posixpath.split(f), f
    """
    ht = p.rsplit("/", 1)
    if len(ht) == 1:
        return "", p
    nh = ht[0].rstrip("/")
    if nh:
        return nh, ht[1]
    return ht[0] + "/", ht[1]


def openhardlinks():
    """return true if it is safe to hold open file handles to hardlinks"""
    return True


def nlinks(name):
    """return number of hardlinks for the given file"""
    return os.lstat(name).st_nlink


def parsepatchoutput(output_line):
    """parses the output produced by patch and returns the filename"""
    pf = output_line[14:]
    if sys.platform == "OpenVMS":
        if pf[0] == "`":
            pf = pf[1:-1]  # Remove the quotes
    else:
        if pf.startswith("'") and pf.endswith("'") and " " in pf:
            pf = pf[1:-1]  # Remove the quotes
    return pf


def sshargs(sshcmd, host, user, port):
    """Build argument list for ssh"""
    args = user and ("%s@%s" % (user, host)) or host
    if "-" in args[:1]:
        raise error.Abort(
            _("illegal ssh hostname or username starting with -: %s") % args
        )
    args = shellquote(args)
    if port:
        args = "-p %s %s" % (shellquote(port), args)
    return args


def isexec(f):
    """check whether a file is executable"""
    return os.lstat(f).st_mode & 0o100 != 0


def setflags(f: str, l: bool, x: bool) -> None:
    st = os.lstat(f)
    s = st.st_mode
    if l:
        if not stat.S_ISLNK(s):
            # switch file to link
            fp = open(f)
            data = fp.read()
            fp.close()
            unlink(f)
            try:
                os.symlink(data, f)
            except OSError:
                # failed to make a link, rewrite file
                fp = open(f, "w")
                fp.write(data)
                fp.close()
        # no chmod needed at this point
        return
    if stat.S_ISLNK(s):
        # switch link to file
        data = os.readlink(f)
        unlink(f)
        fp = open(f, "w")
        fp.write(data)
        fp.close()
        s = 0o666 & ~umask  # avoid restatting for chmod

    sx = s & 0o100
    if st.st_nlink > 1 and bool(x) != bool(sx):
        # the file is a hardlink, break it
        with open(f, "rb") as fp:
            data = fp.read()
        unlink(f)
        with open(f, "wb") as fp:
            fp.write(data)

    if x and not sx:
        # Turn on +x for every +r bit when making a file executable
        # and obey umask.
        os.chmod(f, s | (s & 0o444) >> 2 & ~umask)
    elif not x and sx:
        # Turn off all +x bits
        os.chmod(f, s & 0o666)


def copymode(src, dst, mode=None):
    """Copy the file mode from the file at path src to dst.
    If src doesn't exist, we're using mode instead. If mode is None, we're
    using umask."""
    try:
        st_mode = os.lstat(src).st_mode & 0o777
    except OSError as inst:
        if inst.errno != errno.ENOENT:
            raise
        st_mode = mode
        if st_mode is None:
            st_mode = ~umask
        st_mode &= 0o666
    os.chmod(dst, st_mode)


def _checkexec(path: str) -> bool:
    """
    Check whether the given path is on a filesystem with UNIX-like exec flags

    Requires a directory (like /foo/.hg)
    """

    cap = fscap.getfscap(getfstype(path), fscap.EXECBIT)
    if cap is not None:
        return cap

    # VFAT on some Linux versions can flip mode but it doesn't persist
    # a FS remount. Frequently we can detect it if files are created
    # with exec bit on.

    try:
        EXECFLAGS = stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
        ident = identity.sniffdir(path) or identity.default()
        cachedir = os.path.join(path, ident.dotdir(), "cache")
        if os.path.isdir(cachedir):
            checkisexec = os.path.join(cachedir, "checkisexec")
            checknoexec = os.path.join(cachedir, "checknoexec")

            try:
                m = os.stat(checkisexec).st_mode
            except OSError as e:
                if e.errno != errno.ENOENT:
                    raise
                # checkisexec does not exist - fall through ...
            else:
                # checkisexec exists, check if it actually is exec
                if m & EXECFLAGS != 0:
                    # ensure checkisexec exists, check it isn't exec
                    try:
                        m = os.stat(checknoexec).st_mode
                    except OSError as e:
                        if e.errno != errno.ENOENT:
                            raise
                        with open(checknoexec, "w"):
                            # might fail
                            pass
                        m = os.stat(checknoexec).st_mode
                    if m & EXECFLAGS == 0:
                        # check-exec is exec and check-no-exec is not exec
                        return True
                    # checknoexec exists but is exec - delete it
                    unlink(checknoexec)
                # checkisexec exists but is not exec - delete it
                unlink(checkisexec)

            # check using one file, leave it as checkisexec
            checkdir = cachedir
        else:
            # check directly in path and don't leave checkisexec behind
            checkdir = path
            checkisexec = None
        fh, fn = tempfile.mkstemp(dir=checkdir, prefix="hg-checkexec-")
        try:
            os.close(fh)
            m = os.stat(fn).st_mode
            if m & EXECFLAGS == 0:
                os.chmod(fn, m & 0o777 | EXECFLAGS)
                if os.stat(fn).st_mode & EXECFLAGS != 0:
                    if checkisexec is not None:
                        os.rename(fn, checkisexec)
                        fn = None
                    return True
        finally:
            if fn is not None:
                unlink(fn)
    except (IOError, OSError):
        # we don't care, the user probably won't be able to commit anyway
        return False
    return False


def _checkbrokensymlink(path, msg=None):
    """Check if path or one of its parent directory is a broken symlink.  Raise
    more detailed error about it.

    Subject to filesystem races. ONLY call this when there is already an ENONET
    error.

    If msg is set, it would be used as extra context in the error message.
    """
    src = path
    while src not in ("", "/"):
        src = os.path.dirname(src)
        errmsg = None
        try:
            if os.path.islink(src):
                dest = os.readlink(src)
                if not os.path.exists(src):
                    errmsg = "Symlink %r points to non-existed destination %r" % (
                        src,
                        dest,
                    )
                    if msg:
                        errmsg += " during %s" % msg
        except OSError:
            # Ignore filesystem races (ex. "src" is deleted before readlink)
            pass
        if errmsg:
            raise OSError(errno.ENOENT, errmsg, path)


def checkosfilename(path):
    """Check that the base-relative path is a valid filename on this platform.
    Returns None if the path is ok, or a UI string describing the problem."""
    return None  # on posix platforms, every path is ok


def setbinary(fd):
    pass


def pconvert(path):
    return path


def localpath(path):
    return path


def samefile(fpath1, fpath2):
    """Returns whether path1 and path2 refer to the same file. This is only
    guaranteed to work for files, not directories."""
    return os.path.samefile(fpath1, fpath2)


def samedevice(fpath1, fpath2):
    """Returns whether fpath1 and fpath2 are on the same device. This is only
    guaranteed to work for files, not directories."""
    st1 = os.lstat(fpath1)
    st2 = os.lstat(fpath2)
    return st1.st_dev == st2.st_dev


def getmaxrss():
    """Returns the maximum resident set size of this process, in bytes"""
    res = resource.getrusage(resource.RUSAGE_SELF)

    # Linux returns the maxrss in KB, whereas macOS returns in bytes.
    if sysutil.isdarwin:
        return res.ru_maxrss
    else:
        return res.ru_maxrss * 1024


if sysutil.isdarwin:

    def normcase(path):
        """
        Normalize a filename for OS X-compatible comparison:
        - escape-encode invalid characters
        - decompose to NFD
        - lowercase
        - omit ignored characters [200c-200f, 202a-202e, 206a-206f,feff]

        >>> normcase('UPPER')
        'upper'
        >>> normcase('Caf\\xc3\\xa9')
        'cafã©'
        >>> normcase('\\xc3\\x89')
        'ã\x89'
        >>> normcase('\\xb8\\xca\\xc3\\xca\\xbe\\xc8.JPG') # issue3918
        '¸êãê¾è.jpg'
        """

        try:
            bytepath = path.encode()
            return encoding.asciilower(bytepath).decode()  # exception for non-ASCII
        except UnicodeDecodeError:
            return normcasefallback(path).decode()

    normcasespec = encoding.normcasespecs.lower

    def normcasefallback(path):
        # Decompose then lowercase (HFS+ technote specifies lower)
        enc = unicodedata.normalize(r"NFD", path).lower().encode("utf-8")
        # drop HFS+ ignored characters
        return encoding.hfsignoreclean(enc)

    checkexec = _checkexec

else:
    # os.path.normcase is a no-op, which doesn't help us on non-native
    # filesystems
    def normcase(path):
        return path.lower()

    # what normcase does to ASCII strings
    normcasespec = encoding.normcasespecs.lower
    # fallback normcase function for non-ASCII strings
    normcasefallback = normcase

    checkexec = _checkexec

_needsshellquote = None


def shellquote(s):
    if sys.platform == "OpenVMS":
        return '"%s"' % s
    global _needsshellquote
    if _needsshellquote is None:
        _needsshellquote = re.compile(r"[^a-zA-Z0-9._/+-]").search
    if s and not _needsshellquote(s):
        # "s" shouldn't have to be quoted
        return s
    else:
        return "'%s'" % s.replace("'", "'\\''")


def popen(command, mode="r"):
    return os.popen(command, mode)


def testpid(pid):
    """return False if pid dead, True if running or not sure"""
    if sys.platform == "OpenVMS":
        return True
    try:
        os.kill(pid, 0)
        return True
    except OSError as inst:
        return inst.errno != errno.ESRCH


def explainexit(code):
    """return a 2-tuple (desc, code) describing a subprocess status
    (codes from kill are negative - not os.system/wait encoding)"""
    if code >= 0:
        return _("exited with status %d") % code, code
    return _("killed by signal %d") % -code, -code


def isowner(st):
    """Return True if the stat object st is from the current user."""
    return st.st_uid == os.getuid()


def findexe(command):
    """Find executable for command searching like which does.
    If command is a basename then PATH is searched for command.
    PATH isn't searched if command is an absolute or relative path.
    If command isn't found None is returned."""
    if sys.platform == "OpenVMS":
        return command

    def findexisting(executable):
        "Will return executable if existing file"
        if os.path.isfile(executable) and os.access(executable, os.X_OK):
            return executable
        return None

    if os.sep in command:
        return findexisting(command)

    if sys.platform == "plan9":
        return findexisting(os.path.join("/bin", command))

    for path in encoding.environ.get("PATH", "").split(os.pathsep):
        executable = findexisting(os.path.join(path, command))
        if executable is not None:
            return executable
    return None


def setsignalhandler():
    pass


_wantedkinds = {stat.S_IFREG, stat.S_IFLNK}


def statfiles(files):
    """Stat each file in files. Yield each stat, or None if a file does not
    exist or has a type we don't care about."""
    lstat = os.lstat
    getkind = stat.S_IFMT
    for nf in files:
        try:
            st = lstat(nf)
            if getkind(st.st_mode) not in _wantedkinds:
                st = None
        except OSError as err:
            if err.errno not in (errno.ENOENT, errno.ENOTDIR):
                raise
            st = None
        yield st


def getuser():
    """return name of current user"""
    return getpass.getuser()


def username(uid=None):
    """Return the name of the user with the given uid.

    If uid is None, return the name of the current user."""

    if uid is None:
        uid = os.getuid()
    try:
        return pwd.getpwuid(uid)[0]
    except KeyError:
        return str(uid)


def groupname(gid=None):
    """Return the name of the group with the given gid.

    If gid is None, return the name of the current group."""

    if gid is None:
        gid = os.getgid()
    try:
        return grp.getgrgid(gid)[0]
    except KeyError:
        return str(gid)


def groupmembers(name):
    """Return the list of members of the group with the given
    name, KeyError if the group does not exist.
    """
    return list(grp.getgrnam(name).gr_mem)


def makedir(path: str, notindexed: bool) -> None:
    try:
        os.mkdir(path)
    except OSError as err:
        # Spend a little more effort making the error less mysterious in case
        # there is a broken symlink.
        if err.errno == errno.ENOENT:
            _checkbrokensymlink(path, "makedir")
        raise


def lookupreg(key, name=None, scope=None):
    return None


def hidewindow():
    """Hide current shell window.

    Used to hide the window opened when starting asynchronous
    child process under Windows, unneeded on other systems.
    """


class cachestat:
    def __init__(self, path):
        from . import util

        if path is None:
            self.stat = None
        else:
            try:
                self.stat = util.stat(path)
            except OSError as ex:
                if ex.errno == errno.ENOENT:
                    self.stat = None
                else:
                    raise

    __hash__ = object.__hash__

    def __eq__(self, other):
        try:
            if self.stat is None or other.stat is None:
                return self.stat is None and other.stat is None
            # Only dev, ino, size, mtime and atime are likely to change. Out
            # of these, we shouldn't compare atime but should compare the
            # rest. However, one of the other fields changing indicates
            # something fishy going on, so return False if anything but atime
            # changes.
            return (
                self.stat.st_mode == other.stat.st_mode
                and self.stat.st_ino == other.stat.st_ino
                and self.stat.st_dev == other.stat.st_dev
                and self.stat.st_nlink == other.stat.st_nlink
                and self.stat.st_uid == other.stat.st_uid
                and self.stat.st_gid == other.stat.st_gid
                and self.stat.st_size == other.stat.st_size
                and self.stat.st_mtime == other.stat.st_mtime
                and self.stat.st_ctime == other.stat.st_ctime
            )
        except AttributeError:
            return False

    def __ne__(self, other):
        return not self == other


def executablepath():
    return None  # available on Windows only


def statisexec(st):
    """check whether a stat result is an executable file"""
    return st and (st.st_mode & 0o100 != 0)


def bindunixsocket(sock, path):
    """Bind the UNIX domain socket to the specified path"""
    # use relative path instead of full path at bind() if possible, since
    # AF_UNIX path has very small length limit (107 chars) on common
    # platforms (see sys/un.h)
    dirname, basename = os.path.split(path)
    bakwdfd = None
    if dirname:
        bakwdfd = os.open(".", os.O_DIRECTORY | O_CLOEXEC)
        os.chdir(dirname)
    sock.bind(basename)
    if bakwdfd:
        os.fchdir(bakwdfd)
        os.close(bakwdfd)


def _safehasattr(thing, attr):
    return hasattr(thing, attr)


def syncfile(fp):
    """Makes best effort attempt to make sure all contents previously written
    to the fp is persisted to a permanent storage device."""
    try:
        fp.flush()
        if _safehasattr(fcntl, "F_FULLFSYNC"):
            # OSX specific. See comments in syncdir for discussion on this topic.
            fcntl.fcntl(fp.fileno(), fcntl.F_FULLFSYNC)
        else:
            os.fsync(fp.fileno())
    except (OSError, IOError):
        # do nothing since this is just best effort
        pass


def syncdir(dirpath):
    """Makes best effort attempt to make sure previously issued renames where
    target is a file immediately inside the specified dirpath is persisted
    to a permanent storage device."""

    # Syncing a file is not as simple as it seems.
    #
    # The most common sequence is to sync a file correctly in Unix is `open`,
    # `fflush`, `fsync`, `close`.
    #
    # However, what is the right sequence in case a temporary staging file is
    # involved? This [LWN article][lwn] lists a sequence of necessary actions.
    #
    # 1. create a new temp file (on the same file system!)
    # 2. write data to the temp file
    # 3. fsync() the temp file
    # 4. rename the temp file to the appropriate name
    # 5. fsync() the containing directory
    #
    # While the above step didn't mention flush, it is important to realize
    # that step 2 implies flush. This is also emphasized by the python
    # documentation for [os][os]: one should first do `f.flush()`, and then do
    # `os.fsync(f.fileno())`.
    #
    # Performance wise, this [blog article][thunk] points out that the
    # performance may be affected by other write operations. Here are two of
    # the many reasons, to help provide an intuitive understanding:
    #
    # 1. There is no requirement to prioritize persistence of the file
    # descriptor with an outstanding fsync call;
    # 2. Some filesystems require a certain order of data persistence (for
    # example, to match the order writes were issued).
    #
    # There are also platform specific complexities.
    #
    # * On [OSX][osx], it is helpful to call fcntl with a particular flag
    #   in addition to calling flush. There is an unresolved
    #   [issue][pythonissue] related to hiding this detail from Python
    #   programmers. In Java, implementation of FileChannel.force was changed
    #   to use fcntl since [JDK-8080589][jdk-rfr].
    # * On [Windows][msdn], it is not possible to call FlushFileBuffers on a
    #   Directory Handle. And this [svn mailing list thread][svn] shows that
    #   MoveFile does not provide durability guarantee. It may be possible to
    #   get durability by using MOVEFILE_WRITE_THROUGH flag.
    #
    # It is important that one does not retry `fsync` on failures, which is a
    # point that PostgreSQL learned the hard way, now known as [fsyncgate][pg].
    # The same thread also points out that the sequence of close/re-open/fsync
    # does not provide the same durability guarantee in the presence of sync
    # failures.
    #
    # [lwn]: https://lwn.net/Articles/457667/
    # [os]: https://docs.python.org/3/library/os.html
    # [osx]: https://github.com/untitaker/python-atomicwrites/pull/16/files
    # [jdk-rfr]: http://mail.openjdk.java.net/pipermail/nio-dev/2015-May/003174.html
    # [pg]: https://www.postgresql.org/message-id/flat/CAMsr%2BYHh%2B5Oq4xziwwoEfhoTZgr07vdGG%2Bhu%3D1adXx59aTeaoQ%40mail.gmail.com
    # [thunk]: https://thunk.org/tytso/blog/2009/03/15/dont-fear-the-fsync/
    # [pythonissue]: https://bugs.python.org/issue11877
    # [msdn]: https://docs.microsoft.com/en-us/windows/desktop/FileIO/obtaining-a-handle-to-a-directory
    # [svn]: http://mail-archives.apache.org/mod_mbox/subversion-dev/201506.mbox/%3cCA+t0gk00nz1f+5bpxjNSK5Xnr4rXZx7ywQ_twr5CN6MyZSKw+w@mail.gmail.com%3e
    try:
        dirfd = os.open(dirpath, os.O_DIRECTORY)
        if _safehasattr(fcntl, "F_FULLFSYNC"):
            # osx specific
            fcntl.fcntl(dirfd, fcntl.F_FULLFSYNC)
        else:
            os.fsync(dirfd)
        os.close(dirfd)
    except (OSError, IOError):
        # do nothing since this is just best effort
        pass


def unixsocket():
    return socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
