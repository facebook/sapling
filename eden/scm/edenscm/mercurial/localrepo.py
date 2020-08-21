# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# localrepo.py - read/write repository class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import hashlib
import os
import random
import time
import weakref
from contextlib import contextmanager

import bindings
from edenscm.hgext.extlib.phabricator import diffprops

from . import (
    bookmarks,
    branchmap,
    bundle2,
    changegroup,
    changelog,
    changelog2,
    color,
    connectionpool,
    context,
    dirstate,
    dirstateguard,
    discovery,
    edenfs,
    encoding,
    error as errormod,
    exchange,
    extensions,
    filelog,
    hook,
    lock as lockmod,
    manifest,
    match as matchmod,
    merge as mergemod,
    mergeutil,
    mutation,
    namespaces,
    obsolete,
    pathutil,
    peer,
    phases,
    progress,
    pushkey,
    pycompat,
    repository,
    revlog,
    revset,
    revsetlang,
    scmutil,
    smallcommitmetadata,
    store,
    transaction,
    treestate,
    txnutil,
    uiconfig,
    util,
    vfs as vfsmod,
    visibility,
)
from .i18n import _
from .node import bin, hex, nullhex, nullid, short
from .pycompat import range


release = lockmod.release
urlerr = util.urlerr
urlreq = util.urlreq

# set of (path, vfs-location) tuples. vfs-location is:
# - 'plain for vfs relative paths
# - '' for svfs relative paths
_cachedfiles = set()


class _basefilecache(scmutil.filecache):
    """filecache usage on repo"""

    def __get__(self, repo, type=None):
        if repo is None:
            return self
        return super(_basefilecache, self).__get__(repo, type)

    def __set__(self, repo, value):
        return super(_basefilecache, self).__set__(repo, value)

    def __delete__(self, repo):
        return super(_basefilecache, self).__delete__(repo)


class repofilecache(_basefilecache):
    """filecache for files in .hg but outside of .hg/store"""

    def __init__(self, localpaths=(), sharedpaths=()):
        paths = [(path, self.localjoin) for path in localpaths] + [
            (path, self.sharedjoin) for path in sharedpaths
        ]
        super(repofilecache, self).__init__(*paths)
        for path in localpaths:
            _cachedfiles.add((path, "local"))
        for path in sharedpaths:
            _cachedfiles.add((path, "shared"))

    def localjoin(self, obj, fname):
        return obj.localvfs.join(fname)

    def sharedjoin(self, obj, fname):
        return obj.sharedvfs.join(fname)


class storecache(_basefilecache):
    """filecache for files in the store"""

    def __init__(self, *paths):
        super(storecache, self).__init__(*paths)
        for path in paths:
            _cachedfiles.add((path, ""))

    def join(self, obj, fname):
        return obj.sjoin(fname)


def isfilecached(repo, name):
    """check if a repo has already cached "name" filecache-ed property

    This returns (cachedobj-or-None, iscached) tuple.
    """
    cacheentry = repo._filecache.get(name, None)
    if not cacheentry:
        return None, False
    return cacheentry.obj, True


def hascache(repo, name):
    """check if a repo has an value for <name>"""
    return name in vars(repo)


moderncaps = {"lookup", "branchmap", "pushkey", "known", "getbundle", "unbundle"}
legacycaps = moderncaps.union({"changegroupsubset"})


class localpeer(repository.peer):
    """peer for a local repo; reflects only the most recent API"""

    def __init__(self, repo, caps=None):
        super(localpeer, self).__init__()

        if caps is None:
            caps = moderncaps.copy()
        self._repo = repo
        self._ui = repo.ui
        self._caps = repo._restrictcapabilities(caps)

    # Begin of _basepeer interface.

    @util.propertycache
    def ui(self):
        return self._ui

    def url(self):
        return self._repo.url()

    def local(self):
        return self._repo

    def peer(self):
        return self

    def canpush(self):
        return True

    def close(self):
        self._repo.close()

    # End of _basepeer interface.

    # Begin of _basewirecommands interface.

    def branchmap(self):
        return self._repo.branchmap()

    def capabilities(self):
        return self._caps

    def debugwireargs(self, one, two, three=None, four=None, five=None):
        """Used to test argument passing over the wire"""
        return "%s %s %s %s %s" % (one, two, three, four, five)

    def getbundle(self, source, heads=None, common=None, bundlecaps=None, **kwargs):
        chunks = exchange.getbundlechunks(
            self._repo,
            source,
            heads=heads,
            common=common,
            bundlecaps=bundlecaps,
            **kwargs
        )
        cb = util.chunkbuffer(chunks)

        if exchange.bundle2requested(bundlecaps):
            # When requesting a bundle2, getbundle returns a stream to make the
            # wire level function happier. We need to build a proper object
            # from it in local peer.
            return bundle2.getunbundler(self.ui, cb)
        else:
            return changegroup.getunbundler("01", cb, None)

    def heads(self, *args, **kwargs):
        return list(self._repo.heads(*args, **kwargs))

    def known(self, nodes):
        return self._repo.known(nodes)

    def listkeys(self, namespace, patterns=None):
        return self._repo.listkeys(namespace, patterns)

    def listkeyspatterns(self, namespace, patterns=None):
        return self._repo.listkeys(namespace, patterns)

    def lookup(self, key):
        return self._repo.lookup(key)

    def pushkey(self, namespace, key, old, new):
        return self._repo.pushkey(namespace, key, old, new)

    def stream_out(self, shallow=False):
        raise errormod.Abort(_("cannot perform stream clone against local " "peer"))

    def unbundle(self, cg, heads, url):
        """apply a bundle on a repo

        This function handles the repo locking itself."""
        try:
            try:
                cg = exchange.readbundle(self.ui, cg, None)
                ret = exchange.unbundle(self._repo, cg, heads, "push", url)
                if util.safehasattr(ret, "getchunks"):
                    # This is a bundle20 object, turn it into an unbundler.
                    # This little dance should be dropped eventually when the
                    # API is finally improved.
                    stream = util.chunkbuffer(ret.getchunks())
                    ret = bundle2.getunbundler(self.ui, stream)
                return ret
            except Exception as exc:
                # If the exception contains output salvaged from a bundle2
                # reply, we need to make sure it is printed before continuing
                # to fail. So we build a bundle2 with such output and consume
                # it directly.
                #
                # This is not very elegant but allows a "simple" solution for
                # issue4594
                output = getattr(exc, "_bundle2salvagedoutput", ())
                if output:
                    bundler = bundle2.bundle20(self._repo.ui)
                    for out in output:
                        bundler.addpart(out)
                    stream = util.chunkbuffer(bundler.getchunks())
                    b = bundle2.getunbundler(self.ui, stream)
                    bundle2.processbundle(self._repo, b)
                raise
        except errormod.PushRaced as exc:
            raise errormod.ResponseError(_("push failed:"), str(exc))

    # End of _basewirecommands interface.

    # Begin of peer interface.

    def iterbatch(self):
        return peer.localiterbatcher(self)

    # End of peer interface.


class locallegacypeer(repository.legacypeer, localpeer):
    """peer extension which implements legacy methods too; used for tests with
    restricted capabilities"""

    def __init__(self, repo):
        super(locallegacypeer, self).__init__(repo, caps=legacycaps)

    # Begin of baselegacywirecommands interface.

    def between(self, pairs):
        return self._repo.between(pairs)

    def branches(self, nodes):
        return self._repo.branches(nodes)

    def changegroup(self, basenodes, source):
        outgoing = discovery.outgoing(
            self._repo, missingroots=basenodes, missingheads=self._repo.heads()
        )
        return changegroup.makechangegroup(self._repo, outgoing, "01", source)

    def changegroupsubset(self, bases, heads, source):
        outgoing = discovery.outgoing(
            self._repo, missingroots=bases, missingheads=heads
        )
        return changegroup.makechangegroup(self._repo, outgoing, "01", source)

    # End of baselegacywirecommands interface.


class localrepository(object):
    """local repository object

    ``unsafe.wvfsauditorcache`` config option allows the user to enable
    the auditor caching for the repo's working copy. This significantly
    reduces the amount of stat-like system calls and thus saves time.
    This option should be safe if symlinks are not used in the repo"""

    supportedformats = {"revlogv1", "generaldelta", "treemanifest"}
    _basesupported = supportedformats | {
        edenfs.requirement,
        "store",
        "fncache",
        "shared",
        "relshared",
        "dotencode",
        "treedirstate",
        "treestate",
        "storerequirements",
    }
    _basestoresupported = {
        "visibleheads",
        "narrowheads",
        "zstorecommitdata",
        "invalidatelinkrev",
        # python revlog
        "pythonrevlogchangelog",
        # rust revlog
        "rustrevlogchangelog",
        # pure segmented changelog (full idmap, full hgommits)
        "segmentedchangelog",
        # segmented changelog (full idmap, partial hgcommits) + revlog
        "doublewritechangelog",
    }
    openerreqs = {"revlogv1", "generaldelta", "treemanifest"}

    # sets of (ui, featureset) functions for repo and store features.
    # only functions defined in module of enabled extensions are invoked
    # pyre-fixme[20]: Argument `expr` expected.
    featuresetupfuncs = set()
    # pyre-fixme[20]: Argument `expr` expected.
    storefeaturesetupfuncs = set()

    # list of prefix for file which can be written without 'wlock'
    # Extensions should extend this list when needed
    _wlockfreeprefix = {
        # We migh consider requiring 'wlock' for the next
        # two, but pretty much all the existing code assume
        # wlock is not needed so we keep them excluded for
        # now.
        "hgrc",
        "requires",
        # XXX cache is a complicatged business someone
        # should investigate this in depth at some point
        "cache/",
        # XXX shouldn't be dirstate covered by the wlock?
        "dirstate",
        # XXX checkoutidentifier has same rules as dirstate.
        "checkoutidentifier",
        # XXX bisect was still a bit too messy at the time
        # this changeset was introduced. Someone should fix
        # the remainig bit and drop this line
        "bisect.state",
        # Race condition to this file is okay.
        "lastsqlsync",
    }

    # Set of prefixes of store files which can be written without 'lock'.
    # Extensions should extend this set when necessary.
    # pyre-fixme[20]: Argument `expr` expected.
    _lockfreeprefix = set()

    def __init__(self, baseui, path, create=False):
        self.requirements = set()
        self.storerequirements = set()
        # wvfs: rooted at the repository root, used to access the working copy
        self.wvfs = vfsmod.vfs(path, expandpath=True, realpath=True, cacheaudited=False)
        # localvfs: rooted at .hg, used to access repo files outside of
        # the store that are local to this working copy.
        self.localvfs = None
        # sharedvfs: the local vfs of the primary shared repo for shared repos.
        # for non-shared repos this is the same as localvfs.
        self.sharedvfs = None
        # svfs: store vfs - usually rooted at .hg/store, used to access repository history
        # If this is a shared repository, this vfs may point to another
        # repository's .hg/store directory.
        self.svfs = None
        self.root = self.wvfs.base
        self.path = self.wvfs.join(".hg")
        self.origroot = path
        # This is only used by context.workingctx.match in order to
        # detect files in forbidden paths.
        self.auditor = pathutil.pathauditor(self.root)
        # This is only used by context.basectx.match in order to detect
        # files in forbidden paths..
        self.nofsauditor = pathutil.pathauditor(self.root, realfs=False, cached=True)
        self.baseui = baseui
        self.ui = baseui.copy()
        self.ui.copy = baseui.copy  # prevent copying repo configuration
        self.localvfs = vfsmod.vfs(self.path, cacheaudited=True)
        if self.ui.configbool("devel", "all-warnings") or self.ui.configbool(
            "devel", "check-locks"
        ):
            self.localvfs.audit = self._getvfsward(self.localvfs.audit)

        # A list of callback to shape the phase if no data were found.
        # Callback are in the form: func(repo, roots) --> processed root.
        # This list it to be filled by extension during repo setup
        self._phasedefaults = []

        reponame = self.ui.config("remotefilelog", "reponame", "")
        # If the repo already exists, load the existing configs
        if self.localvfs.isdir():
            self.ui.reloadconfigs(self.root)
            uiconfig.validatedynamicconfig(self.ui)
        elif create:
            # If we're in the process of creating the repo, load the dynamic configs in
            # memory only. They will be written to disk later once the localvfs
            # is created.
            uiconfig.applydynamicconfig(self.ui, reponame, self.path)

        self._loadextensions()

        cacheaudited = self.ui.configbool("unsafe", "wvfsauditorcache")
        self.wvfs.audit._cached = cacheaudited

        self.supported = self._featuresetup(self.featuresetupfuncs, self._basesupported)
        self.storesupported = self._featuresetup(
            self.storefeaturesetupfuncs, self._basestoresupported
        )
        color.setup(self.ui)

        # Add compression engines.
        for name in util.compengines:
            engine = util.compengines[name]
            if engine.revlogheader():
                self.supported.add("exp-compression-%s" % name)

        created = False
        if not self.localvfs.isdir():
            if create:
                created = True
                self.requirements = newreporequirements(self)
                if "store" in self.requirements:
                    self.storerequirements = newrepostorerequirements(self)

                if not self.wvfs.exists():
                    self.wvfs.makedirs()
                self.localvfs.makedir(notindexed=True)

                if "store" in self.requirements:
                    self.localvfs.mkdir("store")

                    # create an invalid changelog
                    self.localvfs.append(
                        "00changelog.i",
                        b"\0\0\0\1"
                        b" dummy changelog to prevent using the old repo layout",
                    )
            else:
                raise errormod.RepoError(_("repository %s not found") % path)
        elif create:
            raise errormod.RepoError(_("repository %s already exists") % path)
        else:
            try:
                self.requirements = scmutil.readrequires(self.localvfs, self.supported)
            except IOError as inst:
                if inst.errno != errno.ENOENT:
                    raise

        cachepath = self.localvfs.join("cache")
        self.sharedpath = self.path
        self.sharedroot = self.root
        try:
            sharedpath = self.localvfs.readutf8("sharedpath").rstrip("\n")
            if "relshared" in self.requirements:
                sharedpath = self.localvfs.join(sharedpath)
            sharedvfs = vfsmod.vfs(sharedpath, realpath=True)
            cachepath = sharedvfs.join("cache")
            s = sharedvfs.base
            if not sharedvfs.exists():
                raise errormod.RepoError(
                    _(".hg/sharedpath points to nonexistent directory %s") % s
                )
            self.sharedpath = s
            self.sharedroot = sharedvfs.dirname(s)
            self.sharedvfs = sharedvfs
            # Read the requirements of the shared repo to make sure we
            # support them.
            try:
                scmutil.readrequires(self.sharedvfs, self.supported)
            except IOError as inst:
                if inst.errno != errno.ENOENT:
                    raise
            self.sharedvfs.audit = self._getvfsward(self.sharedvfs.audit)
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
            self.sharedvfs = self.localvfs

        # If this is a new repo, generate the dynamic configs. We must do this
        # after the sharedvfs is set up so we can generate the dynamic config in
        # the shared vfs.
        if created:
            self.ui.reloadconfigs(self.root)

        self.store = store.store(
            self.requirements,
            self.sharedpath,
            lambda base: vfsmod.vfs(base, cacheaudited=True),
            self.ui.uiconfig(),
        )
        self.spath = self.store.path
        self.svfs = self.store.vfs
        self.sjoin = self.store.join
        self.localvfs.createmode = self.store.createmode
        self.sharedvfs.createmode = self.store.createmode
        self.cachevfs = vfsmod.vfs(cachepath, cacheaudited=True)
        self.cachevfs.createmode = self.store.createmode
        if self.ui.configbool("devel", "all-warnings") or self.ui.configbool(
            "devel", "check-locks"
        ):
            if util.safehasattr(self.svfs, "vfs"):  # this is filtervfs
                self.svfs.vfs.audit = self._getsvfsward(self.svfs.vfs.audit)
            else:  # standard vfs
                self.svfs.audit = self._getsvfsward(self.svfs.audit)
        if not create and "store" in self.requirements:
            try:
                self.storerequirements = scmutil.readrequires(
                    self.svfs, self.storesupported
                )
            except IOError as inst:
                if inst.errno != errno.ENOENT:
                    raise
        if create:
            self._writerequirements()
            self._writestorerequirements()
        if "hgsql" in self.requirements:
            # hgsql wants raw access to revlog. Disable modern features
            # unconditionally for hgsql.
            self.ui.setconfig("experimental", "evolution", "obsolete", "hgsql")
            self.ui.setconfig("experimental", "narrow-heads", "false", "hgsql")
            self.ui.setconfig("experimental", "rust-commits", "false", "hgsql")
            self.ui.setconfig("format", "use-zstore-commit-data", "false", "hgsql")
            self.ui.setconfig("visibility", "enabled", "false", "hgsql")

        self._dirstatevalidatewarned = False

        self._branchcaches = {}
        self.filterpats = {}
        self._datafilters = {}
        self._transref = self._lockref = self._wlockref = None

        self.connectionpool = connectionpool.connectionpool(self)

        self._smallcommitmetadata = None

        # A cache for various files under .hg/ that tracks file changes,
        # (used by the filecache decorator)
        #
        # Maps a property name to its util.filecacheentry
        self._filecache = {}

        # post-dirstate-status hooks
        self._postdsstatus = []

        # generic mapping between names and nodes
        self.names = namespaces.namespaces(self)

        self._applyopenerreqs()

        self._eventreporting = True
        try:
            self._svfsmigration()
            self._narrowheadsmigration()
            self._zstorecommitdatamigration()
        except errormod.LockHeld:
            # Not fatal.
            pass

    def _svfsmigration(self):
        # Migrate 'remotenames' and 'bookmarks' state from sharedvfs to
        # storevfs.
        # This cannot be safely done in the remotenames extension because
        # changelog might access 'remotenames' and other extensions might
        # use changelog before 'remotenames.reposetup'.
        for name in ["remotenames", "bookmarks"]:
            if self.sharedvfs.exists(name) and not os.path.exists(self.svfs.join(name)):
                with self.wlock(wait=False), self.lock(wait=False):
                    data = self.sharedvfs.read(name)
                    # avoid svfs.write so it does not write into metalog.
                    util.writefile(self.svfs.join(name), data)

    def _narrowheadsmigration(self):
        """Migrate if 'narrow-heads' config has changed."""
        narrowheadsdesired = self.ui.configbool("experimental", "narrow-heads")
        # 'narrow-heads' must work with visibility and remotenames
        if (
            not self.ui.configbool("visibility", "enabled")
            or self.ui.config("extensions", "remotenames") == "!"
            or "visibleheads" not in self.storerequirements
        ):
            # Set narrow-heads to False so other code paths wouldn't try
            # to use it.
            if narrowheadsdesired:
                self.ui.setconfig("experimental", "narrow-heads", False)
            narrowheadsdesired = False
        narrowheadscurrent = "narrowheads" in self.storerequirements
        if narrowheadsdesired != narrowheadscurrent:
            if narrowheadsdesired:
                # Migrating up is easy: Just add the requirement.
                with self.lock(wait=False):
                    self.storerequirements.add("narrowheads")
                    self._writestorerequirements()
            else:
                # Migrating down to non-narrow heads requires restoring phases.
                # For this invocation, still pretend that we use narrow-heads.
                # But the next invocation will use non-narrow-heads.
                self.ui.setconfig("experimental", "narrow-heads", True)
                with self.lock(wait=False):
                    # Writing to <shared repo path>/.hg/phaseroots
                    # Accessing the raw file directly without going through
                    # complicated phasescache APIs.
                    draftroots = self.nodes("roots(draft())")
                    lines = set(self.svfs.tryread("phaseroots").splitlines(True))
                    toadd = ""
                    for root in draftroots:
                        # 1: phases.draft
                        line = "1 %s\n" % hex(root)
                        if line not in lines:
                            toadd += line
                    with self.svfs.open("phaseroots", "ab") as f:
                        f.write(pycompat.encodeutf8(toadd))
                    self.storerequirements.remove("narrowheads")
                    self._writestorerequirements()

    @contextmanager
    def disableeventreporting(self):
        self._eventreporting = False
        yield
        self._eventreporting = True

    def _zstorecommitdatamigration(self):
        """Migrate if 'narrow-heads' config has changed."""
        zstorecommitdatadesired = self.ui.configbool("format", "use-zstore-commit-data")
        zstorecommitdatacurrent = "zstorecommitdata" in self.storerequirements
        if zstorecommitdatadesired != zstorecommitdatacurrent:
            if zstorecommitdatadesired:
                # Migrating up. Read all commits in revlog and store them in
                # zstore.
                with self.lock(wait=False):
                    self._syncrevlogtozstore()
                    self.storerequirements.add("zstorecommitdata")
                    self._writestorerequirements()
            else:
                # Migrating down is just removing the store requirement.
                with self.lock(wait=False):
                    self.storerequirements.remove("zstorecommitdata")
                    self._writestorerequirements()

    def _syncrevlogtozstore(self):
        """Sync commit data from revlog to zstore"""
        zstore = bindings.zstore.zstore(self.svfs.join("hgcommits/v1"))
        self.changelog.zstore = zstore

        if self.ui.configbool(
            "format", "use-zstore-commit-data-revlog-fallback"
        ) or self.ui.configbool("format", "use-zstore-commit-data-server-fallback"):
            revs = list(self.revs("not public()"))
        else:
            revs = self

        with progress.bar(
            self.ui, _("migrating commit data"), _("commits"), len(revs)
        ) as prog:
            cl = self.changelog
            cl.zstore = None
            textwithheader = revlog.textwithheader
            clrevision = cl.revision
            clparents = cl.parents
            clnode = cl.node
            insert = zstore.insert
            contains = zstore.__contains__
            for rev in revs:
                prog.value += 1
                node = clnode(rev)
                if contains(node):
                    continue
                text = clrevision(rev)
                p1, p2 = clparents(node)
                newnode = insert(textwithheader(text, p1, p2))
                assert node == newnode
                if (rev + 1) % 1000000 == 0:
                    zstore.flush()
            zstore.flush()

    @property
    def vfs(self):
        self.ui.develwarn(
            "use of bare vfs instead of localvfs or sharedvfs", stacklevel=1
        )
        return self.localvfs

    def _getvfsward(self, origfunc):
        """build a ward for self.localvfs and self.sharedvfs"""
        rref = weakref.ref(self)

        def checkvfs(path, mode=None):
            ret = origfunc(path, mode=mode)
            repo = rref()
            if (
                repo is None
                or not util.safehasattr(repo, "_wlockref")
                or not util.safehasattr(repo, "_lockref")
            ):
                return
            if mode in (None, "r", "rb"):
                return
            if path.startswith(repo.path):
                # truncate name relative to the repository (.hg)
                path = path[len(repo.path) + 1 :]
            if path.startswith("cache/"):
                msg = 'accessing cache with vfs instead of cachevfs: "%s"'
                repo.ui.develwarn(msg % path, stacklevel=2, config="cache-vfs")
            if path.startswith("journal."):
                # journal is covered by 'lock'
                if repo._currentlock(repo._lockref) is None:
                    repo.ui.develwarn(
                        'write with no lock: "%s"' % path,
                        stacklevel=2,
                        config="check-locks",
                    )
            elif repo._currentlock(repo._wlockref) is None:
                # rest of vfs files are covered by 'wlock'
                #
                # exclude special files
                for prefix in self._wlockfreeprefix:
                    if path.startswith(prefix):
                        return
                repo.ui.develwarn(
                    'write with no wlock: "%s"' % path,
                    stacklevel=2,
                    config="check-locks",
                )
            return ret

        return checkvfs

    def _getsvfsward(self, origfunc):
        """build a ward for self.svfs"""
        rref = weakref.ref(self)

        def checksvfs(path, mode=None):
            ret = origfunc(path, mode=mode)
            repo = rref()
            if repo is None or not util.safehasattr(repo, "_lockref"):
                return
            if mode in (None, "r", "rb"):
                return
            if path.startswith(repo.sharedpath):
                # truncate name relative to the repository (.hg)
                path = path[len(repo.sharedpath) + 1 :]
            if repo._currentlock(repo._lockref) is None:
                for prefix in self._lockfreeprefix:
                    if path.startswith(prefix):
                        return
                repo.ui.develwarn('write with no lock: "%s"' % path, stacklevel=3)
            return ret

        return checksvfs

    def _featuresetup(self, setupfuncs, basesupported):
        if setupfuncs:
            supported = set(basesupported)  # use private copy
            extmods = set(m.__name__ for n, m in extensions.extensions(self.ui))
            for setupfunc in setupfuncs:
                if setupfunc.__module__ in extmods:
                    setupfunc(self.ui, supported)
        else:
            supported = basesupported
        return supported

    def close(self):
        if util.safehasattr(self, "connectionpool"):
            self.connectionpool.close()

        self.commitpending()

    def commitpending(self):
        # If we have any pending manifests, commit them to disk.
        if "manifestlog" in self.__dict__:
            self.manifestlog.commitpending()

        if "fileslog" in self.__dict__:
            self.fileslog.commitpending()

    def _loadextensions(self):
        extensions.loadall(self.ui)

    def _restrictcapabilities(self, caps):
        if self.ui.configbool("experimental", "bundle2-advertise"):
            caps = set(caps)
            capsblob = bundle2.encodecaps(bundle2.getrepocaps(self))
            caps.add("bundle2=" + urlreq.quote(capsblob))
        return caps

    def _applyopenerreqs(self):
        self.svfs.options = dict(
            (r, 1) for r in self.requirements if r in self.openerreqs
        )
        # experimental config: format.chunkcachesize
        chunkcachesize = self.ui.configint("format", "chunkcachesize")
        if chunkcachesize is not None:
            self.svfs.options["chunkcachesize"] = chunkcachesize
        # experimental config: format.maxchainlen
        maxchainlen = self.ui.configint("format", "maxchainlen")
        if maxchainlen is not None:
            self.svfs.options["maxchainlen"] = maxchainlen
        # experimental config: format.manifestcachesize
        manifestcachesize = self.ui.configint("format", "manifestcachesize")
        if manifestcachesize is not None:
            self.svfs.options["manifestcachesize"] = manifestcachesize
        # experimental config: format.aggressivemergedeltas
        aggressivemergedeltas = self.ui.configbool("format", "aggressivemergedeltas")
        self.svfs.options["aggressivemergedeltas"] = aggressivemergedeltas
        self.svfs.options["lazydeltabase"] = not scmutil.gddeltaconfig(self.ui)
        mmapindexthreshold = self.ui.configbytes("experimental", "mmapindexthreshold")
        if mmapindexthreshold is not None:
            self.svfs.options["mmapindexthreshold"] = mmapindexthreshold
        withsparseread = self.ui.configbool("experimental", "sparse-read")
        srdensitythres = float(
            self.ui.config("experimental", "sparse-read.density-threshold")
        )
        srmingapsize = self.ui.configbytes("experimental", "sparse-read.min-gap-size")
        self.svfs.options["with-sparse-read"] = withsparseread
        self.svfs.options["sparse-read-density-threshold"] = srdensitythres
        self.svfs.options["sparse-read-min-gap-size"] = srmingapsize

        for r in self.requirements:
            if r.startswith("exp-compression-"):
                self.svfs.options["compengine"] = r[len("exp-compression-") :]

        treemanifestserver = self.ui.configbool("treemanifest", "server")
        self.svfs.options["treemanifest-server"] = treemanifestserver

        bypassrevlogtransaction = (
            self.ui.configbool("experimental", "narrow-heads")
            and self.ui.configbool("experimental", "head-based-commit-transaction")
            and self.ui.configbool("experimental", "rust-commits")
        )
        self.svfs.options["bypass-revlog-transaction"] = bypassrevlogtransaction

    def _writerequirements(self):
        scmutil.writerequires(self.localvfs, self.requirements)

    def _writestorerequirements(self):
        if "store" in self.requirements:
            util.info(
                "writestorerequirements",
                requirements=" ".join(sorted(self.storerequirements)),
            )
            scmutil.writerequires(self.svfs, self.storerequirements)

    def peer(self):
        return localpeer(self)  # not cached to avoid reference cycle

    def pull(
        self, source="default", bookmarknames=(), headnodes=(), headnames=(), quiet=True
    ):
        """Pull specified revisions and remote bookmarks.

        headnodes is a list of binary nodes to pull.
        headnames is a list of text names (ex. hex prefix of a commit hash).
        bookmarknames is a list of bookmark names to pull.

        If a remote bookmark no longer exists on the server-side, it will be
        removed.

        This differs from the traditional pull command in a few ways:
        - Pull nothing if both bookmarknames and headnodes are empty.
        - Do not write local bookmarks.
        - Only update remote bookmarks that are explicitly specified. Do not
          save every name the server has.

        This is also done in proper ways so the remote bookmarks updated will
        match the commits pulled. The remotenames extension might fail to do so
        as it issues a second wireproto command to fetch remote bookmarks,
        which can be racy.
        """
        # All nodes are known locally. No need to pull.
        if (
            not bookmarknames
            and all(n in self for n in headnodes)
            and all(n in self for n in headnames)
        ):
            return

        configoverride = {}
        if quiet:
            configoverride[("ui", "quiet")] = True
        with self.conn(source) as conn, self.wlock(), self.lock(), self.transaction(
            "pull"
        ), self.ui.configoverride(configoverride):
            remote = conn.peer
            remotenamechanges = {}  # changes to remotenames, {name: hexnode}
            heads = set()

            # Resolve the bookmark names to heads.
            if bookmarknames:
                remotebookmarks = remote.listkeyspatterns(
                    "bookmarks", patterns=list(bookmarknames)
                )  # {name: hexnode}
                for name in bookmarknames:
                    if name in remotebookmarks:
                        hexnode = remotebookmarks[name]
                        heads.add(bin(hexnode))
                        remotenamechanges[name] = hexnode  # update it
                    else:
                        remotenamechanges[name] = nullhex  # delete it

            # Resolve headnames to heads.
            if headnames:
                batch = remote.iterbatch()
                for name in headnames:
                    batch.lookup(name)
                batch.submit()
                heads.update(batch.results())

            # Merge headnodes into heads.
            for node in headnodes:
                heads.add(node)

            # Only perform a pull if heads are not empty.
            if heads:
                # Bypass the bookmarks logic as remotenames are updated here.
                # Note: remotenamechanges contains bookmark deletion
                # information while the exchange.pull does not know about what
                # to delete.  Consider also bypass phases if narrow-heads is
                # enabled everywhere.
                extras = {"bookmarks": False, "obsolete": False}
                opargs = {"extras": extras}
                heads = sorted(heads)
                exchange.pull(self, remote, heads, opargs=opargs)

            # Update remotenames.
            if remotenamechanges:
                remotename = bookmarks.remotenameforurl(
                    self.ui, remote.url()
                )  # ex. 'default' or 'remote'
                bookmarks.saveremotenames(
                    self, {remotename: remotenamechanges}, override=False
                )

    def conn(self, source="default", **opts):
        """Create a connection from the connection pool"""
        from . import hg  # avoid cycle

        source, _branches = hg.parseurl(self.ui.expandpath(source))
        return self.connectionpool.get(source, opts=opts)

    @repofilecache(localpaths=["shared"])
    def sharedfeatures(self):
        """Returns the set of enabled 'shared' features for this repo"""
        try:
            return set(self.localvfs.read("shared").splitlines())
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
            return set()

    @repofilecache(sharedpaths=["store/bookmarks"], localpaths=["bookmarks.current"])
    def _bookmarks(self):
        return bookmarks.bmstore(self)

    @repofilecache(sharedpaths=["store/remotenames"])
    def _remotenames(self):
        return bookmarks.remotenames(self)

    @property
    def _activebookmark(self):
        return self._bookmarks.active

    # _phasesets depend on changelog. what we need is to call
    # _phasecache.invalidate() if '00changelog.i' was changed, but it
    # can't be easily expressed in filecache mechanism.
    @storecache("phaseroots", "00changelog.i", "remotenames", "visibleheads")
    def _phasecache(self):
        return phases.phasecache(self, self._phasedefaults)

    @storecache("obsstore")
    def obsstore(self):
        return obsolete.makestore(self.ui, self)

    @storecache("00changelog.i", "visibleheads", "remotenames")
    def changelog(self):
        def loadchangelog(self):
            # Remove left-over ".tmp" files created by Rust revlogindex,
            # or by the truncation logic on 00changelog.i.
            names = [
                n
                for n in self.svfs.listdir("")
                if n.startswith(".tmp") or n.startswith("00changelog.i-")
            ]
            now = time.time()
            deleted = 0
            for name in names:
                # svfs might escape "." to "~2e". Bypass that using
                # os.path.join.
                path = os.path.join(self.svfs.join(""), name)
                try:
                    # Avoid deleting files that are being written.
                    # If we do delete files written by revlogindex, it's
                    # okay because revlogindex won't error out.
                    if now - os.stat(path).st_mtime > 10:
                        util.unlink(path)
                        deleted += 1
                except (OSError, IOError):
                    pass
            # Log it so we know when to get rid of this bandaid fix.
            if deleted:
                self.ui.log("features", feature="remove-svfs-dottmp")

            if "doublewritechangelog" in self.storerequirements:
                return changelog2.changelog.opendoublewrite(
                    self.svfs, self.ui.uiconfig()
                )
            if "segmentedchangelog" in self.storerequirements:
                return changelog2.changelog.opensegments(self.svfs, self.ui.uiconfig())

            if (
                (
                    (
                        self.ui.configbool("experimental", "rust-commits")
                        and getattr(self.svfs, "options", {}).get(
                            "bypass-revlog-transaction"
                        )
                    )
                    or "rustrevlogchangelog" in self.storerequirements
                )
                and "hgsql" not in self.requirements
                and "pythonrevlogchangelog" not in self.storerequirements
            ):
                return changelog2.changelog.openrevlog(self.svfs, self.ui.uiconfig())

            if "zstorecommitdata" in self.storerequirements:
                zstore = bindings.zstore.zstore(self.svfs.join("hgcommits/v1"))
            else:
                zstore = None
            return changelog.changelog(
                self.svfs,
                uiconfig=self.ui.uiconfig(),
                trypending=txnutil.mayhavesharedpending(self.root, self.sharedroot),
                zstore=zstore,
            )

        cl = loadchangelog(self)

        # Migrate from inline to non-inline
        # internal config: format.inline-changelog
        if getattr(cl, "_inline", False) and not self.ui.configbool(
            "format", "inline-changelog"
        ):
            self.ui.write_err(_("(migrating to non-inlined changelog)\n"))
            with self.lock():
                cl = loadchangelog(self)
                if cl._inline:
                    # See revlog.checkinlinesize
                    with cl.opener(cl.datafile, "w") as df:
                        for r in cl:
                            df.write(cl._getsegmentforrevs(r, r)[1])
                    with cl.opener(cl.indexfile, "w", atomictemp=True) as fp:
                        cl.version &= ~revlog.FLAG_INLINE_DATA
                        cl._inline = False
                        for i in cl:
                            e = cl._io.packentry(cl.index[i], cl.node, cl.version, i)
                            fp.write(e)
                    # Reopen
                    cl = loadchangelog(self)
        return cl

    def _constructmanifest(self):
        # This is a temporary function while we migrate from manifest to
        # manifestlog. It allows bundlerepo to intercept the manifest creation.
        return manifest.manifestrevlog(self.svfs)

    @storecache("00manifest.i", "00manifesttree.i")
    def manifestlog(self):
        return manifest.manifestlog(self.svfs, self)

    @util.propertycache
    def fileslog(self):
        return filelog.fileslog(self)

    @repofilecache(localpaths=["dirstate"])
    def dirstate(self):
        # type: () -> dirstate.dirstate
        if edenfs.requirement in self.requirements:
            return self._eden_dirstate

        istreestate = "treestate" in self.requirements
        # Block nontreestate repos entirely. Add a config to bypass in case we
        # break something.
        if not istreestate and self.ui.configbool("treestate", "required", True):
            raise errormod.RequirementError(
                "legacy dirstate implementations are no longer supported"
            )
        istreedirstate = "treedirstate" in self.requirements

        return dirstate.dirstate(
            self.localvfs,
            self.ui,
            self.root,
            self._dirstatevalidate,
            self,
            istreestate=istreestate,
            istreedirstate=istreedirstate,
        )

    @util.propertycache
    def _eden_dirstate(self):
        # type: () -> dirstate.dirstate
        # Disable demand import when pulling in the thrift runtime,
        # as it attempts to import missing modules and changes behavior
        # based on what it finds.  Demand import masks those and causes
        # obscure and false import errors at runtime.
        from edenscm import hgdemandimport

        with hgdemandimport.deactivated():
            from . import eden_dirstate as dirstate_reimplementation

        return dirstate_reimplementation.eden_dirstate(self, self.ui, self.root)

    def _dirstatevalidate(self, node):
        # type: (bytes) -> bytes
        self.changelog.rev(node)
        return node

    def __getitem__(self, changeid):
        if changeid is None:
            return context.workingctx(self)
        if isinstance(changeid, slice):
            # wdirrev isn't contiguous so the slice shouldn't include it
            return [
                context.changectx(self, i) for i in range(*changeid.indices(len(self)))
            ]
        try:
            return context.changectx(self, changeid)
        except errormod.WdirUnsupported:
            return context.workingctx(self)

    def __contains__(self, changeid):
        """True if the given changeid exists

        error.LookupError is raised if an ambiguous node specified.
        """
        try:
            self[changeid]
            return True
        except errormod.RepoLookupError:
            return False

    def __nonzero__(self):
        return True

    __bool__ = __nonzero__

    def __len__(self):
        return len(self.changelog)

    def __iter__(self):
        return iter(self.changelog)

    def dageval(self, func):
        """Evaluate func with the current DAG context.

        Unlike changelog.dageval, this function provides extra utilities:
        - draft(): draft commits
        - public(): public commits
        - obsolete(): obsoleted commits
        """

        def getphaseset(repo, phase):
            return repo.changelog.tonodes(repo._phasecache.getrevset(repo, [phase]))

        def removenull(nodes):
            return [n for n in nodes if n != nullid]

        return self.changelog.dageval(
            func,
            extraenv={
                "dot": lambda: removenull(self.dirstate.parents()[:1]),
                "draft": lambda: getphaseset(self, phases.draft),
                "lookup": lambda *names: removenull([self[n].node() for n in names]),
                "obsolete": lambda: mutation.obsoletenodes(self),
                "public": lambda: getphaseset(self, phases.public),
            },
        )

    def revs(self, expr, *args):
        """Find revisions matching a revset.

        The revset is specified as a string ``expr`` that may contain
        %-formatting to escape certain types. See ``revsetlang.formatspec``.

        Revset aliases from the configuration are not expanded. To expand
        user aliases, consider calling ``scmutil.revrange()`` or
        ``repo.anyrevs([expr], user=True)``.

        Returns a revset.abstractsmartset, which is a list-like interface
        that contains integer revisions.
        """
        expr = revsetlang.formatspec(expr, *args)
        m = revset.match(None, expr)
        return m(self)

    def set(self, expr, *args):
        """Find revisions matching a revset and emit changectx instances.

        This is a convenience wrapper around ``revs()`` that iterates the
        result and is a generator of changectx instances.

        Revset aliases from the configuration are not expanded. To expand
        user aliases, consider calling ``scmutil.revrange()``.
        """
        for r in self.revs(expr, *args):
            yield self[r]

    def nodes(self, expr, *args):
        """Find revisions matching a revset and emit their nodes.

        This is a convenience wrapper around ``revs()`` that iterates the
        result and is a generator of nodes.

        Revset aliases from the configuration are not expanded. To expand
        user aliases, consider calling ``scmutil.revrange()``.
        """
        clnode = self.changelog.node
        for r in self.revs(expr, *args):
            yield clnode(r)

    def anyrevs(self, specs, user=False, localalias=None):
        """Find revisions matching one of the given revsets.

        Revset aliases from the configuration are not expanded by default. To
        expand user aliases, specify ``user=True``. To provide some local
        definitions overriding user aliases, set ``localalias`` to
        ``{name: definitionstring}``.
        """
        if user:
            m = revset.matchany(self.ui, specs, repo=self, localalias=localalias)
        else:
            m = revset.matchany(None, specs, localalias=localalias)
        return m(self)

    def url(self):
        return "file:" + self.root

    def hook(self, name, throw=False, **args):
        """Call a hook, passing this repo instance.

        This a convenience method to aid invoking hooks. Extensions likely
        won't call this unless they have registered a custom hook or are
        replacing code that is expected to call a hook.
        """
        return hook.hook(self.ui, self, name, throw, **args)

    @util.propertycache
    def _mutationstore(self):
        return mutation.makemutationstore(self)

    def nodebookmarks(self, node):
        """return the list of bookmarks pointing to the specified node"""
        marks = []
        for bookmark, n in pycompat.iteritems(self._bookmarks):
            if n == node:
                marks.append(bookmark)
        return sorted(marks)

    def branchmap(self):
        """returns a dictionary {branch: [branchheads]} with branchheads
        ordered by increasing revision number"""
        branchmap.updatecache(self)
        return self._branchcaches[None]

    def branchtip(self, branch, ignoremissing=False):
        """return the tip node for a given branch

        If ignoremissing is True, then this method will not raise an error.
        This is helpful for callers that only expect None for a missing branch
        (e.g. namespace).

        """
        try:
            return self.branchmap().branchtip(branch)
        except KeyError:
            if not ignoremissing:
                raise errormod.RepoLookupError(_("unknown branch '%s'") % branch)
            else:
                pass

    def lookup(self, key):
        return self[key].node()

    def lookupbranch(self, key, remote=None):
        repo = remote or self
        if key in repo.branchmap():
            return key

        repo = (remote and remote.local()) and remote or self
        return repo[key].branch()

    def known(self, nodes):
        cl = self.changelog
        nm = cl.nodemap
        result = []
        for n in nodes:
            r = nm.get(n)
            resp = not (r is None)
            result.append(resp)
        return result

    def local(self):
        return self

    def publishing(self):
        # narrow-heads repos are NOT publishing. This ensures pushing to a
        # narrow-heads repo would cause visible heads changes to make the
        # pushed commits visible. Otherwise the pushed commits are invisible
        # during discovery if the push does not update bookmarks on the commit.
        if "narrowheads" in self.storerequirements:
            return False
        # it's safe (and desirable) to trust the publish flag unconditionally
        # so that we don't finalize changes shared between users via ssh or nfs
        return self.ui.configbool("phases", "publish", untrusted=True)

    def cancopy(self):
        if not self.local():
            return False
        if not self.publishing():
            return True
        return True

    def shared(self):
        """the type of shared repository (None if not shared)"""
        if self.sharedpath != self.path:
            return "store"
        return None

    def wjoin(self, f, *insidef):
        return self.localvfs.reljoin(self.root, f, *insidef)

    def file(self, f):
        if f[0] == "/":
            f = f[1:]
        return filelog.filelog(self.svfs, f)

    def changectx(self, changeid):
        return self[changeid]

    def setparents(self, p1, p2=nullid):
        with self.dirstate.parentchange():
            copies = self.dirstate.setparents(p1, p2)
            pctx = self[p1]
            if copies:
                # Adjust copy records, the dirstate cannot do it, it
                # requires access to parents manifests. Preserve them
                # only for entries added to first parent.
                for f in copies:
                    if f not in pctx and copies[f] in pctx:
                        self.dirstate.copy(copies[f], f)
            if p2 == nullid:
                for f, s in sorted(self.dirstate.copies().items()):
                    if f not in pctx and s not in pctx:
                        self.dirstate.copy(None, f)

    def filectx(self, path, changeid=None, fileid=None):
        """changeid can be a changeset revision or node.
           fileid can be a file revision or node."""
        return context.filectx(self, path, changeid, fileid)

    def getcwd(self):
        return self.dirstate.getcwd()

    def pathto(self, f, cwd=None):
        return self.dirstate.pathto(f, cwd)

    def _loadfilter(self, filter):
        if filter not in self.filterpats:
            l = []
            for pat, cmd in self.ui.configitems(filter):
                if cmd == "!":
                    continue
                mf = matchmod.match(self.root, "", [pat])
                fn = None
                params = cmd
                for name, filterfn in pycompat.iteritems(self._datafilters):
                    if cmd.startswith(name):
                        fn = filterfn
                        params = cmd[len(name) :].lstrip()
                        break
                if not fn:
                    fn = lambda s, c, **kwargs: util.filter(s, c)
                l.append((mf, fn, params))
            self.filterpats[filter] = l
        return self.filterpats[filter]

    def _filter(self, filterpats, filename, data):
        for mf, fn, cmd in filterpats:
            if mf(filename):
                self.ui.debug("filtering %s through %s\n" % (filename, cmd))
                data = fn(data, cmd, ui=self.ui, repo=self, filename=filename)
                break

        return data

    @util.propertycache
    def _encodefilterpats(self):
        return self._loadfilter("encode")

    @util.propertycache
    def _decodefilterpats(self):
        return self._loadfilter("decode")

    def adddatafilter(self, name, filter):
        self._datafilters[name] = filter

    def wread(self, filename):
        if self.wvfs.islink(filename):
            data = pycompat.encodeutf8(self.wvfs.readlink(filename))
        else:
            data = self.wvfs.read(filename)
        return self._filter(self._encodefilterpats, filename, data)

    def wwrite(self, filename, data, flags, backgroundclose=False):
        # type: (str, bytes, str, bool) -> int
        """write ``data`` into ``filename`` in the working directory

        This returns length of written (maybe decoded) data.
        """
        data = self._filter(self._decodefilterpats, filename, data)
        if "l" in flags:
            self.wvfs.symlink(data, filename)
        else:
            self.wvfs.write(filename, data, backgroundclose=backgroundclose)
            if "x" in flags:
                self.wvfs.setflags(filename, False, True)
        return len(data)

    def wwritedata(self, filename, data):
        return self._filter(self._decodefilterpats, filename, data)

    def currenttransaction(self):
        """return the current transaction or None if non exists"""
        if self._transref:
            tr = self._transref()
        else:
            tr = None

        if tr and tr.running():
            return tr
        return None

    def transaction(self, desc, report=None):
        if self.ui.configbool("devel", "all-warnings") or self.ui.configbool(
            "devel", "check-locks"
        ):
            if self._currentlock(self._lockref) is None:
                raise errormod.ProgrammingError("transaction requires locking")
        tr = self.currenttransaction()
        if tr is not None:
            return tr.nest()

        # abort here if the journal already exists
        if self.svfs.exists("journal"):
            raise errormod.AbandonedTransactionFoundError(
                _("abandoned transaction found"),
                hint=_("run 'hg recover' to clean up transaction"),
            )

        idbase = b"%.40f#%f" % (random.random(), time.time())
        ha = hex(hashlib.sha1(idbase).digest())
        txnid = "TXN:" + ha
        self.hook("pretxnopen", throw=True, txnname=desc, txnid=txnid)

        self._writejournal(desc)
        renames = [(vfs, x, undoname(x)) for vfs, x in self._journalfiles()]
        if report:
            rp = report
        else:
            rp = self.ui.warn
        vfsmap = {"shared": self.sharedvfs, "local": self.localvfs}
        # we must avoid cyclic reference between repo and transaction.
        reporef = weakref.ref(self)

        def validate(tr2):
            """will run pre-closing hooks"""
            # XXX the transaction API is a bit lacking here so we take a hacky
            # path for now
            #
            # We cannot add this as a "pending" hooks since the 'tr.hookargs'
            # dict is copied before these run. In addition we needs the data
            # available to in memory hooks too.
            #
            # Moreover, we also need to make sure this runs before txnclose
            # hooks and there is no "pending" mechanism that would execute
            # logic only if hooks are about to run.
            #
            # Fixing this limitation of the transaction is also needed to track
            # other families of changes (bookmarks, phases, obsolescence).
            #
            # This will have to be fixed before we remove the experimental
            # gating.
            repo = reporef()
            if hook.hashook(repo.ui, "pretxnclose-bookmark"):
                for name, (old, new) in sorted(tr.changes["bookmarks"].items()):
                    args = tr.hookargs.copy()
                    args.update(bookmarks.preparehookargs(name, old, new))
                    repo.hook("pretxnclose-bookmark", throw=True, txnname=desc, **args)
            if hook.hashook(repo.ui, "pretxnclose-phase"):
                cl = repo.changelog
                for rev, (old, new) in tr.changes["phases"].items():
                    args = tr.hookargs.copy()
                    node = hex(cl.node(rev))
                    args.update(phases.preparehookargs(node, old, new))
                    repo.hook("pretxnclose-phase", throw=True, txnname=desc, **args)

            repo.hook("pretxnclose", throw=True, txnname=desc, **(tr.hookargs))

        def flushchangelog(tr):
            repo = reporef()
            cl = repo.changelog
            if cl.userust("addrevision"):
                main = bookmarks.mainbookmark(repo)
                mainnodes = []
                if main in repo:
                    node = repo[main].node()
                    if node != nullid:
                        mainnodes.append(node)
                cl.inner.flush(mainnodes)

        def releasefn(tr, success):
            repo = reporef()
            # Flush changelog zstore unconditionally. This makes the commit
            # data available even if the transaction gets rolled back.
            zstore = getattr(repo.changelog, "zstore", None)
            if zstore is not None:
                zstore.flush()
            if success:
                # this should be explicitly invoked here, because
                # in-memory changes aren't written out at closing
                # transaction, if tr.addfilegenerator (via
                # dirstate.write or so) isn't invoked while
                # transaction running
                repo.dirstate.write(None)
                flushchangelog(tr)
            else:
                # discard all changes (including ones already written
                # out) in this transaction
                repo.dirstate.restorebackup(None, "journal.dirstate")

                repo.invalidate(clearfilecache=True)

        tr = transaction.transaction(
            rp,
            self.svfs,
            vfsmap,
            "journal",
            "undo",
            aftertrans(renames),
            self.store.createmode,
            validator=validate,
            releasefn=releasefn,
            checkambigfiles=_cachedfiles,
            uiconfig=self.ui.uiconfig(),
            desc=desc,
        )
        tr.changes["nodes"] = []
        tr.changes["obsmarkers"] = set()
        tr.changes["phases"] = {}
        tr.changes["bookmarks"] = {}

        tr.hookargs["txnid"] = txnid

        # TODO: Consider changing 'self' to 'reporef()'.

        def commitnotransaction(tr):
            self.commitpending()

        def abortnotransaction(tr):
            if "manifestlog" in self.__dict__:
                self.manifestlog.abortpending()

            if "fileslog" in self.__dict__:
                self.fileslog.abortpending()

        def writependingnotransaction(tr):
            commitnotransaction(tr)
            tr.addpending("notransaction", writependingnotransaction)
            return False

        tr.addfinalize("notransaction", commitnotransaction)
        tr.addabort("notransaction", abortnotransaction)
        tr.addpending("notransaction", writependingnotransaction)
        tr.addpending("flushchangelog", flushchangelog)

        # If any writes happened outside the transaction, go ahead and flush
        # them before opening the new transaction.
        commitnotransaction(None)

        # note: writing the fncache only during finalize mean that the file is
        # outdated when running hooks. As fncache is used for streaming clone,
        # this is not expected to break anything that happen during the hooks.
        tr.addfinalize("flush-fncache", self.store.write)

        def txnclosehook(tr2):
            """To be run if transaction is successful, will schedule a hook run
            """
            # Don't reference tr2 in hook() so we don't hold a reference.
            # This reduces memory consumption when there are multiple
            # transactions per lock. This can likely go away if issue5045
            # fixes the function accumulation.
            hookargs = tr2.hookargs

            def hookfunc():
                repo = reporef()
                if hook.hashook(repo.ui, "txnclose-bookmark"):
                    bmchanges = sorted(tr.changes["bookmarks"].items())
                    for name, (old, new) in bmchanges:
                        args = tr.hookargs.copy()
                        args.update(bookmarks.preparehookargs(name, old, new))
                        repo.hook(
                            "txnclose-bookmark", throw=False, txnname=desc, **args
                        )

                if hook.hashook(repo.ui, "txnclose-phase"):
                    cl = repo.changelog
                    phasemv = sorted(tr.changes["phases"].items())
                    for rev, (old, new) in phasemv:
                        args = tr.hookargs.copy()
                        node = hex(cl.node(rev))
                        args.update(phases.preparehookargs(node, old, new))
                        repo.hook("txnclose-phase", throw=False, txnname=desc, **args)

                repo.hook("txnclose", throw=False, txnname=desc, **hookargs)

            reporef()._afterlock(hookfunc)

        tr.addfinalize("txnclose-hook", txnclosehook)
        tr.addpostclose("warms-cache", self._buildcacheupdater(tr))

        def txnaborthook(tr2):
            """To be run if transaction is aborted
            """
            reporef().hook("txnabort", throw=False, txnname=desc, **tr2.hookargs)

        tr.addabort("txnabort-hook", txnaborthook)
        # avoid eager cache invalidation. in-memory data should be identical
        # to stored data if transaction has no error.
        tr.addpostclose("refresh-filecachestats", self._refreshfilecachestats)
        self._transref = weakref.ref(tr)
        return tr

    def _journalfiles(self):
        return (
            (self.svfs, "journal"),
            (self.localvfs, "journal.dirstate"),
            (self.localvfs, "journal.branch"),
            (self.localvfs, "journal.desc"),
            (self.svfs, "journal.bookmarks"),
            (self.svfs, "journal.phaseroots"),
            (self.svfs, "journal.visibleheads"),
        )

    def undofiles(self):
        return [(vfs, undoname(x)) for vfs, x in self._journalfiles()]

    def _writejournal(self, desc):
        self.dirstate.savebackup(None, "journal.dirstate")
        self.localvfs.writeutf8("journal.branch", "default")
        self.localvfs.writeutf8("journal.desc", "%d\n%s\n" % (len(self), desc))
        self.svfs.write("journal.bookmarks", self.svfs.tryread("bookmarks"))
        self.svfs.write("journal.phaseroots", self.svfs.tryread("phaseroots"))
        self.svfs.write("journal.visibleheads", self.svfs.tryread("visibleheads"))

    def recover(self):
        with self.lock():
            if self.svfs.exists("journal"):
                self.ui.status(_("rolling back interrupted transaction\n"))
                vfsmap = {
                    "": self.svfs,
                    "shared": self.sharedvfs,
                    "local": self.localvfs,
                }
                transaction.rollback(
                    self.svfs,
                    vfsmap,
                    "journal",
                    self.ui.warn,
                    checkambigfiles=_cachedfiles,
                )
                self.invalidate()
                return True
            else:
                self.ui.warn(_("no interrupted transaction available\n"))
                return False

    def rollback(self, dryrun=False, force=False):
        wlock = lock = dsguard = None
        try:
            wlock = self.wlock()
            lock = self.lock()
            if self.svfs.exists("undo"):
                dsguard = dirstateguard.dirstateguard(self, "rollback")

                return self._rollback(dryrun, force, dsguard)
            else:
                self.ui.warn(_("no rollback information available\n"))
                return 1
        finally:
            release(dsguard, lock, wlock)

    def _rollback(self, dryrun, force, dsguard):
        ui = self.ui
        try:
            args = pycompat.decodeutf8(self.localvfs.read("undo.desc")).splitlines()
            (oldlen, desc, detail) = (int(args[0]), args[1], None)
            if len(args) >= 3:
                detail = args[2]
            oldtip = oldlen - 1

            if detail and ui.verbose:
                msg = _(
                    "repository tip rolled back to revision %d" " (undo %s: %s)\n"
                ) % (oldtip, desc, detail)
            else:
                msg = _("repository tip rolled back to revision %d" " (undo %s)\n") % (
                    oldtip,
                    desc,
                )
        except IOError:
            msg = _("rolling back unknown transaction\n")
            desc = None

        if not force and self["."] != self["tip"] and desc == "commit":
            raise errormod.Abort(
                _("rollback of last commit while not checked out " "may lose data"),
                hint=_("use -f to force"),
            )

        ui.status(msg)
        if dryrun:
            return 0

        parents = self.dirstate.parents()
        self.destroying()
        vfsmap = {"": self.svfs, "shared": self.sharedvfs, "local": self.localvfs}
        transaction.rollback(
            self.svfs, vfsmap, "undo", ui.warn, checkambigfiles=_cachedfiles
        )
        if self.svfs.exists("undo.bookmarks"):
            self.svfs.rename("undo.bookmarks", "bookmarks", checkambig=True)
        if self.svfs.exists("undo.phaseroots"):
            self.svfs.rename("undo.phaseroots", "phaseroots", checkambig=True)
        if self.svfs.exists("undo.visibleheads"):
            self.svfs.rename("undo.visibleheads", "visibleheads", checkambig=True)
        self.invalidate()

        parentgone = (
            parents[0] not in self.changelog.nodemap
            or parents[1] not in self.changelog.nodemap
        )
        if parentgone:
            # prevent dirstateguard from overwriting already restored one
            dsguard.close()

            self.dirstate.restorebackup(None, "undo.dirstate")
            try:
                branch = self.localvfs.readutf8("undo.branch")
                self.dirstate.setbranch(encoding.tolocal(branch))
            except IOError:
                ui.warn(
                    _(
                        "named branch could not be reset: "
                        "current branch is still '%s'\n"
                    )
                    % self.dirstate.branch()
                )

            parents = tuple([p.rev() for p in self[None].parents()])
            if len(parents) > 1:
                ui.status(
                    _("working directory now based on " "revisions %d and %d\n")
                    % parents
                )
            else:
                ui.status(
                    _("working directory now based on " "revision %d\n") % parents
                )
            mergemod.mergestate.clean(self, self["."].node())

        # TODO: if we know which new heads may result from this rollback, pass
        # them to destroy(), which will prevent the branchhead cache from being
        # invalidated.
        self.destroyed()
        return 0

    def _buildcacheupdater(self, newtransaction):
        """called during transaction to build the callback updating cache

        Lives on the repository to help extension who might want to augment
        this logic. For this purpose, the created transaction is passed to the
        method.
        """
        # we must avoid cyclic reference between repo and transaction.
        reporef = weakref.ref(self)

        def updater(tr):
            repo = reporef()
            repo.updatecaches(tr)

        return updater

    def updatecaches(self, tr=None):
        """warm appropriate caches

        If this function is called after a transaction closed. The transaction
        will be available in the 'tr' argument. This can be used to selectively
        update caches relevant to the changes in that transaction.
        """
        if tr is not None and tr.hookargs.get("source") == "strip":
            # During strip, many caches are invalid but
            # later call to `destroyed` will refresh them.
            return

    def invalidatecaches(self):
        self._branchcaches.clear()
        self.invalidatevolatilesets()

    def invalidatevolatilesets(self):
        obsolete.clearobscaches(self)
        mutation.clearobsoletecache(self)
        if "_phasecache" in self._filecache and "_phasecache" in self.__dict__:
            self._phasecache.invalidate()

    def invalidatedirstate(self):
        """Invalidates the dirstate, causing the next call to dirstate
        to check if it was modified since the last time it was read,
        rereading it if it has.

        This is different to dirstate.invalidate() that it doesn't always
        rereads the dirstate. Use dirstate.invalidate() if you want to
        explicitly read the dirstate again (i.e. restoring it to a previous
        known good state)."""
        # eden_dirstate has its own invalidation logic.
        if edenfs.requirement in self.requirements:
            self.dirstate.invalidate()
            return

        if hascache(self, "dirstate"):
            for k in self.dirstate._filecache:
                try:
                    delattr(self.dirstate, k)
                except AttributeError:
                    pass
            delattr(self, "dirstate")

    def invalidate(self, clearfilecache=False):
        """Invalidates both store and non-store parts other than dirstate

        If a transaction is running, invalidation of store is omitted,
        because discarding in-memory changes might cause inconsistency
        (e.g. incomplete fncache causes unintentional failure, but
        redundant one doesn't).
        """
        for k in list(self._filecache.keys()):
            # dirstate is invalidated separately in invalidatedirstate()
            if k == "dirstate":
                continue
            if (
                k == "changelog"
                and self.currenttransaction()
                and (self.changelog._delayed or self.changelog.userust("addrevision"))
            ):
                # The changelog object may store unwritten revisions. We don't
                # want to lose them.
                # TODO: Solve the problem instead of working around it.
                continue

            if k == "manifestlog" and "manifestlog" in self.__dict__:
                # The manifestlog may have uncommitted additions, let's just
                # flush them to disk so we don't lose them.
                self.manifestlog.commitpending()

            if clearfilecache:
                del self._filecache[k]
            try:
                delattr(self, k)
            except AttributeError:
                pass

        if "fileslog" in self.__dict__:
            # The fileslog may have uncommitted additions, let's just
            # flush them to disk so we don't lose them.
            self.fileslog.commitpending()
            del self.__dict__["fileslog"]

        self.invalidatecaches()
        if not self.currenttransaction():
            # TODO: Changing contents of store outside transaction
            # causes inconsistency. We should make in-memory store
            # changes detectable, and abort if changed.
            self.store.invalidatecaches()

    def invalidatechangelog(self):
        """Invalidates the changelog. Discard pending changes."""
        if "changelog" in self._filecache:
            del self._filecache["changelog"]
        if "changelog" in self.__dict__:
            del self.__dict__["changelog"]

    def invalidateall(self):
        """Fully invalidates both store and non-store parts, causing the
        subsequent operation to reread any outside changes."""
        # extension should hook this to invalidate its caches
        self.invalidate()
        self.invalidatedirstate()

    def _refreshfilecachestats(self, tr):
        """Reload stats of cached files so that they are flagged as valid"""
        for k, ce in self._filecache.items():
            if k == "dirstate" or k not in self.__dict__:
                continue
            ce.refresh()

    def _lock(
        self,
        vfs,
        lockname,
        wait,
        releasefn,
        acquirefn,
        desc,
        inheritchecker=None,
        parentenvvar=None,
    ):
        parentlock = None
        # the contents of parentenvvar are used by the underlying lock to
        # determine whether it can be inherited
        if parentenvvar is not None:
            parentlock = encoding.environ.get(parentenvvar)

        timeout = 0
        warntimeout = 0
        if wait:
            timeout = self.ui.configint("ui", "timeout")
            warntimeout = self.ui.configint("ui", "timeout.warn")

        l = lockmod.trylock(
            self.ui,
            vfs,
            lockname,
            timeout,
            warntimeout,
            releasefn=releasefn,
            acquirefn=acquirefn,
            desc=desc,
            inheritchecker=inheritchecker,
            parentlock=parentlock,
        )
        return l

    def _afterlock(self, callback):
        """add a callback to be run when the repository is fully unlocked

        The callback will be executed when the outermost lock is released
        (with wlock being higher level than 'lock')."""
        for ref in (self._wlockref, self._lockref):
            l = ref and ref()
            if l and l.held:
                l.postrelease.append(callback)
                break
        else:  # no lock have been found.
            callback()

    def lock(self, wait=True):
        """Lock the repository store (.hg/store) and return a weak reference
        to the lock. Use this before modifying the store (e.g. committing or
        stripping). If you are opening a transaction, get a lock as well.)

        If both 'lock' and 'wlock' must be acquired, ensure you always acquires
        'wlock' first to avoid a dead-lock hazard."""
        l = self._currentlock(self._lockref)
        if l is not None:
            l.lock()
            return l

        if self.ui.configbool("devel", "debug-lockers"):
            util.debugstacktrace(
                "acquiring store lock for %s" % self.root, skip=1, depth=5
            )

        svfs = self.svfs
        # Invalidate metalog state.
        metalog = svfs.__dict__.pop("metalog", None)
        if metalog and metalog.isdirty():
            # |<- A ->|<----------- repo lock --------->|
            #         |<- B ->|<- transaction ->|<- C ->|
            #  ^^^^^^^
            raise errormod.ProgrammingError(
                "metalog should not be changed before repo.lock"
            )
        releasefn = None
        if self.ui.configbool("experimental", "metalog"):
            # XXX: metalog.commit should ideally be only called by a transaction
            # close. However, there are misuses across the code base, so we
            # cannot really rely on transaction now.
            def metalogcommit():
                metalog = svfs.__dict__.pop("metalog", None)
                # |<- A ->|<----------- repo lock --------->|
                #         |<- B ->|<- transaction ->|<- C ->|
                #                                    ^^^^^^^
                if metalog and metalog.isdirty():
                    raise errormod.ProgrammingError(
                        "metalog change outside a transaction is unsupported"
                    )

            releasefn = metalogcommit

        l = self._lock(
            self.svfs,
            "lock",
            wait,
            releasefn,
            self.invalidate,
            _("repository %s") % self.origroot,
        )
        self._lockref = weakref.ref(l)
        return l

    def _wlockchecktransaction(self):
        if self.currenttransaction() is not None:
            raise errormod.LockInheritanceContractViolation(
                "wlock cannot be inherited in the middle of a transaction"
            )

    def wlock(self, wait=True):
        """Lock the non-store parts of the repository (everything under
        .hg except .hg/store) and return a weak reference to the lock.

        Use this before modifying files in .hg.

        If both 'lock' and 'wlock' must be acquired, ensure you always acquires
        'wlock' first to avoid a dead-lock hazard."""
        l = self._wlockref and self._wlockref()
        if l is not None and l.held:
            l.lock()
            return l

        if self.ui.configbool("devel", "debug-lockers"):
            util.debugstacktrace("acquiring wlock for %s" % self.root, skip=1, depth=5)

        # We do not need to check for non-waiting lock acquisition.  Such
        # acquisition would not cause dead-lock as they would just fail.
        if wait and (
            self.ui.configbool("devel", "all-warnings")
            or self.ui.configbool("devel", "check-locks")
        ):
            if self._currentlock(self._lockref) is not None:
                self.ui.develwarn('"wlock" acquired after "lock"')

        # if this is a shared repo and we must also lock the shared wlock
        # or else we can deadlock due to lock ordering issues
        sharedwlock = None
        if self.shared():
            sharedwlock = self._lock(
                self.sharedvfs,
                "wlock",
                wait,
                None,
                None,
                _("shared repository %s") % self.sharedroot,
                inheritchecker=self._wlockchecktransaction,
                parentenvvar="HG_WLOCK_LOCKER",
            )

        def unlock():
            if sharedwlock:
                sharedwlock.release()
            if self.dirstate.pendingparentchange():
                self.dirstate.invalidate()
            else:
                self.dirstate.write(None)

            self._filecache["dirstate"].refresh()

        l = self._lock(
            self.localvfs,
            "wlock",
            wait,
            unlock,
            self.invalidatedirstate,
            _("working directory of %s") % self.origroot,
            inheritchecker=self._wlockchecktransaction,
            parentenvvar="HG_WLOCK_LOCKER",
        )
        self._wlockref = weakref.ref(l)
        return l

    def _currentlock(self, lockref):
        """Returns the lock if it's held, or None if it's not."""
        if lockref is None:
            return None
        l = lockref()
        if l is None or not l.held:
            return None
        return l

    def currentwlock(self):
        """Returns the wlock if it's held, or None if it's not."""
        return self._currentlock(self._wlockref)

    def _filecommit(self, fctx, manifest1, manifest2, linkrev, tr, changelist):
        """
        commit an individual file as part of a larger transaction
        """

        fname = fctx.path()
        fparent1 = manifest1.get(fname, nullid)
        fparent2 = manifest2.get(fname, nullid)
        node = fctx.filenode()
        if node in [fparent1, fparent2]:
            self.ui.debug("reusing %s filelog entry (parent match)\n" % fname)
            if node == fparent1:
                if manifest1.flags(fname) != fctx.flags():
                    changelist.append(fname)
            else:
                if manifest2.flags(fname) != fctx.flags():
                    changelist.append(fname)
            return node

        flog = self.file(fname)
        meta = {}
        copy = fctx.renamed()

        # Is filelog metadata (currently only the copy information) unchanged?
        # If it is, then filenode hash could be unchanged if data is also known
        # unchanged. This allows fast path of adding new file nodes without
        # calculating .data() or new hash.
        #
        # Metadata is unchanged if there is no copy information.
        # Otherwise, the "copy from" revision needs to be checked.
        metamatched = not bool(copy)

        if copy and copy[0] != fname:
            # Mark the new revision of this file as a copy of another
            # file.  This copy data will effectively act as a parent
            # of this new revision.  If this is a merge, the first
            # parent will be the nullid (meaning "look up the copy data")
            # and the second one will be the other parent.  For example:
            #
            # 0 --- 1 --- 3   rev1 changes file foo
            #   \       /     rev2 renames foo to bar and changes it
            #    \- 2 -/      rev3 should have bar with all changes and
            #                      should record that bar descends from
            #                      bar in rev2 and foo in rev1
            #
            # this allows this merge to succeed:
            #
            # 0 --- 1 --- 3   rev4 reverts the content change from rev2
            #   \       /     merging rev3 and rev4 should use bar@rev2
            #    \- 2 --- 4        as the merge base
            #

            cfname, oldcrev = copy
            crev = manifest1.get(cfname)
            newfparent = fparent2

            if manifest2:  # branch merge
                if fparent2 == nullid or crev is None:  # copied on remote side
                    if cfname in manifest2:
                        crev = manifest2[cfname]
                        newfparent = fparent1

            # Here, we used to search backwards through history to try to find
            # where the file copy came from if the source of a copy was not in
            # the parent directory. However, this doesn't actually make sense to
            # do (what does a copy from something not in your working copy even
            # mean?) and it causes bugs (eg, issue4476). Instead, we will warn
            # the user that copy information was dropped, so if they didn't
            # expect this outcome it can be fixed, but this is the correct
            # behavior in this circumstance.

            if crev:
                self.ui.debug(" %s: copy %s:%s\n" % (fname, cfname, hex(crev)))
                meta["copy"] = cfname
                meta["copyrev"] = hex(crev)
                metamatched = crev == oldcrev
                fparent1, fparent2 = nullid, newfparent
            else:
                self.ui.warn(
                    _("warning: can't find ancestor for '%s' " "copied from '%s'!\n")
                    % (fname, cfname)
                )

        elif fparent1 == nullid:
            fparent1, fparent2 = fparent2, nullid
        elif fparent2 != nullid:
            # is one parent an ancestor of the other?
            fparentancestors = flog.commonancestorsheads(fparent1, fparent2)
            if fparent1 in fparentancestors:
                fparent1, fparent2 = fparent2, nullid
            elif fparent2 in fparentancestors:
                fparent2 = nullid

        # Fast path: reuse rawdata? (skip .data() (ex. flagprocessors) and hash
        # calculation)
        if (
            metamatched
            and node is not None
            # some filectxs do not support rawdata or flags
            and util.safehasattr(fctx, "rawdata")
            and util.safehasattr(fctx, "rawflags")
            # some (external) filelogs do not have addrawrevision
            and util.safehasattr(flog, "addrawrevision")
            # parents must match to be able to reuse rawdata
            and fctx.filelog().parents(node) == (fparent1, fparent2)
        ):
            # node is different from fparents, no need to check manifest flag
            changelist.append(fname)
            if node in flog.nodemap:
                self.ui.debug("reusing %s filelog node (exact match)\n" % fname)
                return node
            self.ui.debug("reusing %s filelog rawdata\n" % fname)
            return flog.addrawrevision(
                fctx.rawdata(), tr, linkrev, fparent1, fparent2, node, fctx.rawflags()
            )

        # is the file changed?
        text = fctx.data()
        if fparent2 != nullid or flog.cmp(fparent1, text) or meta:
            changelist.append(fname)
            return flog.add(text, meta, tr, linkrev, fparent1, fparent2)
        # are just the flags changed during merge?
        elif fname in manifest1 and manifest1.flags(fname) != fctx.flags():
            changelist.append(fname)

        return fparent1

    def checkcommitpatterns(self, wctx, match, status, fail):
        """check for commit arguments that aren't committable"""
        if match.isexact() or match.prefix():
            matched = set(status.modified + status.added + status.removed)

            for f in match.files():
                f = self.dirstate.normalize(f)
                if f == "." or f in matched:
                    continue
                if f in status.deleted:
                    fail(f, _("file not found!"))

                d = f + "/"
                for mf in matched:
                    if mf.startswith(d):
                        break
                else:
                    if self.wvfs.isdir(f):
                        fail(f, _("no match under directory!"))
                    elif f not in self.dirstate:
                        fail(f, _("file not tracked!"))

    def commit(
        self,
        text="",
        user=None,
        date=None,
        match=None,
        force=False,
        editor=False,
        extra=None,
        loginfo=None,
        mutinfo=None,
    ):
        """Add a new revision to current repository.

        Revision information is gathered from the working directory,
        match can be used to filter the committed files. If editor is
        supplied, it is called to get a commit message.
        """
        if extra is None:
            extra = {}

        if loginfo is None:
            loginfo = {}

        def fail(f, msg):
            raise errormod.Abort("%s: %s" % (f, msg))

        if not match:
            match = matchmod.always(self.root, "")

        if not force:
            match.bad = fail

        wlock = lock = tr = None
        try:
            wlock = self.wlock()
            lock = self.lock()  # for recent changelog (see issue4368)

            wctx = self[None]
            merge = len(wctx.parents()) > 1

            if not force and merge and not match.always():
                raise errormod.Abort(
                    _(
                        "cannot partially commit a merge "
                        "(do not specify files or patterns)"
                    )
                )

            status = self.status(match=match, clean=force)

            # make sure all explicit patterns are matched
            if not force:
                self.checkcommitpatterns(wctx, match, status, fail)

            loginfo.update({"checkoutidentifier": self.dirstate.checkoutidentifier})

            cctx = context.workingcommitctx(
                self, status, text, user, date, extra, loginfo, mutinfo
            )

            # internal config: ui.allowemptycommit
            allowemptycommit = (
                wctx.branch() != wctx.p1().branch()
                or extra.get("close")
                or merge
                or cctx.files()
                or self.ui.configbool("ui", "allowemptycommit")
            )
            if not allowemptycommit:
                return None

            if merge and cctx.deleted():
                raise errormod.Abort(_("cannot commit merge with missing files"))

            ms = mergemod.mergestate.read(self)
            mergeutil.checkunresolved(ms)

            if editor:
                cctx._text = editor(self, cctx)
            edited = text != cctx._text

            # Save commit message in case this transaction gets rolled back
            # (e.g. by a pretxncommit hook).  Leave the content alone on
            # the assumption that the user will use the same editor again.
            msgfn = self.savecommitmessage(cctx._text)

            p1, p2 = self.dirstate.parents()
            hookp1, hookp2 = hex(p1), (p2 != nullid and hex(p2) or "")
            try:
                self.hook("precommit", throw=True, parent1=hookp1, parent2=hookp2)
                tr = self.transaction("commit")
                ret = self.commitctx(cctx, True)
            except:  # re-raises
                if edited:
                    self.ui.write(_("note: commit message saved in %s\n") % msgfn)
                raise
            # update bookmarks, dirstate and mergestate
            bookmarks.update(self, [p1, p2], ret)
            cctx.markcommitted(ret)
            ms.reset()
            tr.close()

        finally:
            lockmod.release(tr, lock, wlock)

        def commithook(node=hex(ret), parent1=hookp1, parent2=hookp2):
            # hack for command that use a temporary commit (eg: histedit)
            # temporary commit got stripped before hook release
            if self.changelog.hasnode(ret):
                self.hook("commit", node=node, parent1=parent1, parent2=parent2)

        self._afterlock(commithook)
        return ret

    def commitctx(self, ctx, error=False):
        """Add a new revision to current repository.
        Revision information is passed via the context argument.
        """

        tr = None
        p1, p2 = ctx.p1(), ctx.p2()
        user = ctx.user()

        descriptionlimit = self.ui.configbytes("commit", "description-size-limit")
        if descriptionlimit:
            descriptionlen = len(ctx.description())
            if descriptionlen > descriptionlimit:
                raise errormod.Abort(
                    _("commit message length (%s) exceeds configured limit (%s)")
                    % (descriptionlen, descriptionlimit)
                )

        extraslimit = self.ui.configbytes("commit", "extras-size-limit")
        if extraslimit:
            extraslen = sum(len(k) + len(v) for k, v in pycompat.iteritems(ctx.extra()))
            if extraslen > extraslimit:
                raise errormod.Abort(
                    _("commit extras total size (%s) exceeds configured limit (%s)")
                    % (extraslen, extraslimit)
                )

        lock = self.lock()
        try:
            tr = self.transaction("commit")
            trp = weakref.proxy(tr)

            if ctx.manifestnode():
                # reuse an existing manifest revision
                mn = ctx.manifestnode()
                files = ctx.files()
            elif ctx.files():
                m1ctx = p1.manifestctx()
                m2ctx = p2.manifestctx()
                mctx = m1ctx.copy()

                m = mctx.read()
                m1 = m1ctx.read()
                m2 = m2ctx.read()

                # Validate that the files that are checked in can be interpreted
                # as utf8. This is to protect against potential crashes as we
                # move to utf8 file paths. Changing encoding is a beast on top
                # of storage format.
                try:
                    for f in ctx.added():
                        if isinstance(f, bytes):
                            f.decode("utf-8")
                except UnicodeDecodeError as inst:
                    raise errormod.Abort(
                        _("invalid file name encoding: %s!") % inst.object
                    )

                # check in files
                added = []
                changed = []

                removed = []
                drop = []

                def handleremove(f):
                    if f in m1 or f in m2:
                        removed.append(f)
                        if f in m:
                            del m[f]
                            drop.append(f)

                for f in ctx.removed():
                    handleremove(f)
                for f in sorted(ctx.modified() + ctx.added()):
                    if ctx[f] is None:
                        # in memctx this means removal
                        handleremove(f)
                    else:
                        added.append(f)

                linkrev = len(self)
                self.ui.note(_("committing files:\n"))
                for f in added:
                    self.ui.note(f + "\n")
                    try:
                        fctx = ctx[f]
                        m[f] = self._filecommit(fctx, m1, m2, linkrev, trp, changed)
                        m.setflag(f, fctx.flags())
                    except OSError:
                        self.ui.warn(_("trouble committing %s!\n") % f)
                        raise
                    except IOError as inst:
                        errcode = getattr(inst, "errno", errno.ENOENT)
                        if error or errcode and errcode != errno.ENOENT:
                            self.ui.warn(_("trouble committing %s!\n") % f)
                        raise

                # update manifest
                self.ui.note(_("committing manifest\n"))
                removed = sorted(removed)
                drop = sorted(drop)
                if added or drop:
                    mn = (
                        mctx.write(
                            trp,
                            linkrev,
                            p1.manifestnode(),
                            p2.manifestnode(),
                            added,
                            drop,
                        )
                        or p1.manifestnode()
                    )
                else:
                    mn = p1.manifestnode()
                files = changed + removed
            else:
                mn = p1.manifestnode()
                files = []

            # update changelog
            self.ui.note(_("committing changelog\n"))
            self.changelog.delayupdate(tr)
            extra = ctx.extra().copy()
            n = self.changelog.add(
                mn,
                files,
                ctx.description(),
                trp,
                p1.node(),
                p2.node(),
                user,
                ctx.date(),
                extra,
            )
            xp1, xp2 = p1.hex(), p2 and p2.hex() or ""
            self.hook("pretxncommit", throw=True, node=hex(n), parent1=xp1, parent2=xp2)
            # set the new commit is proper phase
            targetphase = phases.newcommitphase(self.ui)
            if targetphase:
                # retract boundary do not alter parent changeset.
                # if a parent have higher the resulting phase will
                # be compliant anyway
                #
                # if minimal phase was 0 we don't need to retract anything
                phases.registernew(self, tr, targetphase, [n])
            # Newly committed commits shouldn't be obsoleted.
            if obsolete.isenabled(self, obsolete.createmarkersopt):
                obsolete.revive([self[n]], "commit")
            # Newly committed commits should be visible.
            if targetphase > phases.public:
                visibility.add(self, [n])
            mutinfo = ctx.mutinfo()
            if mutinfo is not None:
                entry = mutation.createentry(n, mutinfo)
                mutation.recordentries(self, [entry], skipexisting=False)
            tr.close()

            # Generate and log interesting data.
            loginfo = ctx.loginfo()
            path = os.path.commonprefix([os.path.dirname(p) for p in files])
            if len(path) > 0:
                loginfo.update({"common_directory_path": path})

            diffnumber = diffprops.parserevfromcommitmsg(ctx.description())
            if diffnumber is not None:
                loginfo.update({"phabricator_diff_number": diffnumber})

            self.ui.log("commit_info", node=hex(n), author=user, **loginfo)
            return n
        finally:
            if tr:
                tr.release()
            lock.release()

    def destroying(self):
        """Inform the repository that nodes are about to be destroyed.
        Intended for use by strip and rollback, so there's a common
        place for anything that has to be done before destroying history.

        This is mostly useful for saving state that is in memory and waiting
        to be flushed when the current lock is released. Because a call to
        destroyed is imminent, the repo will be invalidated causing those
        changes to stay in memory (waiting for the next unlock), or vanish
        completely.
        """
        # When using the same lock to commit and strip, the phasecache is left
        # dirty after committing. Then when we strip, the repo is invalidated,
        # causing those changes to disappear.
        if "_phasecache" in vars(self):
            self._phasecache.write()

    def destroyed(self):
        """Inform the repository that nodes have been destroyed.
        Intended for use by strip and rollback, so there's a common
        place for anything that has to be done after destroying history.
        """
        # When one tries to:
        # 1) destroy nodes thus calling this method (e.g. strip)
        # 2) use phasecache somewhere (e.g. commit)
        #
        # then 2) will fail because the phasecache contains nodes that were
        # removed. We can either remove phasecache from the filecache,
        # causing it to reload next time it is accessed, or simply filter
        # the removed nodes now and write the updated cache.
        self._phasecache.filterunknown(self)
        self._phasecache.write()

        # refresh all repository caches
        self.updatecaches()
        self.invalidate()

    def walk(self, match, node=None):
        """
        walk recursively through the directory tree or a given
        changeset, finding all files matched by the match
        function
        """
        self.ui.deprecwarn("use repo[node].walk instead of repo.walk", "4.3")
        return self[node].walk(match)

    def status(
        self,
        node1=".",
        node2=None,
        match=None,
        ignored=False,
        clean=False,
        unknown=False,
    ):
        """a convenience method that calls node1.status(node2)"""
        return self[node1].status(node2, match, ignored, clean, unknown)

    def addpostdsstatus(self, ps, afterdirstatewrite=True):
        """Add a callback to run within the wlock, at the point at which status
        fixups happen.

        On status completion, callback(wctx, status) will be called with the
        wlock held, unless the dirstate has changed from underneath or the wlock
        couldn't be grabbed.

        If afterdirstatewrite is True, it runs after writing dirstate,
        otherwise it runs before writing dirstate.

        Callbacks should not capture and use a cached copy of the dirstate --
        it might change in the meanwhile. Instead, they should access the
        dirstate via wctx.repo().dirstate.

        This list is emptied out after each status run -- extensions should
        make sure it adds to this list each time dirstate.status is called.
        Extensions should also make sure they don't call this for statuses
        that don't involve the dirstate.
        """

        # The list is located here for uniqueness reasons -- it is actually
        # managed by the workingctx, but that isn't unique per-repo.
        self._postdsstatus.append((ps, afterdirstatewrite))

    def postdsstatus(self, afterdirstatewrite=True):
        """Used by workingctx to get the list of post-dirstate-status hooks."""
        return [ps for (ps, after) in self._postdsstatus if after == afterdirstatewrite]

    def clearpostdsstatus(self):
        """Used by workingctx to clear post-dirstate-status hooks."""
        del self._postdsstatus[:]

    def headrevs(self, start=None, includepublic=True, includedraft=True, reverse=True):
        cl = self.changelog
        if self.ui.configbool("experimental", "narrow-heads"):
            if includedraft:
                nodes = [
                    n
                    for n in self.dirstate.parents() + list(self._bookmarks.values())
                    if n != nullid
                ]
            else:
                nodes = []
            # Do not treat the draft heads returned by remotenames as
            # unconditionally visible. This makes it possible to hide
            # them by "hg hide".
            publicnodes, _draftnodes = _remotenodes(self)
            cl = self.changelog
            torev = cl.nodemap.get
            if includepublic:
                nodes += publicnodes
            if includedraft:
                if cl._uiconfig.configbool("visibility", "all-heads"):
                    visibleheads = cl._visibleheads.allheads()
                else:
                    visibleheads = cl._visibleheads.heads
                nodes += visibleheads
            # Do not report nullid. index2.headsancestors does not support it.
            if cl.userust("index2"):
                hasnode = cl.hasnode
                nodes = [n for n in set(nodes) - {nullid} if hasnode(n)]
                headnodes = cl.dag.headsancestors(nodes)
                headrevs = list(cl.torevs(headnodes))
            else:
                revs = [r for r in map(torev, nodes) if r is not None and r >= 0]
                headrevs = cl.index2.headsancestors(revs)
            # headrevs is already in DESC.
            reverse = not reverse
        else:
            headrevs = cl.rawheadrevs()
        if start is not None:
            startrev = cl.rev(start)
            headrevs = [r for r in headrevs if r > startrev]
        if reverse:
            return list(reversed(headrevs))
        else:
            return headrevs

    def heads(self, start=None, includepublic=True, includedraft=True):
        headrevs = self.headrevs(start, includepublic, includedraft)
        return list(map(self.changelog.node, headrevs))

    def branchheads(self, branch=None, start=None, closed=False):
        """return a list of heads for the given branch

        Heads are returned in topological order, from newest to oldest.
        If branch is None, use the dirstate branch.
        If start is not None, return only heads reachable from start.
        If closed is True, return heads that are marked as closed as well.
        """
        if branch is None:
            branch = self[None].branch()
        branches = self.branchmap()
        if branch not in branches:
            return []
        # the cache returns heads ordered lowest to highest
        bheads = list(reversed(branches.branchheads(branch, closed=closed)))
        if start is not None:
            # filter out the heads that cannot be reached from startrev
            fbheads = set(self.changelog.nodesbetween([start], bheads)[2])
            bheads = [h for h in bheads if h in fbheads]
        return bheads

    def branches(self, nodes):
        if not nodes:
            nodes = [self.changelog.tip()]
        b = []
        for n in nodes:
            t = n
            while True:
                p = self.changelog.parents(n)
                if p[1] != nullid or p[0] == nullid:
                    b.append((t, n, p[0], p[1]))
                    break
                n = p[0]
        return b

    def between(self, pairs):
        r = []

        for top, bottom in pairs:
            n, l, i = top, [], 0
            f = 1

            while n != bottom and n != nullid:
                p = self.changelog.parents(n)[0]
                if i == f:
                    l.append(n)
                    f = f * 2
                n = p
                i += 1

            r.append(l)

        return r

    def checkpush(self, pushop):
        """Extensions can override this function if additional checks have
        to be performed before pushing, or call it if they override push
        command.
        """

    @util.propertycache
    def prepushoutgoinghooks(self):
        """Return util.hooks consists of a pushop with repo, remote, outgoing
        methods, which are called before pushing changesets.
        """
        return util.hooks()

    def pushkey(self, namespace, key, old, new):
        try:
            tr = self.currenttransaction()
            hookargs = {}
            if tr is not None:
                hookargs.update(tr.hookargs)
            hookargs["namespace"] = namespace
            hookargs["key"] = key
            hookargs["old"] = old
            hookargs["new"] = new
            self.hook("prepushkey", throw=True, **hookargs)
        except errormod.HookAbort as exc:
            self.ui.write_err(_("pushkey-abort: %s\n") % exc)
            if exc.hint:
                self.ui.write_err(_("(%s)\n") % exc.hint)
            return False
        self.ui.debug('pushing key for "%s:%s"\n' % (namespace, key))
        ret = pushkey.push(self, namespace, key, old, new)

        def runhook():
            self.hook(
                "pushkey", namespace=namespace, key=key, old=old, new=new, ret=ret
            )

        self._afterlock(runhook)
        return ret

    def listkeys(self, namespace, patterns=None):
        self.hook("prelistkeys", throw=True, namespace=namespace)
        self.ui.debug('listing keys for "%s"\n' % namespace)
        values = pushkey.list(self, namespace)
        self.hook("listkeys", namespace=namespace, values=values)
        if patterns is not None and namespace == "bookmarks":
            bmarks = values
            values = util.sortdict()
            for pattern in patterns:
                if pattern.endswith("*"):
                    pattern = "re:^" + pattern[:-1] + ".*"
                kind, pat, matcher = util.stringmatcher(pattern)
                for bookmark, node in pycompat.iteritems(bmarks):
                    if matcher(bookmark):
                        values[bookmark] = node
        return values

    def debugwireargs(self, one, two, three=None, four=None, five=None):
        """used to test argument passing over the wire"""
        return "%s %s %s %s %s" % (one, two, three, four, five)

    def savecommitmessage(self, text):
        self.localvfs.writeutf8("last-message.txt", text)
        path = self.localvfs.join("last-message.txt")
        return self.pathto(path[len(self.root) + 1 :])

    def automigratestart(self):
        """perform potentially expensive in-place migrations

        Called at the start of pull if pull.automigrate is true
        """
        try:
            with self.wlock(wait=False), self.lock(wait=False):
                treestate.automigrate(self)
                mutation.automigrate(self)
                visibility.automigrate(self)
        except errormod.LockHeld:
            self.ui.debug("skipping automigrate because lock is held\n")
        except errormod.AbandonedTransactionFoundError:
            # Migration can fail if the repository has an abandoned transaction
            # and needs to be manually recovered.  Ignore this for now and proceed,
            # so that read-only operations in the repository still work.  The abandoned
            # transaction will still block commands that need to modify the repository,
            # but those will fail later.
            self.ui.debug("skipping automigrate due to an abandoned transaction\n")

    def automigratefinish(self):
        """perform potentially expensive in-place migrations

        Called at the end of pull if pull.automigrate is true
        """
        pass

    @property
    def smallcommitmetadata(self):
        # Load smallcommitmetdata lazily because it is rarely used.
        if self._smallcommitmetadata is None:
            self._smallcommitmetadata = smallcommitmetadata.smallcommitmetadata(
                self.cachevfs, self.ui.configint("smallcommitmetadata", "entrylimit")
            )
        return self._smallcommitmetadata


# used to avoid circular references so destructors work
def aftertrans(files):
    renamefiles = [tuple(t) for t in files]

    def a():
        for vfs, src, dest in renamefiles:
            # if src and dest refer to a same file, vfs.rename is a no-op,
            # leaving both src and dest on disk. delete dest to make sure
            # the rename couldn't be such a no-op.
            vfs.tryunlink(dest)
            try:
                vfs.rename(src, dest)
            except OSError:  # journal file does not yet exist
                pass

    return a


def undoname(fn):
    base, name = os.path.split(fn)
    assert name.startswith("journal")
    return os.path.join(base, name.replace("journal", "undo", 1))


def instance(ui, path, create):
    return localrepository(ui, util.urllocalpath(path), create)


def islocal(path):
    return True


def newreporequirements(repo):
    """Determine the set of requirements for a new local repository.

    Extensions can wrap this function to specify custom requirements for
    new repositories.
    """
    ui = repo.ui
    requirements = {"revlogv1"}
    if ui.configbool("format", "usestore"):
        requirements.add("store")
        if ui.configbool("format", "usefncache"):
            requirements.add("fncache")
            if ui.configbool("format", "dotencode"):
                requirements.add("dotencode")

    compengine = ui.config("experimental", "format.compression")
    if compengine not in util.compengines:
        raise errormod.Abort(
            _(
                "compression engine %s defined by "
                "experimental.format.compression not available"
            )
            % compengine,
            hint=_('run "hg debuginstall" to list available ' "compression engines"),
        )

    requirements.add("treestate")

    # zlib is the historical default and doesn't need an explicit requirement.
    if compengine != "zlib":
        requirements.add("exp-compression-%s" % compengine)

    if scmutil.gdinitconfig(ui):
        requirements.add("generaldelta")
    if ui.configbool("experimental", "treemanifest"):
        requirements.add("treemanifest")

    return requirements


def newrepostorerequirements(repo):
    ui = repo.ui
    requirements = set()

    # hgsql wants stable legacy revlog access, bypassing visibleheads,
    # narrow-heads, and zstore-commit-data.
    if "hgsql" in repo.requirements:
        return requirements

    if ui.configbool("visibility", "enabled"):
        requirements.add("visibleheads")

    # See also self._narrowheadsmigration()
    if (
        ui.configbool("experimental", "narrow-heads")
        and ui.configbool("visibility", "enabled")
        and extensions.isenabled(ui, "remotenames")
    ):
        requirements.add("narrowheads")

    if ui.configbool("format", "use-zstore-commit-data"):
        requirements.add("zstorecommitdata")

    if ui.configbool("format", "use-segmented-changelog"):
        requirements.add("segmentedchangelog")
        # linkrev are not going to work with segmented changelog,
        # because the numbers might get rewritten.
        requirements.add("invalidatelinkrev")

    return requirements


def _remotenodes(repo):
    """Return (remote public nodes, remote draft nodes)."""
    publicnodes = []
    draftnodes = []

    draftpattern = repo.ui.config("infinitepush", "branchpattern")
    if draftpattern:
        isdraft = util.stringmatcher(draftpattern)[-1]
    else:

        def isdraft(name):
            return False

    remotebookmarks = repo._remotenames["bookmarks"]
    for fullname, nodes in remotebookmarks.items():
        # fullname: remote/foo/bar
        # remote: remote, name: foo/bar
        remote, name = bookmarks.splitremotename(fullname)
        if isdraft(name):
            draftnodes += nodes
        else:
            publicnodes += nodes

    return publicnodes, draftnodes
