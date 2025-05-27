# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# archival.py - revision archival for mercurial
#
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


import gzip
import io
import os
import struct
import tarfile
import time
import zipfile
import zlib

from . import error, formatter, progress, util, vfs as vfsmod
from .i18n import _

# from unzip source code:
_UNX_IFREG = 0x8000
_UNX_IFLNK = 0xA000


def tidyprefix(dest, kind, prefix):
    """choose prefix to use for names in archive.  make sure prefix is
    safe for consumers."""

    if prefix:
        prefix = util.normpath(prefix)
    else:
        if not isinstance(dest, str):
            raise ValueError("dest must be string if no prefix")
        prefix = os.path.basename(dest)
        lower = prefix.lower()
        for sfx in exts.get(kind, []):
            if lower.endswith(sfx):
                prefix = prefix[: -len(sfx)]
                break
    lpfx = os.path.normpath(util.localpath(prefix))
    prefix = util.pconvert(lpfx)
    if not prefix.endswith("/"):
        prefix += "/"
    # Drop the leading '.' path component if present, so Windows can read the
    # zip files (issue4634)
    if prefix.startswith("./"):
        prefix = prefix[2:]
    if prefix.startswith("../") or os.path.isabs(lpfx) or "/../" in prefix:
        raise error.Abort(_("archive prefix contains illegal components"))
    return prefix


exts = {
    "tar": [".tar"],
    "tbz2": [".tbz2", ".tar.bz2"],
    "tgz": [".tgz", ".tar.gz"],
    "zip": [".zip"],
}


def guesskind(dest):
    for kind, extensions in exts.items():
        if any(dest.endswith(ext) for ext in extensions):
            return kind
    return None


def _rootctx(repo):
    # repo[0] may be hidden
    for rev in repo:
        return repo[rev]
    return repo["null"]


def buildmetadata(ctx):
    """build content of .sl_archival.txt"""
    repo = ctx.repo()

    default = (
        r"repo: {root}\n"
        r'node: {ifcontains(rev, revset("wdir()"),'
        r'"{p1node}{dirty}", "{node}")}\n'
        r"branch: {branch|utf8}\n"
    )

    opts = {"template": repo.ui.config("experimental", "archivemetatemplate", default)}

    out = io.StringIO()

    fm = formatter.formatter(repo.ui, out, "archive", opts)
    fm.startitem()
    fm.context(ctx=ctx)
    fm.data(root=_rootctx(repo).hex())

    if ctx.rev() is None:
        dirty = ""
        if ctx.dirty(missing=True):
            dirty = "+"
        fm.data(dirty=dirty)
    fm.end()

    return out.getvalue().encode()


class tarit:
    """write archive to tar file or stream.  can write uncompressed,
    or compress with gzip or bzip2."""

    def __init__(self, dest, mtime, kind=""):
        self.mtime = mtime
        self.fileobj = None

        def taropen(mode, name="", fileobj=None):
            if kind == "gz":
                mode = mode[0]
                if not fileobj:
                    fileobj = open(name, mode + "b")
                gzfileobj = gzip.GzipFile(
                    name, mode + "b", zlib.Z_BEST_COMPRESSION, fileobj, mtime=mtime
                )
                self.fileobj = gzfileobj
                return tarfile.TarFile.taropen(name, mode, gzfileobj)
            else:
                return tarfile.open(name, mode + kind, fileobj)

        if isinstance(dest, str):
            self.z = taropen("w:", name=dest)
        else:
            self.z = taropen("w|", fileobj=dest)

    def addfile(self, name, mode, islink, data):
        i = tarfile.TarInfo(name)
        i.mtime = self.mtime
        i.size = len(data)
        if islink:
            i.type = tarfile.SYMTYPE
            i.mode = 0o777
            i.linkname = data.decode()
            data = None
            i.size = 0
        else:
            i.mode = mode
            data = io.BytesIO(data)
        self.z.addfile(i, data)

    def done(self):
        self.z.close()
        if self.fileobj:
            self.fileobj.close()


class tellable:
    """provide tell method for zipfile.ZipFile when writing to http
    response file object."""

    def __init__(self, fp):
        self.fp = fp
        self.offset = 0

    def __getattr__(self, key):
        return getattr(self.fp, key)

    def write(self, s):
        self.fp.write(s)
        self.offset += len(s)

    def tell(self):
        return self.offset


class zipit:
    """write archive to zip file or stream.  can write uncompressed,
    or compressed with deflate."""

    def __init__(self, dest, mtime, compress=True):
        if not isinstance(dest, str):
            try:
                dest.tell()
            except (AttributeError, IOError):
                dest = tellable(dest)
        self.z = zipfile.ZipFile(
            dest, "w", compress and zipfile.ZIP_DEFLATED or zipfile.ZIP_STORED
        )

        # Python's zipfile module emits deprecation warnings if we try
        # to store files with a date before 1980.
        epoch = 315532800  # calendar.timegm((1980, 1, 1, 0, 0, 0, 1, 1, 0))
        if mtime < epoch:
            mtime = epoch

        self.mtime = mtime
        self.date_time = time.gmtime(mtime)[:6]

    def addfile(self, name, mode, islink, data):
        i = zipfile.ZipInfo(name, self.date_time)
        i.compress_type = self.z.compression
        # unzip will not honor unix file modes unless file creator is
        # set to unix (id 3).
        i.create_system = 3
        ftype = _UNX_IFREG
        if islink:
            mode = 0o777
            ftype = _UNX_IFLNK
        i.external_attr = (mode | ftype) << 16
        # add "extended-timestamp" extra block, because zip archives
        # without this will be extracted with unexpected timestamp,
        # if TZ is not configured as GMT
        i.extra += struct.pack(
            "<hhBl",
            0x5455,  # block type: "extended-timestamp"
            1 + 4,  # size of this block
            1,  # "modification time is present"
            int(self.mtime),
        )  # last modification (UTC)
        self.z.writestr(i, data)

    def done(self):
        self.z.close()


class fileit:
    """write archive as files in directory."""

    def __init__(self, name, mtime):
        self.basedir = name
        self.opener = vfsmod.vfs(self.basedir)
        self.mtime = mtime

    def addfile(self, name, mode, islink, data):
        if islink:
            self.opener.symlink(data, name)
            return
        f = self.opener(name, "w", atomictemp=False)
        f.write(data)
        f.close()
        destfile = os.path.join(self.basedir, name)
        os.chmod(destfile, mode)
        if self.mtime is not None:
            os.utime(destfile, (self.mtime, self.mtime))

    def done(self):
        pass


archivers = {
    "files": fileit,
    "tar": tarit,
    "tbz2": lambda name, mtime: tarit(name, mtime, "bz2"),
    "tgz": lambda name, mtime: tarit(name, mtime, "gz"),
    "uzip": lambda name, mtime: zipit(name, mtime, False),
    "zip": zipit,
}


def archive(repo, dest, node, kind, matchfn=None, prefix="", mtime=None):
    """create archive of repo as it was at node.

    dest can be name of directory, name of archive file, or file
    object to write archive to.

    kind is type of archive to create.

    matchfn is function to filter names of files to write to archive.

    prefix is name of path to put before every archive member.

    mtime is the modified time, in seconds, or None to use the changeset time.
    """

    if kind == "files":
        if prefix:
            raise error.Abort(_("cannot give prefix when archiving to files"))
    else:
        prefix = tidyprefix(dest, kind, prefix)

    def write(name, mode, islink, getdata):
        data = getdata()
        archiver.addfile(prefix + name, mode, islink, data)

    if kind not in archivers:
        raise error.Abort(_("unknown archive type '%s'") % kind)

    ctx = repo[node]
    archiver = archivers[kind](dest, mtime or ctx.date()[0])

    if repo.ui.configbool("ui", "archivemeta"):
        name = f"{repo.ui.identity.dotdir()}_archival.txt"
        if not matchfn or matchfn(name):
            write(name, 0o644, False, lambda: buildmetadata(ctx))

    files = computefiles(ctx, matchfn)
    total = len(files)
    if total:
        files.sort()
        with progress.bar(repo.ui, _("archiving"), _("files"), total) as prog:
            for i, f in enumerate(files, 1):
                ff = ctx.flags(f)
                write(f, "x" in ff and 0o755 or 0o644, "l" in ff, ctx[f].data)
                prog.value = (i, f)

    if total == 0:
        raise error.Abort(_("no files match the archive pattern"))

    archiver.done()
    return total


def computefiles(ctx, matchfn):
    if matchfn:
        files = ctx.manifest().matches(matchfn).keys()
    else:
        files = ctx.manifest().keys()
    return files
