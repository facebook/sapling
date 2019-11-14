# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# posix.py - Posix utility function implementations for Mercurial
#
#  Copyright 2005-2009 Matt Mackall <mpm@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import contextlib
import errno
import fcntl
import getpass
import grp
import os
import pwd
import re
import resource
import select
import stat
import sys
import tempfile
import unicodedata

from edenscmnative import osutil

from . import encoding, error, fscap, pycompat
from .i18n import _


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
    >>> for f in [b'/absolute/path/to/file',
    ...           b'relative/path/to/file',
    ...           b'file_alone',
    ...           b'path/to/directory/',
    ...           b'/multiple/path//separators',
    ...           b'/file_at_root',
    ...           b'///multiple_leading_separators_at_root',
    ...           b'']:
    ...     assert split(f) == posixpath.split(f), f
    """
    ht = p.rsplit("/", 1)
    if len(ht) == 1:
        return "", p
    nh = ht[0].rstrip("/")
    if nh:
        return nh, ht[1]
    return ht[0] + "/", ht[1]


@contextlib.contextmanager
def _locked(pathname):
    """Context manager locking on a path. Use this to make short decisions
    in an "atomic" way across multiple processes.

    pathname must already exist.
    """
    fd = os.open(pathname, os.O_RDONLY | os.O_NOFOLLOW | O_CLOEXEC)
    fcntl.flock(fd, fcntl.LOCK_EX)
    try:
        yield
    finally:
        os.close(fd)


def _issymlinklockstale(oldinfo, newinfo):
    """Test if the lock is stale (owned by dead process).

    Only works for symlink locks. Both oldinfo and newinfo have the form:

        info := namespace + ":" + pid
        namespace := hostname (non-Linux) | hostname + "/" + pid-namespace (Linux)

    Return True if it's certain that oldinfo is stale. Return False if it's not
    or not sure.
    """

    if ":" not in oldinfo or ":" not in newinfo:
        # Malformed. Unsure.
        return False

    oldhost, oldpid = oldinfo.split(":", 1)
    newhost, newpid = newinfo.split(":", 1)

    if oldhost != newhost:
        # Not in a same host, or namespace. Unsure.
        return False

    try:
        pid = int(oldpid)
    except ValueError:
        # pid is not a number. Unsure.
        return False

    return not testpid(pid)


def makelock(info, pathname):
    """Try to make a lock at given path. Write info inside it.

    Stale non-symlink or symlink locks are removed automatically. Symlink locks
    are only used by legacy code, or by the new code temporarily to prevent
    issues running together with the old code.

    Return file descriptor on success. The file descriptor must be kept
    for the lock to be effective.

    Raise EAGAIN, likely caused by another process holding the lock.
    Raise EEXIST or ELOOP, likely caused by another legacy hg process
    holding the lock.

    Can also raise other errors or those errors for other causes.
    Callers should convert errors to error.LockHeld or error.LockUnavailable.
    """

    # This is a bit complex, since it aims to support old lock code where the
    # lock file is removed when the lock is released.  The simpler version
    # where the lock file does not get unlinked when releasing the lock is:
    #
    #     # Open the file. Create on demand. Fail if it's a symlink.
    #     fd = os.open(pathname, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | O_CLOEXEC)
    #     try:
    #         fcntl.flock(fd, fcntl.LOCK_NB | fcntl.LOCK_EX)
    #         os.write(fd, info)
    #     except (OSError, IOError):
    #         os.close(fd)
    #         raise
    #     else:
    #         return fd
    #
    # With "unlink" on release, the above simple logic can break in this way:
    #
    #     [process 1] got fd.
    #     [process 2] got fd pointing to a same file.
    #     [process 1] .... release lock. file unlinked.
    #     [process 2] flock on fd. (broken lock - file was gone)
    #
    # A direct fix is to use O_EXCL to make sure the file is created by the
    # current process, then use "flock". That means there needs to be a way to
    # remove stale lock, and that is not easy. A naive check and delete can
    # break subtly:
    #
    #     [process 1] to check stale lock - got fd.
    #     [process 2] ... release lock. file unlinked.
    #     [process 1] flock taken, decided to remove file.
    #     [process 3] create a new lock.
    #     [process 1] unlink lock file. (wrong - removed the wrong lock)
    #
    # Instead of figuring out how to handle all corner cases carefully, we just
    # always lock the parent directory when doing "racy" write operations
    # (creating a lock, or removing a stale lock). So they become "atomic" and
    # safe. There are 2 kinds of write operations that can happen without
    # taking the directory lock:
    #
    #   - Legacy symlink lock creation or deletion. The new code errors out
    #     when it saw a symlink lock (os.open(..., O_NOFOLLOW) and os.rename).
    #     So they play well with each other.
    #   - Unlinking lock file when when releasing. The release logic is holding
    #     the flock. So it knows nobody else has the lock. Therefore it can do
    #     the unlink without extra locking.
    dirname = os.path.dirname(pathname)
    with _locked(dirname or "."):
        # Check and remove stale lock
        try:
            fd = os.open(pathname, os.O_RDONLY | os.O_NOFOLLOW | O_CLOEXEC)
        except (OSError, IOError) as ex:
            # ELOOP: symlink lock. Check if it's stale.
            if ex.errno == errno.ELOOP:
                oldinfo = os.readlink(pathname)
                if _issymlinklockstale(oldinfo, info):
                    os.unlink(pathname)
            elif ex.errno != errno.ENOENT:
                raise
        else:
            try:
                # Use fcntl to test stale lock
                fcntl.flock(fd, fcntl.LOCK_NB | fcntl.LOCK_EX)
                os.unlink(pathname)
            except (OSError, IOError) as ex:
                # EAGAIN: lock taken - return directly
                # ENOENT: lock removed already - continue
                if ex.errno != errno.ENOENT:
                    raise
            finally:
                os.close(fd)

        # Create symlink placeholder. Make sure the file replaced by
        # "os.rename" can only be this symlink. This avoids race condition
        # when legacy code creates the symlink lock without locking the
        # parent directory.
        #
        # This is basically the legacy lock logic.
        placeholdercreated = False
        try:
            os.symlink(info, pathname)
            placeholdercreated = True
        except (IOError, OSError) as ex:
            if ex.errno == errno.EEXIST:
                raise
        except AttributeError:
            pass

        if not placeholdercreated:
            # No symlink support. Suboptimal. Create a placeholder by using an
            # empty file.  Other legacy process might see a "malformed lock"
            # temporarily. New processes won't see this because both "readlock"
            # and "islocked" take the directory lock.
            fd = os.open(pathname, os.O_CREAT | os.O_WRONLY | os.O_EXCL | O_CLOEXEC)
            os.close(fd)

        try:
            # Create new lock.
            #
            # mkstemp sets FD_CLOEXEC automatically. For thread-safety. Threads
            # used here (progress, profiling, Winodws update worker) do not fork.
            # So it's fine to not patch `os.open` here.
            fd, tmppath = tempfile.mkstemp(prefix="makelock", dir=dirname)
            try:
                os.fchmod(fd, 0o664)
                fcntl.flock(fd, fcntl.LOCK_NB | fcntl.LOCK_EX)
                os.write(fd, info)
                os.rename(tmppath, pathname)
                return fd
            except Exception:
                unlink(tmppath)
                os.close(fd)
                raise
        except Exception:
            # Remove the placeholder
            unlink(pathname)
            raise


def readlock(pathname):
    with _locked(os.path.dirname(pathname) or "."):
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


def releaselock(lockfd, pathname):
    # Explicitly unlock. This avoids issues when a
    # forked process inherits the flock.
    fcntl.flock(lockfd, fcntl.LOCK_UN)
    os.close(lockfd)
    os.unlink(pathname)


def islocked(pathname):
    with _locked(os.path.dirname(pathname) or "."):
        try:
            fd = os.open(pathname, os.O_RDONLY | os.O_NOFOLLOW | O_CLOEXEC)
        except OSError as ex:
            # ELOOP, ENOENT, EPERM, ...
            # Only treat ENOENT as "not locked".
            return ex.errno != errno.ENOENT
        try:
            fcntl.flock(fd, fcntl.LOCK_NB | fcntl.LOCK_EX)
            return False
        except IOError as ex:
            if ex.errno == errno.EAGAIN:
                return True
            else:
                raise
        finally:
            os.close(fd)


def openhardlinks():
    """return true if it is safe to hold open file handles to hardlinks"""
    return True


def nlinks(name):
    """return number of hardlinks for the given file"""
    return os.lstat(name).st_nlink


def parsepatchoutput(output_line):
    """parses the output produced by patch and returns the filename"""
    pf = output_line[14:]
    if pycompat.sysplatform == "OpenVMS":
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


def setflags(f, l, x):
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


def _findmountpoint(dirpath):
    """Return the mount point of dirpath (best-effort)"""
    path = os.path.abspath(dirpath)
    while True:
        if os.path.ismount(path):
            return path
        nextpath = os.path.dirname(path)
        if nextpath == path:
            return None
        path = nextpath


def _findstdev(mountpoint):
    """Return st_dev major:minor (ex. "8:1") for the given mountpoint.

    Linux-only. Require /proc to be mounted properly.
    """
    if mountpoint:
        try:
            for line in open("/proc/self/mountinfo"):
                # Refer to "man 5 proc" for the format of mountinfo
                columns = line.split(" ")
                if len(columns) != 11:
                    continue
                if mountpoint == columns[4]:
                    stdev = columns[2]
                    return stdev
        except (OSError, IOError):
            return None
    return None


def _findudevprops(stdev):
    """Return udev properties as a dict (best-effort) for a block device.

    Linux-only. stdev is a string returned by _findstdev.
    """
    # "b": block device
    udevpath = "/run/udev/data/b%s" % stdev
    try:
        return dict(l.rstrip().split("=", 1) for l in open(udevpath) if "=" in l)
    except (OSError, IOError):
        return {}


def getfstype(dirpath):
    """Get the filesystem type name from a directory (best-effort)

    Returns None if we are unsure, or hit errors like ENOENT, EPERM, etc.
    """
    try:
        if _iseden(dirpath):
            return "eden"
        result = osutil.getfstype(dirpath)
        if result == "fuse" and pycompat.islinux:
            # Spend some extra efforts to find out the actual filesystem
            stdev = _findstdev(_findmountpoint(dirpath))
            if stdev:
                props = _findudevprops(stdev)
                # The name is used in well-known projects: systemd (udev) and
                # util-linux (libblkid). Although it does not seem to be
                # documented publicly.
                fstype = props.get("E:ID_FS_TYPE")
                if fstype:
                    result += ".%s" % fstype
    except (IOError, OSError):
        # Not fatal
        result = None
    return result


def _iseden(dirpath):
    """Returns True if the specified directory is the root directory of,
    or is a sub-directory of an Eden mount."""
    return os.path.islink(os.path.join(dirpath, ".eden", "root"))


def _checkexec(path):
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
        cachedir = os.path.join(path, ".hg", "cache")
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
                        open(checknoexec, "w").close()  # might fail
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


def _checklink(path):
    """check whether the given path is on a symlink-capable filesystem"""
    cap = fscap.getfscap(getfstype(path), fscap.SYMLINK)
    if cap is not None:
        return cap

    # mktemp is not racy because symlink creation will fail if the
    # file already exists
    while True:
        cachedir = os.path.join(path, ".hg", "cache")
        checklink = os.path.join(cachedir, "checklink")
        # try fast path, read only
        if os.path.islink(checklink):
            return True
        if os.path.isdir(cachedir):
            checkdir = cachedir
        else:
            checkdir = path
            cachedir = None
        fscheckdir = pycompat.fsdecode(checkdir)
        name = tempfile.mktemp(dir=fscheckdir, prefix=r"checklink-")
        name = pycompat.fsencode(name)
        try:
            fd = None
            if cachedir is None:
                fd = tempfile.NamedTemporaryFile(
                    dir=fscheckdir, prefix=r"hg-checklink-"
                )
                target = pycompat.fsencode(os.path.basename(fd.name))
            else:
                # create a fixed file to link to; doesn't matter if it
                # already exists.
                target = "checklink-target"
                try:
                    open(os.path.join(cachedir, target), "w").close()
                except IOError as inst:
                    if inst[0] == errno.EACCES:
                        # If we can't write to cachedir, just pretend
                        # that the fs is readonly and by association
                        # that the fs won't support symlinks. This
                        # seems like the least dangerous way to avoid
                        # data loss.
                        return False
                    raise
            try:
                os.symlink(target, name)
                if cachedir is None:
                    unlink(name)
                else:
                    try:
                        os.rename(name, checklink)
                    except OSError:
                        unlink(name)
                return True
            except OSError as inst:
                # link creation might race, try again
                if inst[0] == errno.EEXIST:
                    continue
                raise
            finally:
                if fd is not None:
                    fd.close()
        except AttributeError:
            return False
        except OSError as inst:
            # sshfs might report failure while successfully creating the link
            if inst[0] == errno.EIO and os.path.exists(name):
                unlink(name)
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
    if pycompat.isdarwin:
        return res.ru_maxrss
    else:
        return res.ru_maxrss * 1024


if pycompat.isdarwin:

    def normcase(path):
        """
        Normalize a filename for OS X-compatible comparison:
        - escape-encode invalid characters
        - decompose to NFD
        - lowercase
        - omit ignored characters [200c-200f, 202a-202e, 206a-206f,feff]

        >>> normcase(b'UPPER')
        'upper'
        >>> normcase(b'Caf\\xc3\\xa9')
        'cafe\\xcc\\x81'
        >>> normcase(b'\\xc3\\x89')
        'e\\xcc\\x81'
        >>> normcase(b'\\xb8\\xca\\xc3\\xca\\xbe\\xc8.JPG') # issue3918
        '%b8%ca%c3\\xca\\xbe%c8.jpg'
        """

        try:
            return encoding.asciilower(path)  # exception for non-ASCII
        except UnicodeDecodeError:
            return normcasefallback(path)

    normcasespec = encoding.normcasespecs.lower

    def normcasefallback(path):
        try:
            u = path.decode("utf-8")
        except UnicodeDecodeError:
            # OS X percent-encodes any bytes that aren't valid utf-8
            s = ""
            pos = 0
            l = len(path)
            while pos < l:
                try:
                    c = encoding.getutf8char(path, pos)
                    pos += len(c)
                except ValueError:
                    c = "%%%02X" % ord(path[pos : pos + 1])
                    pos += 1
                s += c

            u = s.decode("utf-8")

        # Decompose then lowercase (HFS+ technote specifies lower)
        enc = unicodedata.normalize(r"NFD", u).lower().encode("utf-8")
        # drop HFS+ ignored characters
        return encoding.hfsignoreclean(enc)

    checkexec = _checkexec
    checklink = _checklink

elif pycompat.sysplatform == "cygwin":
    # workaround for cygwin, in which mount point part of path is
    # treated as case sensitive, even though underlying NTFS is case
    # insensitive.

    # default mount points
    cygwinmountpoints = sorted(["/usr/bin", "/usr/lib", "/cygdrive"], reverse=True)

    # use upper-ing as normcase as same as NTFS workaround
    def normcase(path):
        pathlen = len(path)
        if (pathlen == 0) or (path[0] != pycompat.ossep):
            # treat as relative
            return encoding.upper(path)

        # to preserve case of mountpoint part
        for mp in cygwinmountpoints:
            if not path.startswith(mp):
                continue

            mplen = len(mp)
            if mplen == pathlen:  # mount point itself
                return mp
            if path[mplen] == pycompat.ossep:
                return mp + encoding.upper(path[mplen:])

        return encoding.upper(path)

    normcasespec = encoding.normcasespecs.other
    normcasefallback = normcase

    # Cygwin translates native ACLs to POSIX permissions,
    # but these translations are not supported by native
    # tools, so the exec bit tends to be set erroneously.
    # Therefore, disable executable bit access on Cygwin.
    def checkexec(path):
        return False

    # Similarly, Cygwin's symlink emulation is likely to create
    # problems when Mercurial is used from both Cygwin and native
    # Windows, with other native tools, or on shared volumes
    def checklink(path):
        return False


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
    checklink = _checklink

_needsshellquote = None


def shellquote(s):
    if pycompat.sysplatform == "OpenVMS":
        return '"%s"' % s
    global _needsshellquote
    if _needsshellquote is None:
        _needsshellquote = re.compile(br"[^a-zA-Z0-9._/+-]").search
    if s and not _needsshellquote(s):
        # "s" shouldn't have to be quoted
        return s
    else:
        return "'%s'" % s.replace("'", "'\\''")


def quotecommand(cmd):
    return cmd


def popen(command, mode="r"):
    return os.popen(command, mode)


def testpid(pid):
    """return False if pid dead, True if running or not sure"""
    if pycompat.sysplatform == "OpenVMS":
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
    if pycompat.sysplatform == "OpenVMS":
        return command

    def findexisting(executable):
        "Will return executable if existing file"
        if os.path.isfile(executable) and os.access(executable, os.X_OK):
            return executable
        return None

    if pycompat.ossep in command:
        return findexisting(command)

    if pycompat.sysplatform == "plan9":
        return findexisting(os.path.join("/bin", command))

    for path in encoding.environ.get("PATH", "").split(pycompat.ospathsep):
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
    return pycompat.fsencode(getpass.getuser())


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


def spawndetached(args):
    return os.spawnvp(os.P_NOWAIT | getattr(os, "P_DETACH", 0), args[0], args)


def gethgcmd():
    return sys.argv[:1]


def makedir(path, notindexed):
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
    pass


class cachestat(object):
    def __init__(self, path):
        if path is None:
            self.stat = None
        else:
            try:
                self.stat = os.stat(path)
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


def statislink(st):
    """check whether a stat result is a symlink"""
    return st and stat.S_ISLNK(st.st_mode)


def statisexec(st):
    """check whether a stat result is an executable file"""
    return st and (st.st_mode & 0o100 != 0)


def poll(fds):
    """block until something happens on any file descriptor

    This is a generic helper that will check for any activity
    (read, write.  exception) and return the list of touched files.

    In unsupported cases, it will raise a NotImplementedError"""
    try:
        while True:
            try:
                res = select.select(fds, fds, fds)
                break
            except select.error as inst:
                if inst.args[0] == errno.EINTR:
                    continue
                raise
    except ValueError:  # out of range file descriptor
        raise NotImplementedError()
    return sorted(list(set(sum(res, []))))


def readpipe(pipe):
    """Read all available data from a pipe."""
    # We can't fstat() a pipe because Linux will always report 0.
    # So, we set the pipe to non-blocking mode and read everything
    # that's available.
    flags = fcntl.fcntl(pipe, fcntl.F_GETFL)
    flags |= os.O_NONBLOCK
    oldflags = fcntl.fcntl(pipe, fcntl.F_SETFL, flags)

    try:
        chunks = []
        while True:
            try:
                s = pipe.read()
                if not s:
                    break
                chunks.append(s)
            except IOError:
                break

        return "".join(chunks)
    finally:
        fcntl.fcntl(pipe, fcntl.F_SETFL, oldflags)


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
    # deferred import to avoid circular import
    from . import util

    return util.safehasattr(thing, attr)


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
