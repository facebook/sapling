# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import errno
import os
import time
import traceback

from bindings import revisionstore
from edenscm.mercurial import error, progress, util, vfs
from edenscm.mercurial.i18n import _

from ..extutil import flock
from . import constants, datapack, historypack, shallowutil


class RepackAlreadyRunning(error.Abort):
    pass


def domaintenancerepack(repo):
    """Perform a background repack if necessary."""

    backgroundrepack(repo, incremental=True)


def backgroundrepack(repo, incremental=True):
    cmd = [util.hgexecutable(), "-R", repo.origroot, "repack"]
    msg = _("(running background repack)\n")
    if incremental:
        cmd.append("--incremental")
        msg = _("(running background incremental repack)\n")

    if not repo.ui.quiet:
        repo.ui.write_err(msg)
    util.spawndetached(cmd)


def _runrustrepack(ui, packpath, stores, incremental, shared):
    if not os.path.isdir(packpath):
        return

    try:
        revisionstore.repack(packpath, stores, not incremental, shared, ui._rcfg)
    except Exception as e:
        ui.log("repack_failure", msg=str(e), traceback=traceback.format_exc())
        if "Repack successful but with errors" not in str(e):
            raise


def runrepacklegacy(ui, packpath, incremental, shared):
    _runrustrepack(ui, packpath, None, incremental, shared)


def _getstores(repo, category):
    if category == constants.FILEPACK_CATEGORY:
        return (repo.fileslog.contentstore, repo.fileslog.metadatastore)
    elif category == constants.TREEPACK_CATEGORY:
        return (repo.manifestlog.datastore, repo.manifestlog.historystore)
    raise error.ProgrammingError("invalid pack category")


def _shareddatastoresrepack(repo, incremental, category):
    packpath = shallowutil.getcachepackpath(repo, category)
    limit = repo.ui.configbytes("remotefilelog", "cachelimit", "10GB")
    _cleanuppacks(repo.ui, packpath, limit)

    _runrustrepack(repo.ui, packpath, _getstores(repo, category), incremental, True)


def _localdatarepack(repo, incremental, category):
    if repo.ui.configbool("remotefilelog", "localdatarepack"):
        packpath = shallowutil.getlocalpackpath(repo.svfs.vfs.base, category)
        _cleanuppacks(repo.ui, packpath, 0)

        _runrustrepack(
            repo.ui, packpath, _getstores(repo, category), incremental, False
        )


def fulllocaldatarepack(repo, stores):
    if repo.ui.configbool("remotefilelog", "localdatarepack"):
        packpath = shallowutil.getlocalpackpath(
            repo.svfs.vfs.base, constants.FILEPACK_CATEGORY
        )
        _cleanuppacks(repo.ui, packpath, 0)

        _runrustrepack(repo.ui, packpath, stores, False, False)


def _manifestrepack(repo, incremental):
    if repo.ui.configbool("treemanifest", "server"):
        # This code path is no longer used. Will be deleted soon.
        pass
    elif util.safehasattr(repo.manifestlog, "datastore"):
        if repo.ui.configbool("treemanifest", "useruststore"):
            # Shared
            _shareddatastoresrepack(repo, incremental, constants.TREEPACK_CATEGORY)

            # Local
            _localdatarepack(repo, incremental, constants.TREEPACK_CATEGORY)
        else:
            localdata, shareddata = _getmanifeststores(repo)
            lpackpath, ldstores, lhstores = localdata
            spackpath, sdstores, shstores = shareddata

            def _domanifestrepack(packpath, dstores, hstores, shared):
                limit = (
                    repo.ui.configbytes("remotefilelog", "manifestlimit", "2GB")
                    if shared
                    else 0
                )
                _cleanuppacks(repo.ui, packpath, limit)
                runrepacklegacy(repo.ui, packpath, incremental, shared)

            # Repack the shared manifest store
            _domanifestrepack(spackpath, sdstores, shstores, True)

            # Repack the local manifest store
            _domanifestrepack(lpackpath, ldstores, lhstores, False)


def _dorepack(repo, incremental):
    try:
        mask = os.umask(0o002)
        with flock(
            repacklockvfs(repo).join("repacklock"),
            _("repacking %s") % repo.origroot,
            timeout=0,
        ):
            repo.hook("prerepack")

            _shareddatastoresrepack(repo, incremental, constants.FILEPACK_CATEGORY)
            _localdatarepack(repo, incremental, constants.FILEPACK_CATEGORY)
            _manifestrepack(repo, incremental)
    except error.LockHeld:
        raise RepackAlreadyRunning(
            _("skipping repack - another repack " "is already running")
        )
    finally:
        os.umask(mask)


def fullrepack(repo):
    _dorepack(repo, False)


def incrementalrepack(repo):
    """This repacks the repo by looking at the distribution of pack files in the
    repo and performing the most minimal repack to keep the repo in good shape.
    """
    _dorepack(repo, True)


def _getmanifeststores(repo):
    shareddatastores = repo.manifestlog.shareddatastores
    localdatastores = repo.manifestlog.localdatastores
    sharedhistorystores = repo.manifestlog.sharedhistorystores
    localhistorystores = repo.manifestlog.localhistorystores

    sharedpackpath = shallowutil.getcachepackpath(repo, constants.TREEPACK_CATEGORY)
    localpackpath = shallowutil.getlocalpackpath(
        repo.svfs.vfs.base, constants.TREEPACK_CATEGORY
    )

    return (
        (localpackpath, localdatastores, localhistorystores),
        (sharedpackpath, shareddatastores, sharedhistorystores),
    )


def _cleanuptemppacks(ui, packpath):
    """In some situations, temporary pack files are left around unecessarily
    using disk space. We've even seen cases where some users had 170GB+ worth
    of these. Let's remove these.
    """
    extensions = [
        datapack.PACKSUFFIX,
        datapack.INDEXSUFFIX,
        historypack.PACKSUFFIX,
        historypack.INDEXSUFFIX,
    ]

    def _shouldhold(f):
        """Newish files shouldn't be removed as they could be used by another
        running command.
        """
        if os.path.isdir(f) or os.path.basename(f) == "repacklock":
            return True

        try:
            stat = os.lstat(f)
        except OSError:
            # If we can't access the file, it's either being removed, or we
            # don't have access to it, either way there is nothing we can do
            # about it, ignore them.
            return True
        return time.gmtime(stat.st_atime + 24 * 3600) > time.gmtime()

    with progress.spinner(ui, _("cleaning old temporary files")):
        try:
            for f in os.listdir(packpath):
                f = os.path.join(packpath, f)
                if _shouldhold(f):
                    continue

                __, ext = os.path.splitext(f)

                if ext not in extensions:
                    try:
                        util.unlink(f)
                    except Exception:
                        pass

        except OSError as ex:
            if ex.errno != errno.ENOENT:
                raise


def _cleanupoldpacks(ui, packpath, limit):
    """Enforce a size limit on the cache. Packfiles will be removed oldest
    first, with the asumption that old packfiles contains less useful data than new ones.
    """
    with progress.spinner(ui, _("cleaning old packs")):

        def _mtime(f):
            stat = util.lstat(f)
            return stat.st_mtime

        def _listpackfiles(path):
            packs = []
            try:
                for f in os.listdir(path):
                    _, ext = os.path.splitext(f)
                    if ext.endswith("pack"):
                        packs.append(os.path.join(packpath, f))
            except OSError as ex:
                if ex.errno != errno.ENOENT:
                    raise

            return packs

        files = sorted(_listpackfiles(packpath), key=_mtime, reverse=True)

        cachesize = 0
        for f in files:
            stat = os.lstat(f)
            cachesize += stat.st_size

        while cachesize > limit:
            f = files.pop()
            stat = util.lstat(f)

            # Dont't remove files that are newer than 10 minutes. This will
            # avoid a race condition where mercurial downloads files from the
            # network and expect these to be present on disk. If the 'limit' is
            # properly set, we should have removed enough files that this
            # condition won't matter.
            if time.gmtime(stat.st_mtime + 10 * 60) > time.gmtime():
                return

            root, ext = os.path.splitext(f)
            try:
                if ext == datapack.PACKSUFFIX:
                    util.unlink(root + datapack.INDEXSUFFIX)
                else:
                    util.unlink(root + historypack.INDEXSUFFIX)
            except OSError as ex:
                if ex.errno != errno.ENOENT:
                    raise

            try:
                util.unlink(f)
            except OSError as ex:
                if ex.errno != errno.ENOENT:
                    raise

            cachesize -= stat.st_size


def _cleanuppacks(ui, packpath, limit):
    _cleanuptemppacks(ui, packpath)
    if ui.configbool("remotefilelog", "cleanoldpacks"):
        if limit != 0:
            _cleanupoldpacks(ui, packpath, limit)


def repacklockvfs(repo):
    if util.safehasattr(repo, "name"):
        # Lock in the shared cache so repacks across multiple copies of the same
        # repo are coordinated.
        sharedcachepath = shallowutil.getcachepackpath(
            repo, constants.FILEPACK_CATEGORY
        )
        return vfs.vfs(sharedcachepath)
    else:
        return repo.svfs
