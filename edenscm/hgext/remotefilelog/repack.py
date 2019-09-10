# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import os
import time
import traceback
from contextlib import contextmanager

from edenscm.mercurial import (
    encoding,
    error,
    extensions,
    mdiff,
    policy,
    progress,
    scmutil,
    util,
    vfs,
)
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import nullid, short
from edenscmnative.bindings import revisionstore

from ..extutil import flock, runshellcommand
from . import (
    constants,
    contentstore,
    datapack,
    historypack,
    metadatastore,
    mutablestores,
    shallowutil,
)


try:
    xrange(0)
except NameError:
    xrange = range


class RepackAlreadyRunning(error.Abort):
    pass


def domaintenancerepack(repo):
    """Perform a background repack if necessary.
    """
    packsonly = False

    if repo.ui.configbool("remotefilelog", "packsonlyrepack"):
        packsonly = True

    backgroundrepack(repo, incremental=True, packsonly=packsonly, looseonly=False)


def backgroundrepack(repo, incremental=True, packsonly=False, looseonly=False):
    cmd = [util.hgexecutable(), "-R", repo.origroot, "repack"]
    msg = _("(running background repack)\n")
    if incremental:
        cmd.append("--incremental")
        msg = _("(running background incremental repack)\n")

    if not looseonly and repo.ui.configbool("remotefilelog", "packsonlyrepack"):
        packsonly = True

    if packsonly:
        cmd.append("--packsonly")
    if looseonly:
        cmd.append("--looseonly")

    cmd = " ".join(map(util.shellquote, cmd))

    repo.ui.warn(msg)
    runshellcommand(cmd, encoding.environ)


def _runrustrepack(repo, options, packpath, incremental, pythonrepack):
    # In the case of a loose-only repack, fallback to Python, as Rust doesn't support them.
    if options.get(constants.OPTION_LOOSEONLY):
        return pythonrepack(repo, options, packpath, incremental)

    # Similarly, if a loose+pack repack is requested, let's first run the loose-only Python repack.
    if not options.get(constants.OPTION_PACKSONLY):
        newoptions = dict(options)
        newoptions[constants.OPTION_LOOSEONLY] = True
        pythonrepack(repo, newoptions, packpath, incremental)

    if not os.path.isdir(packpath):
        return

    if incremental:
        repacks = [
            revisionstore.repackincrementaldatapacks,
            revisionstore.repackincrementalhistpacks,
        ]
    else:
        repacks = [revisionstore.repackdatapacks, revisionstore.repackhistpacks]

    for dorepack in repacks:
        try:
            dorepack(packpath, packpath)
        except Exception as e:
            repo.ui.log("repack_failure", msg=str(e), traceback=traceback.format_exc())
            if "Repack successful but with errors" not in str(e):
                raise


def _shareddatastorespythonrepack(repo, options, packpath, incremental):
    if incremental:
        _incrementalrepack(
            repo,
            repo.fileslog.shareddatastores,
            repo.fileslog.sharedhistorystores,
            packpath,
            constants.FILEPACK_CATEGORY,
            options=options,
            shared=True,
        )
    else:
        datasource = contentstore.unioncontentstore(*repo.fileslog.shareddatastores)
        historysource = metadatastore.unionmetadatastore(
            *repo.fileslog.sharedhistorystores, allowincomplete=True
        )

        _runrepack(
            repo,
            datasource,
            historysource,
            packpath,
            constants.FILEPACK_CATEGORY,
            options=options,
            shared=True,
        )


def _shareddatastoresrepack(repo, options, incremental):
    if util.safehasattr(repo.fileslog, "shareddatastores"):
        packpath = shallowutil.getcachepackpath(repo, constants.FILEPACK_CATEGORY)
        limit = repo.ui.configbytes("remotefilelog", "cachelimit", "10GB")
        _cleanuppacks(repo.ui, packpath, limit)

        _runrustrepack(
            repo, options, packpath, incremental, _shareddatastorespythonrepack
        )


def _localdatapythonrepack(repo, options, packpath, incremental):
    if incremental:
        # Always do a full repack of the local loosefiles.
        options = dict(options)
        options["incremental"] = False

    datasource = contentstore.unioncontentstore(*repo.fileslog.localdatastores)
    historysource = metadatastore.unionmetadatastore(
        *repo.fileslog.localhistorystores, allowincomplete=True
    )
    _runrepack(
        repo,
        datasource,
        historysource,
        packpath,
        constants.FILEPACK_CATEGORY,
        options=options,
        shared=False,
    )


def _localdatarepack(repo, options, incremental):
    if repo.ui.configbool("remotefilelog", "localdatarepack") and util.safehasattr(
        repo.fileslog, "localdatastores"
    ):
        packpath = shallowutil.getlocalpackpath(
            repo.svfs.vfs.base, constants.FILEPACK_CATEGORY
        )
        _cleanuppacks(repo.ui, packpath, 0)

        _runrustrepack(repo, options, packpath, incremental, _localdatapythonrepack)


def _manifestpythonrepack(
    repo, options, packpath, dstores, hstores, incremental, shared
):
    if incremental:
        _incrementalrepack(
            repo,
            dstores,
            hstores,
            packpath,
            constants.TREEPACK_CATEGORY,
            options=options,
            shared=shared,
        )
    else:
        datasource = contentstore.unioncontentstore(*dstores)
        historysource = metadatastore.unionmetadatastore(*hstores, allowincomplete=True)
        _runrepack(
            repo,
            datasource,
            historysource,
            packpath,
            constants.TREEPACK_CATEGORY,
            options=options,
            shared=shared,
        )


def _manifestrepack(repo, options, incremental):
    if repo.ui.configbool("treemanifest", "server"):
        treemfmod = extensions.find("treemanifest")
        _runrustrepack(
            repo,
            options,
            repo.localvfs.join("cache/packs/manifests"),
            incremental,
            lambda repo, options, packpath, incremental: treemfmod.serverrepack(
                repo, options=options, incremental=incremental
            ),
        )
    elif util.safehasattr(repo.manifestlog, "datastore"):
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
            _runrustrepack(
                repo,
                options,
                packpath,
                incremental,
                lambda repo, options, packpath, incremental: _manifestpythonrepack(
                    repo, options, packpath, dstores, hstores, incremental, shared
                ),
            )

        # Repack the shared manifest store
        _domanifestrepack(spackpath, sdstores, shstores, True)

        # Repack the local manifest store
        _domanifestrepack(lpackpath, ldstores, lhstores, False)


def _dorepack(repo, options, incremental):
    if options is None:
        options = {}

    options["incremental"] = incremental

    try:
        mask = os.umask(0o002)
        with flock(
            repacklockvfs(repo).join("repacklock"),
            _("repacking %s") % repo.origroot,
            timeout=0,
        ):
            repo.hook("prerepack")

            _shareddatastoresrepack(repo, options, incremental)
            _localdatarepack(repo, options, incremental)
            _manifestrepack(repo, options, incremental)
    except error.LockHeld:
        raise RepackAlreadyRunning(
            _("skipping repack - another repack " "is already running")
        )
    finally:
        os.umask(mask)


def fullrepack(repo, options=None):
    _dorepack(repo, options, False)


def incrementalrepack(repo, options=None):
    """This repacks the repo by looking at the distribution of pack files in the
    repo and performing the most minimal repack to keep the repo in good shape.
    """
    _dorepack(repo, options, True)


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


def _topacks(packpath, files, constructor):
    paths = list(os.path.join(packpath, p) for p in files)
    packs = list(constructor(p) for p in paths)
    return packs


def _deletebigpacks(repo, folder, files):
    """Deletes packfiles that are bigger than ``packs.maxpacksize``.

    Returns ``files` with the removed files omitted."""
    maxsize = repo.ui.configbytes("packs", "maxpacksize")
    if maxsize <= 0:
        return files

    # This only considers datapacks today, but we could broaden it to include
    # historypacks.
    VALIDEXTS = [".datapack", ".dataidx"]

    # Either an oversize index or datapack will trigger cleanup of the whole
    # pack:
    oversized = set(
        [
            os.path.splitext(path)[0]
            for path, ftype, stat in files
            if (stat.st_size > maxsize and (os.path.splitext(path)[1] in VALIDEXTS))
        ]
    )

    for rootfname in oversized:
        rootpath = os.path.join(folder, rootfname)
        for ext in VALIDEXTS:
            path = rootpath + ext
            repo.ui.debug(
                "removing oversize packfile %s (%s)\n"
                % (path, util.bytecount(os.stat(path).st_size))
            )
            os.unlink(path)
    return [row for row in files if os.path.basename(row[0]) not in oversized]


def _incrementalrepack(
    repo,
    datastore,
    historystore,
    packpath,
    category,
    allowincompletedata=False,
    options=None,
    shared=False,
):
    shallowutil.mkstickygroupdir(repo.ui, packpath)

    files = util.listdir(packpath, stat=True)
    if shared:
        files = _deletebigpacks(repo, packpath, files)
    datapacks = _topacks(
        packpath, _computeincrementaldatapack(repo.ui, files), revisionstore.datapack
    )
    datapacks.extend(
        s
        for s in datastore
        if not (
            isinstance(s, datapack.datapackstore)
            or isinstance(s, revisionstore.datapackstore)
        )
    )

    historypacks = _topacks(
        packpath,
        _computeincrementalhistorypack(repo.ui, files),
        revisionstore.historypack,
    )
    historypacks.extend(
        s
        for s in historystore
        if not (
            isinstance(s, historypack.historypackstore)
            or isinstance(s, revisionstore.historypackstore)
        )
    )

    # ``allhistory{files,packs}`` contains all known history packs, even ones we
    # don't plan to repack. They are used during the datapack repack to ensure
    # good ordering of nodes.
    allhistoryfiles = _allpackfileswithsuffix(
        files, historypack.PACKSUFFIX, historypack.INDEXSUFFIX
    )
    allhistorypacks = _topacks(
        packpath, (f for f, mode, stat in allhistoryfiles), revisionstore.historypack
    )
    allhistorypacks.extend(
        s for s in historystore if not isinstance(s, historypack.historypackstore)
    )
    _runrepack(
        repo,
        contentstore.unioncontentstore(*datapacks, allowincomplete=allowincompletedata),
        metadatastore.unionmetadatastore(*historypacks, allowincomplete=True),
        packpath,
        category,
        fullhistory=metadatastore.unionmetadatastore(
            *allhistorypacks, allowincomplete=True
        ),
        options=options,
        shared=shared,
    )


def _computeincrementaldatapack(ui, files):
    opts = {
        "gencountlimit": ui.configint("remotefilelog", "data.gencountlimit", 2),
        "generations": ui.configlist(
            "remotefilelog", "data.generations", ["1GB", "100MB", "1MB"]
        ),
        "maxrepackpacks": ui.configint("remotefilelog", "data.maxrepackpacks", 50),
        "repackmaxpacksize": ui.configbytes(
            "remotefilelog", "data.repackmaxpacksize", "4GB"
        ),
        "repacksizelimit": ui.configbytes(
            "remotefilelog", "data.repacksizelimit", "100MB"
        ),
    }

    packfiles = _allpackfileswithsuffix(
        files, datapack.PACKSUFFIX, datapack.INDEXSUFFIX
    )
    return _computeincrementalpack(packfiles, opts)


def _computeincrementalhistorypack(ui, files):
    opts = {
        "gencountlimit": ui.configint("remotefilelog", "history.gencountlimit", 2),
        "generations": ui.configlist("remotefilelog", "history.generations", ["100MB"]),
        "maxrepackpacks": ui.configint("remotefilelog", "history.maxrepackpacks", 50),
        "repackmaxpacksize": ui.configbytes(
            "remotefilelog", "history.repackmaxpacksize", "400MB"
        ),
        "repacksizelimit": ui.configbytes(
            "remotefilelog", "history.repacksizelimit", "100MB"
        ),
    }

    packfiles = _allpackfileswithsuffix(
        files, historypack.PACKSUFFIX, historypack.INDEXSUFFIX
    )
    return _computeincrementalpack(packfiles, opts)


def _allpackfileswithsuffix(files, packsuffix, indexsuffix):
    result = []
    fileset = set(fn for fn, mode, stat in files)
    for filename, mode, stat in files:
        if not filename.endswith(packsuffix):
            continue

        prefix = filename[: -len(packsuffix)]

        # Don't process a pack if it doesn't have an index.
        if (prefix + indexsuffix) not in fileset:
            continue
        result.append((prefix, mode, stat))

    return result


def _computeincrementalpack(files, opts):
    """Given a set of pack files along with the configuration options, this
    function computes the list of files that should be packed as part of an
    incremental repack.

    It tries to strike a balance between keeping incremental repacks cheap (i.e.
    packing small things when possible, and rolling the packs up to the big ones
    over time).
    """

    limits = list(
        sorted((util.sizetoint(s) for s in opts["generations"]), reverse=True)
    )
    limits.append(0)

    # Group the packs by generation (i.e. by size)
    generations = []
    for i in xrange(len(limits)):
        generations.append([])

    sizes = {}
    for prefix, mode, stat in files:
        size = stat.st_size
        if size > opts["repackmaxpacksize"]:
            continue

        sizes[prefix] = size
        for i, limit in enumerate(limits):
            if size > limit:
                generations[i].append(prefix)
                break

    # Steps for picking what packs to repack:
    # 1. Pick the largest generation with > gencountlimit pack files.
    # 2. Take the smallest three packs.
    # 3. While total-size-of-packs < repacksizelimit: add another pack

    # Find the largest generation with more than gencountlimit packs
    genpacks = []
    for i, limit in enumerate(limits):
        if len(generations[i]) > opts["gencountlimit"]:
            # Sort to be smallest last, for easy popping later
            genpacks.extend(
                sorted(generations[i], reverse=True, key=lambda x: sizes[x])
            )
            break

    # Take as many packs from the generation as we can
    chosenpacks = genpacks[-3:]
    genpacks = genpacks[:-3]
    repacksize = sum(sizes[n] for n in chosenpacks)
    while (
        repacksize < opts["repacksizelimit"]
        and genpacks
        and len(chosenpacks) < opts["maxrepackpacks"]
    ):
        chosenpacks.append(genpacks.pop())
        repacksize += sizes[chosenpacks[-1]]

    return chosenpacks


def _runrepack(
    repo,
    data,
    history,
    packpath,
    category,
    fullhistory=None,
    options=None,
    shared=False,
):
    shallowutil.mkstickygroupdir(repo.ui, packpath)

    def isold(repo, filename, node):
        """Check if the file node is older than a limit.
        Unless a limit is specified in the config the default limit is taken.
        """
        filectx = repo.filectx(filename, fileid=node)
        filetime = repo[filectx.linkrev()].date()

        # Currently default TTL limit is 30 days
        defaultlimit = 60 * 60 * 24 * 30
        ttl = repo.ui.configint("remotefilelog", "nodettl", defaultlimit)

        limit = time.time() - ttl
        return filetime[0] < limit

    garbagecollect = repo.ui.configbool("remotefilelog", "gcrepack")
    if not fullhistory:
        fullhistory = history
    packer = repacker(
        repo,
        data,
        history,
        fullhistory,
        category,
        packpath,
        gc=garbagecollect,
        isold=isold,
        options=options,
        shared=shared,
    )

    with mutablestores.mutabledatastore(repo, packpath) as dpack:
        with mutablestores.mutablehistorystore(repo, packpath) as hpack:
            packer.run(packpath, dpack, hpack)


def keepset(repo, keyfn, lastkeepkeys=None):
    """Computes a keepset which is not garbage collected.
    'keyfn' is a function that maps filename, node to a unique key.
    'lastkeepkeys' is an optional argument and if provided the keepset
    function updates lastkeepkeys with more keys and returns the result.
    """
    if not lastkeepkeys:
        keepkeys = set()
    else:
        keepkeys = lastkeepkeys

    # We want to keep:
    # 1. Working copy parent
    # 2. Draft commits
    # 3. Parents of draft commits
    # 4. Pullprefetch and bgprefetchrevs revsets if specified
    revs = [".", "draft()", "parents(draft())"]
    prefetchrevs = repo.ui.config("remotefilelog", "pullprefetch", None)
    if prefetchrevs:
        revs.append("(%s)" % prefetchrevs)
    prefetchrevs = repo.ui.config("remotefilelog", "bgprefetchrevs", None)
    if prefetchrevs:
        revs.append("(%s)" % prefetchrevs)
    revs = "+".join(revs)

    revs = ['sort((%s), "topo")' % revs]
    keep = scmutil.revrange(repo, revs)

    processed = set()
    lastmanifest = None

    # process the commits in toposorted order starting from the oldest
    for r in reversed(keep._list):
        ctx = repo[r]
        if ctx.p1().rev() in processed:
            # if the direct parent has already been processed
            # then we only need to process the delta
            m = ctx.manifest().diff(ctx.p1().manifest())
        else:
            # otherwise take the manifest and diff it
            # with the previous manifest if one exists
            if lastmanifest:
                m = ctx.manifest().diff(lastmanifest)
            else:
                m = ctx.manifest()
        lastmanifest = ctx.manifest()
        processed.add(r)

        # populate keepkeys with keys from the current manifest
        if type(m) is dict:
            # m is a result of diff of two manifests and is a dictionary that
            # maps filename to ((newnode, newflag), (oldnode, oldflag)) tuple
            for filename, diff in m.iteritems():
                if diff[0][0] is not None:
                    keepkeys.add(keyfn(filename, diff[0][0]))
        else:
            # m is a manifest object
            for filename, filenode in m.iteritems():
                keepkeys.add(keyfn(filename, filenode))

    return keepkeys


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

        stat = os.lstat(f)
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
            stat = os.lstat(f)
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
            stat = os.lstat(f)

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


class repacker(object):
    """Class for orchestrating the repack of data and history information into a
    new format.
    """

    def __init__(
        self,
        repo,
        data,
        history,
        fullhistory,
        category,
        packpath,
        gc=False,
        isold=None,
        options=None,
        shared=False,
    ):
        self.repo = repo
        self.data = data
        self.history = history
        self.fullhistory = fullhistory
        self.unit = constants.getunits(category)
        self.garbagecollect = gc
        self.options = options
        self.sharedstr = _("shared") if shared else _("local")
        self.packpath = packpath
        if self.garbagecollect:
            if not isold:
                raise ValueError("Function 'isold' is not properly specified")
            # use (filename, node) tuple as a keepset key
            self.keepkeys = keepset(repo, lambda f, n: (f, n))
            self.isold = isold

    def _runpythonrepack(self, ledger, packpath, targetdata, targethistory, options):
        # Populate ledger from source
        with progress.spinner(
            self.repo.ui,
            _("scanning for %s %s to repack") % (self.sharedstr, self.unit),
        ) as prog:
            ledger.prog = prog
            self.data.markledger(ledger, options=options)
            self.history.markledger(ledger, options=options)
            ledger.prog = None

        # Run repack
        self.repackdata(ledger, targetdata)
        self.repackhistory(ledger, targethistory)

        # Flush renames in the directory
        util.syncdir(packpath)

        # Call cleanup on each non-corrupt source
        for source in ledger.sources:
            if source not in ledger.corruptsources:
                source.cleanup(ledger)

        # Call other cleanup functions
        for cleanup in ledger.cleanup:
            cleanup(self.repo.ui)

    def run(self, packpath, targetdata, targethistory):
        ledger = repackledger()

        self._runpythonrepack(ledger, packpath, targetdata, targethistory, self.options)

    def _chainorphans(self, ui, filename, nodes, orphans, deltabases):
        """Reorderes ``orphans`` into a single chain inside ``nodes`` and
        ``deltabases``.

        We often have orphan entries (nodes without a base that aren't
        referenced by other nodes -- i.e., part of a chain) due to gaps in
        history. Rather than store them as individual fulltexts, we prefer to
        insert them as one chain sorted by size.
        """
        if not orphans:
            return nodes

        def getsize(node, default=0):
            meta = self.data.getmeta(filename, node)
            if constants.METAKEYSIZE in meta:
                return meta[constants.METAKEYSIZE]
            else:
                return default

        # Sort orphans by size; biggest first is preferred, since it's more
        # likely to be the newest version assuming files grow over time.
        # (Sort by node first to ensure the sort is stable.)
        orphans = sorted(orphans)
        orphans = list(sorted(orphans, key=getsize, reverse=True))
        if ui.debugflag:
            ui.debug(
                "%s: orphan chain: %s\n"
                % (filename, ", ".join([short(s) for s in orphans]))
            )

        # Create one contiguous chain and reassign deltabases.
        for i, node in enumerate(orphans):
            if i == 0:
                deltabases[node] = (nullid, 0)
            else:
                parent = orphans[i - 1]
                deltabases[node] = (parent, deltabases[parent][1] + 1)
        nodes = filter(lambda node: node not in orphans, nodes)
        nodes += orphans
        return nodes

    def repackdata(self, ledger, target):
        ui = self.repo.ui
        maxchainlen = ui.configint("packs", "maxchainlen", 1000)

        byfile = {}
        for entry in ledger.entries.itervalues():
            if entry.datasource:
                byfile.setdefault(entry.filename, {})[entry.node] = entry

        with progress.bar(
            ui,
            _("repacking data for %s %s") % (self.sharedstr, self.unit),
            self.unit,
            total=len(byfile),
        ) as prog:
            for filename, entries in sorted(byfile.iteritems()):
                ancestors = {}
                nodes = list(node for node in entries.iterkeys())
                nohistory = []
                with progress.bar(
                    ui, _("building history"), "nodes", total=len(nodes)
                ) as historyprog:
                    for i, node in enumerate(nodes):
                        if node in ancestors:
                            continue
                        historyprog.value = i
                        try:
                            ancestors.update(
                                self.fullhistory.getancestors(
                                    filename, node, known=ancestors
                                )
                            )
                        except KeyError:
                            # Since we're packing data entries, we may not have
                            # the corresponding history entries for them. It's
                            # not a big deal, but the entries won't be delta'd
                            # perfectly.
                            nohistory.append(node)

                # Order the nodes children first, so we can produce reverse
                # deltas
                orderednodes = list(reversed(self._toposort(ancestors)))
                if len(nohistory) > 0:
                    ui.debug("repackdata: %d nodes without history\n" % len(nohistory))
                orderednodes.extend(sorted(nohistory))

                # Filter orderednodes to just the nodes we want to serialize (it
                # currently also has the edge nodes' ancestors).
                orderednodes = filter(lambda node: node in nodes, orderednodes)

                # Garbage collect old nodes:
                if self.garbagecollect:
                    neworderednodes = []
                    for node in orderednodes:
                        # If the node is old and is not in the keepset, we skip
                        # it, and mark as garbage collected
                        if (filename, node) not in self.keepkeys and self.isold(
                            self.repo, filename, node
                        ):
                            entries[node].gced = True
                            continue
                        neworderednodes.append(node)
                    orderednodes = neworderednodes

                # Compute delta bases for nodes:
                deltabases = {}
                nobase = set()
                referenced = set()
                nodes = set(nodes)
                with progress.bar(
                    ui, _("processing nodes"), "nodes", len(orderednodes)
                ) as nodeprog:
                    for i, node in enumerate(orderednodes):
                        nodeprog.value = i
                        # Find delta base
                        # TODO: allow delta'ing against most recent descendant
                        # instead of immediate child
                        deltatuple = deltabases.get(node, None)
                        if deltatuple is None:
                            deltabase, chainlen = nullid, 0
                            deltabases[node] = (nullid, 0)
                            nobase.add(node)
                        else:
                            deltabase, chainlen = deltatuple
                            referenced.add(deltabase)

                        # Use available ancestor information to inform our delta
                        # choices
                        ancestorinfo = ancestors.get(node)
                        if ancestorinfo:
                            p1, p2, linknode, copyfrom = ancestorinfo

                            # The presence of copyfrom means we're at a point
                            # where the file was copied from elsewhere. So don't
                            # attempt to do any deltas with the other file.
                            if copyfrom:
                                p1 = nullid

                            if chainlen < maxchainlen:
                                # Record this child as the delta base for its
                                # parents. This may be non optimal, since the
                                # parents may have many children, and this will
                                # only choose the last one.
                                # TODO: record all children and try all deltas
                                # to find best
                                if p1 != nullid:
                                    deltabases[p1] = (node, chainlen + 1)
                                if p2 != nullid:
                                    deltabases[p2] = (node, chainlen + 1)

                # experimental config: repack.chainorphansbysize
                if ui.configbool("repack", "chainorphansbysize", True):
                    orphans = nobase - referenced
                    orderednodes = self._chainorphans(
                        ui, filename, orderednodes, orphans, deltabases
                    )

                # Compute deltas and write to the pack
                for i, node in enumerate(orderednodes):
                    deltabase, chainlen = deltabases[node]
                    # Compute delta
                    # TODO: Optimize the deltachain fetching. Since we're
                    # iterating over the different version of the file, we may
                    # be fetching the same deltachain over and over again.
                    meta = None
                    if deltabase != nullid:
                        deltaentry = self.data.getdelta(filename, node)
                        delta, deltabasename, origdeltabase, meta = deltaentry
                        size = meta.get(constants.METAKEYSIZE)
                        if (
                            deltabasename != filename
                            or origdeltabase != deltabase
                            or size is None
                        ):
                            deltabasetext = self.data.get(filename, deltabase)
                            original = self.data.get(filename, node)
                            size = len(original)
                            delta = mdiff.textdiff(deltabasetext, original)
                    else:
                        delta = self.data.get(filename, node)
                        size = len(delta)
                        meta = self.data.getmeta(filename, node)

                    # TODO: don't use the delta if it's larger than the fulltext
                    if constants.METAKEYSIZE not in meta:
                        meta[constants.METAKEYSIZE] = size
                    target.add(filename, node, deltabase, delta, meta)

                    entries[node].datarepacked = True

                prog.value += 1

        ledger.addcreated(target.flush())

    def repackhistory(self, ledger, target):
        ui = self.repo.ui

        byfile = {}
        for entry in ledger.entries.itervalues():
            if entry.historysource:
                byfile.setdefault(entry.filename, {})[entry.node] = entry

        with progress.bar(
            ui,
            _("repacking history for %s %s") % (self.sharedstr, self.unit),
            self.unit,
            len(byfile),
        ) as prog:
            for filename, entries in sorted(byfile.iteritems()):
                ancestors = {}
                nodes = list(node for node in entries.iterkeys())

                for node in nodes:
                    if node in ancestors:
                        continue
                    ancestors.update(
                        self.history.getancestors(filename, node, known=ancestors)
                    )

                # Order the nodes children first
                orderednodes = reversed(self._toposort(ancestors))

                # Write to the pack
                dontprocess = set()
                for node in orderednodes:
                    p1, p2, linknode, copyfrom = ancestors[node]

                    # If the node is marked dontprocess, but it's also in the
                    # explicit entries set, that means the node exists both in
                    # this file and in another file that was copied to this
                    # file. Usually this happens if the file was copied to
                    # another file, then the copy was deleted, then reintroduced
                    # without copy metadata. The original add and the new add
                    # have the same hash since the content is identical and the
                    # parents are null.
                    if node in dontprocess and node not in entries:
                        # If copyfrom == filename, it means the copy history
                        # went to come other file, then came back to this one,
                        # so we should continue processing it.
                        if p1 != nullid and copyfrom != filename:
                            dontprocess.add(p1)
                        if p2 != nullid:
                            dontprocess.add(p2)
                        continue

                    if copyfrom:
                        dontprocess.add(p1)

                    target.add(filename, node, p1, p2, linknode, copyfrom)

                    if node in entries:
                        entries[node].historyrepacked = True

                prog.value += 1

        ledger.addcreated(target.flush())

    def _toposort(self, ancestors):
        def parentfunc(node):
            p1, p2, linknode, copyfrom = ancestors[node]
            parents = []
            if p1 != nullid:
                parents.append(p1)
            if p2 != nullid:
                parents.append(p2)
            return parents

        sortednodes = shallowutil.sortnodes(ancestors.keys(), parentfunc)
        return sortednodes


class repackledger(object):
    """Storage for all the bookkeeping that happens during a repack. It contains
    the list of revisions being repacked, what happened to each revision, and
    which source store contained which revision originally (for later cleanup).
    """

    def __init__(self):
        self.entries = {}
        self.sources = {}
        self.corruptsources = set()
        self.cleanup = []
        self.created = set()
        self.prog = None

    def markdataentry(self, source, filename, node):
        """Mark the given filename+node revision as having a data rev in the
        given source.
        """
        entry = self._getorcreateentry(filename, node)
        entry.datasource = True
        entries = self.sources.get(source)
        if not entries:
            entries = set()
            self.sources[source] = entries
        entries.add(entry)

    def markhistoryentry(self, source, filename, node):
        """Mark the given filename+node revision as having a history rev in the
        given source.
        """
        entry = self._getorcreateentry(filename, node)
        entry.historysource = True
        entries = self.sources.get(source)
        if not entries:
            entries = set()
            self.sources[source] = entries
        entries.add(entry)

    def markcorruptsource(self, source):
        self.corruptsources.add(source)

    def addcleanup(self, cleanup):
        self.cleanup.append(cleanup)

    def _getorcreateentry(self, filename, node):
        key = (filename, node)
        value = self.entries.get(key)
        if not value:
            value = repackentry(filename, node)
            self.entries[key] = value

        return value

    def addcreated(self, value):
        if value is not None:
            self.created.add(value)

    def setlocation(self, location=None):
        if self.prog is not None:
            if location is not None:
                self.prog.value = None, location
            else:
                self.prog.value = None

    @contextmanager
    def location(self, location):
        self.setlocation(location)
        yield
        self.setlocation()


class repackentry(object):
    """Simple class representing a single revision entry in the repackledger.
    """

    __slots__ = [
        "filename",
        "node",
        "datasource",
        "historysource",
        "datarepacked",
        "historyrepacked",
        "gced",
    ]

    def __init__(self, filename, node):
        self.filename = filename
        self.node = node
        # If the revision has a data entry in the source
        self.datasource = False
        # If the revision has a history entry in the source
        self.historysource = False
        # If the revision's data entry was repacked into the repack target
        self.datarepacked = False
        # If the revision's history entry was repacked into the repack target
        self.historyrepacked = False
        # If garbage collected
        self.gced = False


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
