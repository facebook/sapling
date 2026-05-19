# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# vfs.py - Mercurial 'vfs' classes
#
#  Copyright Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


import abc
import contextlib
import errno
import os
import queue as queuemod
import re
import shutil
import stat
import tempfile
import threading
import typing
from typing import (
    Any,
    BinaryIO,
    Callable,
    IO,
    Iterable,
    Iterator,
    List,
    Optional,
    Tuple,
    Union,
)

import bindings

from . import error, pathutil, util
from .i18n import _


def _avoidambig(path, oldstat):
    """Avoid file stat ambiguity forcibly

    This function causes copying ``path`` file, if it is owned by
    another (see issue5418 and issue5584 for detail).
    """

    def checkandavoid():
        newstat = util.filestat.frompath(path)
        # return whether file stat ambiguity is (already) avoided
        return not newstat.isambig(oldstat) or newstat.avoidambig(path, oldstat)

    if not checkandavoid():
        # simply copy to change owner of path to get privilege to
        # advance mtime (see issue5418)
        util.rename(util.mktempcopy(path), path)
        checkandavoid()


class abstractvfs(abc.ABC):
    """Abstract base class; cannot be instantiated"""

    _backgroundfilecloser: "Optional[backgroundfilecloser]" = None

    def __init__(self, *args, **kwargs):
        """Prevent instantiation; don't call this from subclasses."""
        raise NotImplementedError("attempted instantiating " + str(type(self)))

    @abc.abstractmethod
    def __call__(
        self,
        path: str,
        mode: str = "r",
        text: bool = False,
        atomictemp: bool = False,
        notindexed: bool = False,
        backgroundclose: bool = False,
        checkambig: bool = False,
        auditpath: bool = True,
    ) -> "BinaryIO":
        raise NotImplementedError("must be implemented by subclasses")

    @abc.abstractmethod
    def join(self, path: "Optional[str]", *insidef: str) -> str:
        raise NotImplementedError("must be implemented by subclasses")

    def tryread(self, path):
        """gracefully return an empty string for missing files"""
        try:
            return self.read(path)
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
        return b""

    def tryreadutf8(self, path):
        return self.tryread(path).decode()

    def tryreadlines(self, path, mode="rb"):
        """gracefully return an empty array for missing files"""
        try:
            return self.readlines(path, mode=mode)
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
        return []

    @util.propertycache
    def open(
        self,
    ) -> "Callable[[str, str, bool, bool, bool, bool, bool, bool], BinaryIO]":
        """Open ``path`` file, which is relative to vfs root.

        Newly created directories are marked as "not to be indexed by
        the content indexing service", if ``notindexed`` is specified
        for "write" mode access.
        """
        return self.__call__

    def read(self, path: str) -> bytes:
        with self(path, "rb") as fp:
            return fp.read()

    def readutf8(self, path: str) -> str:
        return self.read(path).decode()

    def readlines(self, path: str, mode: str = "rb") -> "List[bytes]":
        with self(path, mode=mode) as fp:
            return fp.readlines()

    def write(self, path: str, data: bytes, backgroundclose: bool = False) -> int:
        with self(path, "wb", backgroundclose=backgroundclose) as fp:
            return fp.write(data)

    def writeutf8(self, path: str, data: str) -> None:
        self.write(path, data.encode())

    def writelines(
        self, path: str, data: "List[bytes]", mode: str = "wb", notindexed: bool = False
    ) -> None:
        with self(path, mode=mode, notindexed=notindexed) as fp:
            return fp.writelines(data)

    def append(self, path: str, data: bytes) -> int:
        with self(path, "ab") as fp:
            return fp.write(data)

    def basename(self, path: str) -> str:
        """return base element of a path (as os.path.basename would do)

        This exists to allow handling of strange encoding if needed."""
        return os.path.basename(path)

    def chmod(self, path: str, mode: int) -> None:
        return os.chmod(self.join(path), mode)

    def dirname(self, path: str) -> str:
        """return dirname element of a path (as os.path.dirname would do)

        This exists to allow handling of strange encoding if needed."""
        return os.path.dirname(path)

    def exists(self, path: "Optional[str]" = None) -> bool:
        return os.path.exists(self.join(path))

    def fstat(self, fp: "IO") -> "util.wrapped_stat_result":
        return util.fstat(fp)

    def isdir(self, path: "Optional[str]" = None) -> bool:
        return os.path.isdir(self.join(path))

    def isfile(self, path: "Optional[str]" = None) -> bool:
        return os.path.isfile(self.join(path))

    def islink(self, path: "Optional[str]" = None) -> bool:
        return os.path.islink(self.join(path))

    def isexec(self, path: "Optional[str]" = None) -> bool:
        return util.isexec(self.join(path))

    def isfileorlink(self, path: "Optional[str]" = None) -> bool:
        """return whether path is a regular file or a symlink

        Unlike isfile, this doesn't follow symlinks."""
        try:
            st = self.lstat(path)
        except OSError:
            return False
        mode = st.st_mode
        return stat.S_ISREG(mode) or stat.S_ISLNK(mode)

    def reljoin(self, *paths: str) -> str:
        """join various elements of a path together (as os.path.join would do)

        The vfs base is not injected so that path stay relative. This exists
        to allow handling of strange encoding if needed."""
        return os.path.join(*paths)

    def split(self, path: str) -> "Tuple[str, str]":
        """split top-most element of a path (as os.path.split would do)

        This exists to allow handling of strange encoding if needed."""
        return os.path.split(path)

    def lexists(self, path: "Optional[str]" = None) -> bool:
        return os.path.lexists(self.join(path))

    def lstat(self, path: "Optional[str]" = None) -> "util.wrapped_stat_result":
        return util.lstat(self.join(path))

    def listdir(self, path: "Optional[str]" = None) -> "List[str]":
        return os.listdir(self.join(path))

    def makedir(self, path: "Optional[str]" = None, notindexed: bool = True) -> None:
        return util.makedir(self.join(path), notindexed)

    def makedirs(
        self, path: "Optional[str]" = None, mode: "Optional[int]" = None
    ) -> None:
        return util.makedirs(self.join(path), mode)

    def mkdir(self, path: "Optional[str]" = None) -> None:
        return os.mkdir(self.join(path))

    def mkstemp(
        self,
        suffix: str = "",
        prefix: str = "tmp",
        dir: "Optional[str]" = None,
        text: bool = False,
    ) -> "Tuple[int, str]":
        fd, name = tempfile.mkstemp(
            suffix=suffix, prefix=prefix, dir=self.join(dir), text=text
        )
        dname, fname = util.split(name)
        if dir:
            return fd, os.path.join(dir, fname)
        else:
            return fd, fname

    def readdir(
        self,
        path: "Optional[str]" = None,
        stat: "Optional[bool]" = None,
        skip: "Optional[str]" = None,
    ) -> "List[str]":
        return util.listdir(self.join(path), stat, skip)

    def rename(self, src: str, dst: str, checkambig: bool = False) -> None:
        """Rename from src to dst

        checkambig argument is used with util.filestat, and is useful
        only if destination file is guarded by any lock
        (e.g. repo.lock or repo.wlock).

        To avoid file stat ambiguity forcibly, checkambig=True involves
        copying ``src`` file, if it is owned by another. Therefore, use
        checkambig=True only in limited cases (see also issue5418 and
        issue5584 for detail).
        """
        srcpath = self.join(src)
        dstpath = self.join(dst)
        oldstat = checkambig and util.filestat.frompath(dstpath)
        if oldstat and oldstat.stat:
            ret = util.rename(srcpath, dstpath)
            _avoidambig(dstpath, oldstat)
            return ret
        return util.rename(srcpath, dstpath)

    def readlink(self, path: str) -> str:
        target = os.readlink(self.join(path))
        return target.replace("\\", "/") if os.name == "nt" else target

    def removedirs(self, path: "Optional[str]" = None) -> None:
        """Remove a leaf directory and all empty intermediate ones"""
        return util.removedirs(self.join(path))

    def rmtree(
        self,
        path: "Optional[str]" = None,
        ignore_errors: bool = False,
        forcibly: bool = False,
    ) -> None:
        """Remove a directory tree recursively

        If ``forcibly``, this tries to remove READ-ONLY files, too.
        """
        if forcibly:

            def onerror(function, path, excinfo):
                if function is not os.remove:
                    raise
                # read-only files cannot be unlinked under Windows
                s = os.stat(path)
                if (s.st_mode & stat.S_IWRITE) != 0:
                    raise
                os.chmod(path, stat.S_IMODE(s.st_mode) | stat.S_IWRITE)
                os.remove(path)

        else:
            onerror = None
        return shutil.rmtree(
            self.join(path), ignore_errors=ignore_errors, onerror=onerror
        )

    def rmdir(self, path: "Optional[str]" = None) -> None:
        return os.rmdir(self.join(path))

    def setflags(self, path: str, l: bool, x: bool) -> None:
        return util.setflags(self.join(path), l, x)

    def stat(self, path: "Optional[str]" = None) -> "util.wrapped_stat_result":
        return util.stat(self.join(path))

    def unlink(self, path: "Optional[str]" = None) -> None:
        return util.unlink(self.join(path))

    def tryunlink(self, path: "Optional[str]" = None) -> None:
        """Attempt to remove a file, ignoring missing file errors."""
        util.tryunlink(self.join(path))

    def unlinkpath(
        self, path: "Optional[str]" = None, ignoremissing: bool = False
    ) -> None:
        return util.unlinkpath(self.join(path), ignoremissing=ignoremissing)

    def utime(self, path=None, t=None):
        return os.utime(self.join(path), t)

    def walk(
        self,
        path: "Optional[str]" = None,
        onerror: "Optional[Callable[[OSError], None]]" = None,
    ) -> "Iterable[Tuple[str, List[str], List[str]]]":
        """Yield (dirpath, dirs, files) tuple for each directories under path

        ``dirpath`` is relative one from the root of this vfs. This
        uses ``/`` as path separator.

        "The root of this vfs" is represented as empty ``dirpath``.
        """
        root = os.path.normpath(self.join(None))
        # when dirpath == root, dirpath[prefixlen:] becomes empty
        # because len(dirpath) < prefixlen.
        prefixlen = len(pathutil.normasprefix(root))
        for dirpath, dirs, files in os.walk(self.join(path), onerror=onerror):
            yield (util.pconvert(dirpath[prefixlen:]), dirs, files)

    @contextlib.contextmanager
    def backgroundclosing(
        self, ui: "Any", expectedcount: int = -1
    ) -> Iterator[Optional["backgroundfilecloser"]]:
        """Allow files to be closed asynchronously.

        When this context manager is active, ``backgroundclose`` can be passed
        to ``__call__``/``open`` to result in the file possibly being closed
        asynchronously, on a background thread.
        """
        # Sharing backgroundfilecloser between threads is complex and using
        # multiple instances puts us at risk of running out of file descriptors
        # only allow to use backgroundfilecloser when in main thread.
        if not isinstance(
            # pyre isn't aware of threading._MainThread
            # Once we are Python 3 only we should switch to threading.main_thread()
            threading.current_thread(),
            threading._MainThread,  # pyre-fixme
        ):
            yield
            return
        vfs = getattr(self, "vfs", self)
        if vfs._backgroundfilecloser is not None:
            raise error.Abort(_("can only have 1 active background file closer"))

        with backgroundfilecloser(ui, expectedcount=expectedcount) as bfc:
            try:
                vfs._backgroundfilecloser = bfc
                yield bfc
            finally:
                vfs._backgroundfilecloser = None


class vfs(abstractvfs):
    """Operate files relative to a base directory

    This class is used to hide the details of COW semantics and
    remote file access from higher level code.

    'cacheaudited' should be enabled only if (a) vfs object is short-lived, or
    (b) the base directory is managed by hg and considered sort-of append-only.
    See pathutil.pathauditor() for details.
    """

    def __init__(
        self,
        base: str,
        audit: bool = True,
        cacheaudited: bool = False,
        expandpath: bool = False,
        realpath: bool = False,
        disablesymlinks: bool = False,
    ) -> None:
        if expandpath:
            base = util.expandpath(base)
        if realpath:
            base = os.path.realpath(base)
        self.base = base
        self._audit = audit
        # self.audit can be patched by localrepo to devel-warn locking issues.
        # merge.py calls wvfs.audit.check (implicitly requires audit=True)
        # rustvfs operations will audit paths, we might replace self.audit to
        # something lighter weight.
        if audit:
            self.audit = pathutil.pathauditor(self.base, cached=cacheaudited)
        else:
            self.audit = lambda path, mode=None: True
        self.createmode = None
        self._trustnlink = None
        self._disablesymlinks = disablesymlinks

    @util.propertycache
    def _cansymlink(self) -> bool:
        if self._disablesymlinks:
            return False
        return util.checklink(self.base)

    @util.propertycache
    def _rustvfs(self):
        # This will raise if self.base does not exist
        vfs = bindings.io.vfs(self.base, destructive=True)
        vfs.set_supports_symlinks(self._cansymlink)
        return vfs

    @util.propertycache
    def _rustvfs_mkdir(self):
        # Unlike _rustvfs, create self.base directory on demand.
        util.makedirs(self.base, self.createmode)
        return self._rustvfs

    def _rustpath(self, path: "Optional[str]") -> str:
        if not path:
            # bindings vfs does not take None.
            return ""
        if os.path.isabs(path):
            # Compatibility with older Python vfs callers that pass self.join(path).
            # Rust VFS operates on root-relative RepoPath values, so convert full
            # paths back to relative paths. Rust still validates and rejects paths
            # that escape the root, such as "../outside".
            originalpath = path
            try:
                path = os.path.relpath(path, self.base)
            except ValueError:
                raise error.ProgrammingError(
                    "vfs path must be on the same drive with vfs, path: %s; vfs: %s"
                    % (originalpath, self.base)
                )

        if path == os.curdir:
            # bindings vfs prefers '' to '.'.
            return ""

        # Additional verifications like Windows path separator, etc.
        # still happen in the lower Rust layer.
        return path

    def _rustcreatemode(self) -> int:
        # Compatibility with pre-Rust-vfs logic. _fixfilemode used 0o666.
        if self.createmode is None:
            return 0o666
        return typing.cast(int, self.createmode) & 0o666

    def makedir(self, path: "Optional[str]" = None, notindexed: bool = True) -> None:
        # notindexed is a legacy Windows indexing hint with few remaining
        # callers. The Rust no-follow VFS intentionally ignores it.
        return self.mkdir(path)

    def makedirs(
        self, path: "Optional[str]" = None, mode: "Optional[int]" = None
    ) -> None:
        if not path:
            return util.makedirs(self.base, mode)
        self._rustvfs_mkdir.makedirs(self._rustpath(path), mode=mode)

    def mkdir(self, path: "Optional[str]" = None) -> None:
        if not path:
            return os.mkdir(self.base)
        self._rustvfs_mkdir.mkdir(self._rustpath(path))

    def chmod(self, path: str, mode: int) -> None:
        self._rustvfs.set_permissions(self._rustpath(path), mode)

    def exists(self, path: "Optional[str]" = None) -> bool:
        """Return whether path exists, using no-follow lstat-style semantics.

        With Rust VFS, exists() and lexists() are intentionally equivalent.
        """
        return self.lexists(path)

    def isdir(self, path: "Optional[str]" = None) -> bool:
        try:
            return self.lstat(path).is_dir()
        except OSError:
            return False

    def isfile(self, path: "Optional[str]" = None) -> bool:
        try:
            return self.lstat(path).is_file()
        except OSError:
            return False

    def islink(self, path: "Optional[str]" = None) -> bool:
        try:
            return self.lstat(path).is_symlink()
        except OSError:
            return False

    def isexec(self, path: "Optional[str]" = None) -> bool:
        try:
            return self.lstat(path).is_executable()
        except OSError:
            return False

    def isfileorlink(self, path: "Optional[str]" = None) -> bool:
        try:
            st = self.lstat(path)
            return st.is_file() or st.is_symlink()
        except OSError:
            return False

    def lexists(self, path: "Optional[str]" = None) -> bool:
        try:
            return self._rustvfs.exists(self._rustpath(path))
        except FileNotFoundError:
            # vfs itself is missing
            return False

    def lstat(self, path: "Optional[str]" = None):
        return self._rustvfs.metadata(self._rustpath(path))

    def readlink(self, path: str) -> str:
        return self._rustvfs.read(self._rustpath(path)).decode()

    def rmtree(
        self,
        path: "Optional[str]" = None,
        ignore_errors: bool = False,
        forcibly: bool = False,
    ) -> None:
        try:
            self._rustvfs.rmtree(self._rustpath(path))
        except Exception:
            if not ignore_errors:
                raise

    def rmdir(self, path: "Optional[str]" = None) -> None:
        self._rustvfs.rmdir(self._rustpath(path))

    def setflags(self, path: str, l: bool, x: bool) -> None:
        # some code paths pass in int flags, convert to bool, required by rustvfs
        l = bool(l)
        x = bool(x)
        path = self._rustpath(path)
        metadata = self._rustvfs.metadata(path)
        islink = metadata.is_symlink()
        if l:
            if not islink:
                data = self._rustvfs.read(path)
                self._rustvfs.unlink(path)
                self._rustvfs.write(path, data, "l")
            return

        if islink:
            data = self._rustvfs.read(path)
            self._rustvfs.unlink(path)
            self._rustvfs.write(path, data, "x" if x else "")
            return

        self._rustvfs.set_executable(path, x)

    def stat(self, path: "Optional[str]" = None):
        """Return no-follow lstat-style metadata.

        With Rust VFS, stat() and lstat() are intentionally equivalent.
        """
        return self.lstat(path)

    def unlink(self, path: "Optional[str]" = None) -> None:
        self._rustvfs.unlink(self._rustpath(path))

    def tryunlink(self, path: "Optional[str]" = None) -> None:
        self._rustvfs.tryunlink(self._rustpath(path))

    def unlinkpath(
        self, path: "Optional[str]" = None, ignoremissing: bool = False
    ) -> None:
        self._rustvfs.unlinkpath(self._rustpath(path), ignoremissing=ignoremissing)

    def __call__(
        self,
        path: str,
        mode: str = "r",
        text: bool = False,
        atomictemp: bool = False,
        notindexed: bool = False,
        backgroundclose: bool = False,
        checkambig: bool = False,
        auditpath: bool = True,
    ) -> "BinaryIO":
        """Open ``path`` file, which is relative to vfs root.

        ``notindexed``, ``backgroundclose``, ``checkambig`` are for historical
        Compatibility, they are ignored.

        ``auditpath`` turns on extra auditing (e.g. devel-warn). The rust vfs
        will audit paths (for illegal components) and prevent writing through
        symlinks regardless.
        """
        assert isinstance(path, str)
        assert isinstance(mode, str)
        assert not text, "open as text is no longer supported"
        path = self._rustpath(path)
        if auditpath:
            self.audit(path, mode=mode)
            if self._audit:
                r = util.checkosfilename(path)
                if r:
                    raise error.Abort("%s: %r" % (r, path))

        createmode = self._rustcreatemode()
        rustvfs = self._rustvfs if mode in ("r", "rb") else self._rustvfs_mkdir
        return rustvfs.open(path, mode=mode, perm=createmode, atomicreplace=atomictemp)

    def symlink(self, src: "Union[bytes, str]", dst: str) -> None:
        dst = self._rustpath(dst)
        if isinstance(src, str):
            src = src.encode()
        rustvfs = self._rustvfs_mkdir
        rustvfs.tryunlink(dst)
        rustvfs.write(dst, src, "l")

    def join(self, path: "Optional[str]", *insidef: str) -> str:
        if path:
            return os.path.join(self.base, path, *insidef)
        else:
            return self.base


opener = vfs


class readonlyvfs(util.proxy_wrapper, abstractvfs):
    """Wrapper vfs preventing any writing."""

    def __call__(
        self, path: str, mode: str = "r", *args: bool, **kw: bool
    ) -> "BinaryIO":
        if mode not in ("r", "rb"):
            raise error.Abort(_("this vfs is read only"))
        return self.inner(path, mode, *args, **kw)

    def join(self, path: "Optional[str]", *insidef: str) -> str:
        return self.inner.join(path, *insidef)


class closewrapbase:
    """Base class of wrapper, which hooks closing

    Do not instantiate outside of the vfs layer.
    """

    def __init__(self, fh):
        object.__setattr__(self, r"_origfh", fh)

    def __getattr__(self, attr):
        return getattr(self._origfh, attr)

    def __setattr__(self, attr, value):
        return setattr(self._origfh, attr, value)

    def __delattr__(self, attr):
        return delattr(self._origfh, attr)

    def __enter__(self):
        return self._origfh.__enter__()

    def __exit__(self, exc_type, exc_value, exc_tb):
        raise NotImplementedError("attempted instantiating " + str(type(self)))

    def close(self):
        raise NotImplementedError("attempted instantiating " + str(type(self)))


class delayclosedfile(closewrapbase):
    """Proxy for a file object whose close is delayed.

    Do not instantiate outside of the vfs layer.
    """

    def __init__(self, fh, closer):
        super(delayclosedfile, self).__init__(fh)
        object.__setattr__(self, r"_closer", closer)

    def __exit__(self, exc_type, exc_value, exc_tb):
        self._closer.close(self._origfh)

    def close(self):
        self._closer.close(self._origfh)


class backgroundfilecloser:
    """Coordinates background closing of file handles on multiple threads."""

    def __init__(self, ui, expectedcount=-1):
        self._running = False
        self._entered = False
        self._threads = []
        self._threadexception = None

        # Only Windows/NTFS has slow file closing. So only enable by default
        # on that platform. But allow to be enabled elsewhere for testing.
        defaultenabled = util.iswindows
        enabled = ui.configbool("worker", "backgroundclose", defaultenabled)

        if not enabled:
            return

        # There is overhead to starting and stopping the background threads.
        # Don't do background processing unless the file count is large enough
        # to justify it.
        minfilecount = ui.configint("worker", "backgroundcloseminfilecount")
        # FUTURE dynamically start background threads after minfilecount closes.
        # (We don't currently have any callers that don't know their file count)
        if expectedcount > 0 and expectedcount < minfilecount:
            return

        maxqueue = ui.configint("worker", "backgroundclosemaxqueue")
        threadcount = ui.configint("worker", "backgroundclosethreadcount")

        self._queue = queuemod.Queue(maxsize=maxqueue)
        self._running = True

        for i in range(threadcount):
            t = threading.Thread(target=self._worker, name="backgroundcloser")
            self._threads.append(t)
            t.start()

    def __enter__(self):
        self._entered = True
        return self

    def __exit__(self, exc_type, exc_value, exc_tb):
        self._running = False

        # Wait for threads to finish closing so open files don't linger for
        # longer than lifetime of context manager.
        for t in self._threads:
            t.join()

    def _worker(self):
        """Main routine for worker thread."""
        while True:
            try:
                fh = self._queue.get(block=True, timeout=0.100)
                # Need to catch or the thread will terminate and
                # we could orphan file descriptors.
                try:
                    fh.close()
                except Exception as e:
                    # Stash so can re-raise from main thread later.
                    self._threadexception = e
            except queuemod.Empty:
                if not self._running:
                    break

    def close(self, fh):
        """Schedule a file for closing."""
        if not self._entered:
            raise error.Abort(_("can only call close() when context manager active"))

        # If a background thread encountered an exception, raise now so we fail
        # fast. Otherwise we may potentially go on for minutes until the error
        # is acted on.
        if self._threadexception:
            e = self._threadexception
            self._threadexception = None
            raise e

        # If we're not actively running, close synchronously.
        if not self._running:
            fh.close()
            return

        self._queue.put(fh, block=True, timeout=None)


class checkambigatclosing(closewrapbase):
    """Proxy for a file object, to avoid ambiguity of file stat

    See also util.filestat for detail about "ambiguity of file stat".

    This proxy is useful only if the target file is guarded by any
    lock (e.g. repo.lock or repo.wlock)

    Do not instantiate outside of the vfs layer.
    """

    def __init__(self, fh):
        super(checkambigatclosing, self).__init__(fh)
        object.__setattr__(self, r"_oldstat", util.filestat.frompath(fh.name))

    def _checkambig(self):
        oldstat = self._oldstat
        if oldstat.stat:
            _avoidambig(self._origfh.name, oldstat)

    def __exit__(self, exc_type, exc_value, exc_tb):
        self._origfh.__exit__(exc_type, exc_value, exc_tb)
        self._checkambig()

    def close(self):
        self._origfh.close()
        self._checkambig()


# 64 bytes for SHA256
_blobvfsre = re.compile(r"\A[a-f0-9]{64}\Z")


class blobvfs(vfs):
    def join(self, path: "Optional[str]", *insidef: str) -> str:
        """split the path at first two characters, like: XX/XXXXX..."""
        if path is None or not _blobvfsre.match(path):
            raise error.ProgrammingError("unexpected blob vfs path: %r" % (path,))
        if insidef:
            raise error.ProgrammingError(
                "unexpected blob vfs path: %r, %r" % (path, insidef)
            )
        return super(blobvfs, self).join(path[0:2], path[2:])

    def walk(
        self,
        path: "Optional[str]" = None,
        onerror: "Optional[Callable[[OSError], None]]" = None,
    ) -> "Iterable[Tuple[str, List[str], List[str]]]":
        """Yield (dirpath, [], oids) tuple for blobs under path

        Oids only exist in the root of this vfs, so dirpath is always ''.
        """
        root = os.path.normpath(self.base)
        # when dirpath == root, dirpath[prefixlen:] becomes empty
        # because len(dirpath) < prefixlen.
        prefixlen = len(pathutil.normasprefix(root))
        oids = []

        for dirpath, dirs, files in os.walk(
            self.reljoin(self.base, path or ""), onerror=onerror
        ):
            dirpath = dirpath[prefixlen:]

            # Silently skip unexpected files and directories
            if len(dirpath) == 2:
                oids.extend(
                    [dirpath + f for f in files if _blobvfsre.match(dirpath + f)]
                )

        yield ("", [], oids)

    def linktovfs(self, oid, vfs):
        """Hardlink a file to another blob vfs"""
        src = self.join(oid)
        dst = vfs.join(oid)
        util.makedirs(os.path.dirname(dst))
        util.copyfile(src, dst, hardlink=True)
