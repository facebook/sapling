# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# hg.py - repository classes for mercurial
#
# Copyright 2005-2007 Olivia Mackall <olivia@selenic.com>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import os
import shutil
from typing import Iterable, Optional, Union

from . import (
    bookmarks,
    bundlerepo,
    clone as clonemod,
    cmdutil,
    destutil,
    eagerpeer,
    edenfs,
    error,
    exchange,
    extensions,
    localrepo,
    lock,
    merge as mergemod,
    mononokepeer,
    node,
    perftrace,
    phases,
    progress,
    pycompat,
    scmutil,
    sshpeer,
    ui as uimod,
    url,
    util,
    vfs as vfsmod,
)
from .i18n import _


release = lock.release

# shared features
sharedbookmarks = "bookmarks"


def _local(path):
    path = util.expandpath(util.urllocalpath(path))
    return os.path.isfile(path) and bundlerepo or localrepo


def addbranchrevs(lrepo, other, branches, revs):
    peer = other.peer()  # a courtesy to callers using a localrepo for other
    hashbranch, branches = branches
    if not hashbranch and not branches:
        x = revs or None
        if util.safehasattr(revs, "first"):
            y = revs.first()
        elif revs:
            y = revs[0]
        else:
            y = None
        return x, y
    if revs:
        revs = list(revs)
    else:
        revs = []

    if not peer.capable("branchmap"):
        if branches:
            raise error.Abort(_("remote branch lookup not supported"))
        revs.append(hashbranch)
        return revs, revs[0]
    branchmap = peer.branchmap()

    def primary(branch):
        if branch == ".":
            if not lrepo:
                raise error.Abort(_("dirstate branch not accessible"))
            branch = lrepo.dirstate.branch()
        if branch in branchmap:
            revs.extend(node.hex(r) for r in reversed(branchmap[branch]))
            return True
        else:
            return False

    for branch in branches:
        if not primary(branch):
            raise error.RepoLookupError(_("unknown branch '%s'") % branch)
    if hashbranch:
        if not primary(hashbranch):
            revs.append(hashbranch)
    return revs, revs[0]


def parseurl(path, branches=None):
    """parse url#branch, returning (url, (branch, branches))"""

    u = util.url(path)
    branch = None
    if u.fragment:
        branch = u.fragment
        u.fragment = None
    return str(u), (branch, branches or [])


schemes = {
    "bundle": bundlerepo,
    "eager": eagerpeer,
    "file": _local,
    "mononoke": mononokepeer,
    "ssh": sshpeer,
    "test": eagerpeer,
}


def _peerlookup(path):
    u = util.url(path)
    scheme = u.scheme or "file"
    thing = schemes.get(scheme) or schemes["file"]
    try:
        return thing(path)
    except TypeError:
        # we can't test callable(thing) because 'thing' can be an unloaded
        # module that implements __call__
        if not util.safehasattr(thing, "instance"):
            raise
        return thing


def islocal(repo: str):
    """return true if repo (or path pointing to repo) is local"""
    if isinstance(repo, str):
        try:
            return _peerlookup(repo).islocal(repo)
        except AttributeError:
            return False
    return repo.local()


def openpath(ui, path):
    """open path with open if local, url.open if remote"""
    pathurl = util.url(path, parsequery=False, parsefragment=False)
    if pathurl.islocal():
        return util.posixfile(pathurl.localpath(), "rb")
    else:
        return url.open(ui, path)


# a list of (ui, repo) functions called for wire peer initialization
wirepeersetupfuncs = []


def _setuprepo(ui, repo, presetupfuncs=None) -> None:
    ui = getattr(repo, "ui", ui)
    for f in presetupfuncs or []:
        f(ui, repo)
    for name, module in extensions.extensions(ui):
        hook = getattr(module, "reposetup", None)
        if hook:
            hook(ui, repo)
    if not repo.local():
        perftrace.traceflag("remote")
        for f in wirepeersetupfuncs:
            f(ui, repo)
    else:
        perftrace.traceflag("local")


@perftrace.tracefunc("Repo Setup")
def repository(
    ui, path: str = "", create: bool = False, presetupfuncs=None, initial_config=None
):
    """return a repository object for the specified path"""
    u = util.url(path)
    if u.scheme == "bundle":
        creator = bundlerepo
    else:
        creator = _local(path)

    repo = creator.instance(ui, path, create, initial_config)
    _setuprepo(ui, repo, presetupfuncs=presetupfuncs)
    repo = repo.local()
    if not repo:
        raise error.Abort(_("repository '%s' is not local") % (path or peer.url()))
    return repo


@perftrace.tracefunc("Peer Setup")
def peer(uiorrepo, opts, path, create: bool = False):
    """return a repository peer for the specified path"""
    rui = remoteui(uiorrepo, opts)
    obj = _peerlookup(path).instance(rui, path, create, initial_config=None)
    # Uncomment this once we stop using file clones in sandcastle
    # if obj.local() and not rui.configbool("experimental", "allowfilepeer"):
    #    raise error.Abort(_("cannot create peer for local file repository '%s'")
    #        % path)
    _setuprepo(rui, obj)
    return obj.peer()


def defaultdest(source):
    """return default destination of clone if none is given

    >>> defaultdest('foo')
    'foo'
    >>> defaultdest('/foo/bar')
    'bar'
    >>> defaultdest('/')
    ''
    >>> defaultdest('')
    ''
    >>> defaultdest('http://example.org/')
    ''
    >>> defaultdest('http://example.org/foo/')
    'foo'
    """
    path = util.url(source).path
    if not path:
        return ""
    return os.path.basename(os.path.normpath(path))


def share(
    ui,
    source: str,
    dest=None,
    update: bool = True,
    bookmarks: bool = True,
    defaultpath=None,
    relative: bool = False,
):
    """create a shared repository"""

    if not islocal(source):
        raise error.Abort(_("can only share local repositories"))

    if not dest:
        dest = defaultdest(source)
    else:
        dest = ui.expandpath(dest)

    if isinstance(source, str):
        origsource = ui.expandpath(source)
        source, branches = parseurl(origsource)
        srcrepo = repository(ui, source)
        rev, checkout = addbranchrevs(srcrepo, srcrepo, branches, None)
    else:
        srcrepo = source.local()
        origsource = source = srcrepo.url()
        checkout = None

    sharedpath = srcrepo.sharedpath  # if our source is already sharing

    destwvfs = vfsmod.vfs(dest, realpath=True)
    destvfs = vfsmod.vfs(
        os.path.join(destwvfs.base, ui.identity.dotdir()), realpath=True
    )

    if destvfs.lexists():
        raise error.Abort(_("destination already exists"))

    if not destwvfs.isdir():
        destwvfs.mkdir()
    destvfs.makedir()

    requirements = srcrepo.requirements.copy()

    if relative:
        try:
            sharedpath = os.path.relpath(sharedpath, destvfs.base)
            requirements.add("relshared")
        except (IOError, ValueError) as e:
            # ValueError is raised on Windows if the drive letters differ on
            # each path
            raise error.Abort(_("cannot calculate relative path"), hint=str(e))
    else:
        requirements.add("shared")

    scmutil.writerequires(destvfs, requirements)
    destvfs.writeutf8("sharedpath", sharedpath)

    r = repository(ui, destwvfs.base)
    postshare(srcrepo, r, bookmarks=bookmarks, defaultpath=defaultpath)
    _postshareupdate(r, update, checkout=checkout)
    return r


def unshare(ui, repo) -> None:
    """convert a shared repository to a normal one

    Copy the store data to the repo and remove the sharedpath data.
    """

    destlock = lock = None
    lock = repo.lock()
    try:
        # we use locks here because if we race with commit, we
        # can end up with extra data in the cloned revlogs that's
        # not pointed to by changesets, thus causing verify to
        # fail

        destlock = copystore(ui, repo, repo.path)

        sharefile = repo.localvfs.join("sharedpath")
        util.rename(sharefile, sharefile + ".old")

        repo.requirements.discard("shared")
        repo.requirements.discard("relshared")
        repo._writerequirements()
    finally:
        destlock and destlock.release()
        lock and lock.release()

    # update store, spath, svfs and sjoin of repo
    # invalidate before rerunning __init__
    repo.invalidate(clearfilecache=True)
    repo.invalidatedirstate()
    repo.__init__(repo.baseui, repo.root)


def postshare(sourcerepo, destrepo, bookmarks: bool = True, defaultpath=None) -> None:
    """Called after a new shared repo is created.

    The new repo only has a requirements file and pointer to the source.
    This function configures additional shared data.

    Extensions can wrap this function and write additional entries to
    destrepo/.hg/shared to indicate additional pieces of data to be shared.
    """
    default = defaultpath or sourcerepo.ui.config("paths", "default")
    if default:
        fp = destrepo.localvfs(destrepo.ui.identity.configrepofile(), "w", text=True)
        fp.write("[paths]\n")
        fp.write("default = %s\n" % default)
        fp.close()

    with destrepo.wlock():
        if bookmarks:
            fp = destrepo.localvfs("shared", "wb")
            fp.write(pycompat.encodeutf8(sharedbookmarks + "\n"))
            fp.close()


def _postshareupdate(repo, update, checkout=None) -> None:
    """Maybe perform a working directory update after a shared repo is created.

    ``update`` can be a boolean or a revision to update to.
    """
    if not update:
        return

    repo.ui.status(_("updating working directory\n"))
    if update is not True:
        checkout = update
    for test in (checkout, "default", "tip"):
        if test is None:
            continue
        try:
            uprev = repo.lookup(test)
            break
        except error.RepoLookupError:
            continue
    # pyre-fixme[61]: `uprev` is undefined, or not always defined.
    _update(repo, uprev)


def copystore(ui, srcrepo, destpath) -> None:
    """copy files from store of srcrepo in destpath

    returns destlock
    """
    destlock = None
    try:
        with progress.bar(ui, _("linking")) as prog:
            hardlink = False
            num = 0
            srcpublishing = srcrepo.publishing()
            srcvfs = vfsmod.vfs(srcrepo.sharedpath)
            dstvfs = vfsmod.vfs(destpath)
            for f in srcrepo.store.copylist():
                if srcpublishing and f.endswith("phaseroots"):
                    continue
                dstbase = os.path.dirname(f)
                if dstbase and not dstvfs.exists(dstbase):
                    dstvfs.mkdir(dstbase)
                if srcvfs.exists(f):
                    if f.endswith("data"):
                        # 'dstbase' may be empty (e.g. revlog format 0)
                        lockfile = os.path.join(dstbase, "lock")
                        # lock to avoid premature writing to the target
                        destlock = lock.lock(dstvfs, lockfile, ui=ui)
                    hardlink, num = util.copyfiles(
                        srcvfs.join(f), dstvfs.join(f), hardlink, num, prog
                    )
        if hardlink:
            ui.debug("linked %d files\n" % num)
        else:
            ui.debug("copied %d files\n" % num)
        # pyre-fixme[7]: Expected `None` but got `Optional[pythonlock]`.
        return destlock
    except:  # re-raises
        release(destlock)
        raise


def clone(
    ui,
    peeropts,
    source,
    dest=None,
    pull: bool = False,
    rev=None,
    update: Union[bool, str] = True,
    stream: Optional[bool] = False,
    shallow: bool = False,
):
    """Make a copy of an existing repository.

    Create a copy of an existing repository in a new directory.  The
    source and destination are URLs, as passed to the repository
    function.  Returns a pair of repository peers, the source and
    newly created destination.

    The location of the source is added to the new repository's
    .hg/hgrc file, as the default to be used for future pulls and
    pushes.

    If an exception is raised, the partly cloned/updated destination
    repository will be deleted.

    Arguments:

    source: repository object or URL

    dest: URL of destination repository to create (defaults to base
    name of source repository)

    pull: always pull from source repository, even in local case or if the
    server prefers streaming

    stream: stream raw data uncompressed from repository (fast over
    LAN, slow over WAN)

    rev: revision to clone up to (implies pull=True)

    update: update working directory after clone completes, if
    destination is local repository (True means update to default rev,
    anything else is treated as a revision)
    """

    ui.log(
        "clone_info",
        rust_clone=False,
        clone_type="full",
        is_update_clone=update,
    )

    if dest is None:
        dest = defaultdest(source)
        if dest:
            ui.status(_("destination directory: %s\n") % dest)
    else:
        dest = ui.expandpath(dest)

    destpeer = None
    dest = util.urllocalpath(dest)
    if not dest:
        raise error.Abort(_("empty destination path is not valid"))

    destcreated = False
    destvfs = vfsmod.vfs(dest, expandpath=True)
    if destvfs.lexists():
        if not destvfs.isdir():
            raise error.Abort(_("destination '%s' already exists") % dest)
        elif destvfs.listdir():
            raise error.Abort(_("destination '%s' is not empty") % dest)
    else:
        destcreated = True

    # Create the destination repo before we even open the connection to the
    # source, so we can use any repo-specific configuration for the connection.
    try:
        # Note: This triggers hgrc.dynamic generation with empty repo hgrc.
        destpeer = repository(ui, dest, create=True)
    except OSError as inst:
        if inst.errno == errno.EEXIST:
            cleandir = None
            raise error.Abort(_("destination '%s' already exists") % dest)
        raise
    destrepo = destpeer.local()

    # Get the source url, so we can write it into the dest hgrc
    if isinstance(source, str):
        origsource = ui.expandpath(source)
    else:
        srcpeer = source.peer()  # in case we were called with a localrepo
        origsource = source = source.peer().url()

    abspath = origsource
    if islocal(origsource):
        abspath = os.path.abspath(util.urllocalpath(origsource))

    if destrepo:
        _writehgrc(destrepo, abspath, ui.configlist("_configs", "configfiles"))
        # Reload hgrc to pick up `%include` configs. We don't need to
        # regenerate dynamicconfig here, unless the hgrc contains reponame or
        # username overrides (unlikely).
        destrepo.ui.reloadconfigs(destrepo.root)

        if shallow:
            from edenscm.ext.remotefilelog.shallowrepo import requirement

            if requirement not in destrepo.requirements:
                with destrepo.lock():
                    destrepo.requirements.add(requirement)
                    destrepo._writerequirements()
                # Reopen the repo so reposetup in extensions can see the added
                # requirement.
                # To keep command line config overrides, reuse the ui from the
                # old repo object. A cleaner way might be figuring out the
                # overrides and then set them, in case extensions changes the
                # class of the ui object.
                origui = destrepo.ui
                destrepo = repository(ui, dest)
                destrepo.ui = origui

    # Construct the srcpeer after the destpeer, so we can use the destrepo.ui
    # configs.
    try:
        if isinstance(source, str):
            source, mayberevs = parseurl(origsource)
            if len(mayberevs) == 1:
                rev = rev or mayberevs[0]
            srcpeer = peer(destrepo.ui if destrepo else ui, peeropts, source)
    except (Exception, KeyboardInterrupt):
        if destcreated:
            # Clean up the entire repo directory we made.
            shutil.rmtree(dest, True)
        else:
            # Clean up just the .hg directory we made.
            shutil.rmtree(os.path.join(dest, ui.identity.dotdir()), True)
        raise

    branch = (None, [])
    # pyre-fixme[61]: `srcpeer` is undefined, or not always defined.
    rev, checkout = addbranchrevs(srcpeer, srcpeer, branch, rev)

    source = util.urllocalpath(source)

    srclock = destlock = destlockw = cleandir = None
    srcrepo = srcpeer.local()
    try:
        if islocal(dest):
            cleandir = dest

        copy = False
        if (
            srcrepo
            and srcrepo.cancopy()
            and islocal(dest)
            and not phases.hassecret(srcrepo)
        ):
            copy = not pull and not rev

        if copy:
            try:
                # we use a lock here because if we race with commit, we
                # can end up with extra data in the cloned revlogs that's
                # not pointed to by changesets, thus causing verify to
                # fail
                srclock = srcrepo.lock(wait=False)
            except error.LockError:
                copy = False

        if copy:
            clonecodepath = "copy"

            srcrepo.hook("preoutgoing", throw=True, source="clone")
            hgdir = os.path.realpath(os.path.join(dest, ui.identity.dotdir()))
            if not os.path.exists(dest):
                os.mkdir(dest)
            else:
                # only clean up directories we create ourselves
                cleandir = hgdir
            try:
                destpath = hgdir
            except OSError as inst:
                if inst.errno == errno.EEXIST:
                    cleandir = None
                    raise error.Abort(_("destination '%s' already exists") % dest)
                raise

            # Drop the existing destrepo so Windows releases the files.
            # Manually gc to ensure the objects are dropped.
            destpeer = destrepo = None
            import gc

            gc.collect()

            destlock = copystore(ui, srcrepo, destpath)
            # repo initialization might also take a lock. Keeping destlock
            # outside the repo object can cause deadlock. To avoid deadlock,
            # we just release destlock here. The lock will be re-acquired
            # soon by `destpeer`, or `local.lock()` below.
            if destlock is not None:
                destlock.release()

            # copy bookmarks over
            srcbookmarks = srcrepo.svfs.join("bookmarks")
            dstbookmarks = os.path.join(destpath, "store", "bookmarks")
            if os.path.exists(srcbookmarks):
                util.copyfile(srcbookmarks, dstbookmarks)

            # we need to re-init the repo after manually copying the data
            # into it
            destpeer = peer(srcrepo, peeropts, dest)
            destrepo = destpeer.local()
            srcrepo.hook("outgoing", source="clone", node=node.hex(node.nullid))
        else:
            clonecodepath = "legacy-pull"

            revs = None
            if rev:
                if not srcpeer.capable("lookup"):
                    raise error.Abort(
                        _(
                            "src repository does not support "
                            "revision lookup and so doesn't "
                            "support clone by revision"
                        )
                    )
                revs = [srcpeer.lookup(r) for r in rev]
                checkout = revs[0]

            # Can we use EdenAPI CloneData provided by a separate EdenAPI
            # client?
            if (
                getattr(destrepo, "nullableedenapi", None)
                and destrepo.name
                and (
                    (
                        ui.configbool("clone", "force-edenapi-clonedata")
                        or destrepo.ui.configbool("clone", "force-edenapi-clonedata")
                    )
                    or (
                        (
                            ui.configbool("clone", "prefer-edenapi-clonedata")
                            or destrepo.ui.configbool(
                                "clone", "prefer-edenapi-clonedata"
                            )
                        )
                        and "segmented-changelog" in destrepo.edenapi.capabilities()
                    )
                )
            ):
                clonecodepath = "segments"
                ui.status(_("fetching lazy changelog\n"))
                clonemod.segmentsclone(srcpeer.url(), destrepo)
            # Can we use the new code path (stream clone + shallow + no
            # update + selective pull)?
            elif (
                destrepo
                and not pull
                and not update
                and not rev
                and shallow
                and stream is not False
                and ui.configbool("remotenames", "selectivepull")
            ):
                if ui.configbool("unsafe", "emergency-clone"):
                    clonecodepath = "emergency"
                    clonemod.emergencyclone(srcpeer.url(), destrepo)
                else:
                    clonecodepath = "revlog"
                    clonemod.revlogclone(srcpeer.url(), destrepo)
            elif destrepo:
                reasons = []
                if pull:
                    reasons.append("pull")
                if update:
                    reasons.append("update")
                if rev:
                    reasons.append("rev")
                if not shallow:
                    reasons.append("not-shallow")
                if stream is False:
                    reasons.append("not-stream")
                if not ui.configbool("remotenames", "selectivepull"):
                    reasons.append("not-selectivepull")
                ui.log(
                    "features",
                    fullargs=repr(pycompat.sysargv),
                    feature="legacy-clone",
                    traceback=util.smarttraceback(),
                    reason=" ".join(reasons),
                )
                with destrepo.wlock(), destrepo.lock(), destrepo.transaction("clone"):
                    if not stream:
                        if pull:
                            stream = False
                        else:
                            stream = None

                    overrides = {
                        # internal config: ui.quietbookmarkmove
                        ("ui", "quietbookmarkmove"): True,
                        # the normal pull process each commit and so is more expensive
                        # than streaming bytes from disk to the wire.
                        # disabling selectivepull allows to run a streamclone
                        ("remotenames", "selectivepull"): False,
                    }
                    opargs = {}
                    if shallow:
                        opargs["extras"] = {"shallow": True}
                    with destrepo.ui.configoverride(overrides, "clone"):
                        exchange.pull(
                            destrepo,
                            # pyre-fixme[61]: `srcpeer` is undefined, or not always
                            #  defined.
                            srcpeer,
                            revs,
                            streamclonerequested=stream,
                            opargs=opargs,
                        )
            elif srcrepo:
                exchange.push(
                    srcrepo, destpeer, revs=revs, bookmarks=srcrepo._bookmarks.keys()
                )
            else:
                raise error.Abort(_("clone from remote to remote not supported"))

        cleandir = None

        if destrepo:
            with destrepo.wlock(), destrepo.lock(), destrepo.transaction("clone"):
                if update:
                    if update is not True:
                        checkout = srcpeer.lookup(update)
                    uprev = None
                    status = None
                    if checkout is not None:
                        try:
                            uprev = destrepo.lookup(checkout)
                        except error.RepoLookupError:
                            if update is not True:
                                try:
                                    uprev = destrepo.lookup(update)
                                except error.RepoLookupError:
                                    pass
                    if uprev is None:
                        try:
                            uprev = destrepo._bookmarks["@"]
                            update = "@"
                            bn = destrepo[uprev].branch()
                            if bn == "default":
                                status = _("updating to bookmark @\n")
                            else:
                                status = _("updating to bookmark @ on branch %s\n") % bn
                        except KeyError:
                            try:
                                uprev = destrepo.branchtip("default")
                            except error.RepoLookupError:
                                uprev = destrepo.lookup("tip")
                    if not status:
                        bn = destrepo[uprev].branch()
                        status = _("updating to branch %s\n") % bn
                    destrepo.ui.status(status)
                    _update(destrepo, uprev)
                    if update in destrepo._bookmarks:
                        bookmarks.activate(destrepo, update)
        clonepreclose(
            ui,
            peeropts,
            source,
            dest,
            pull,
            rev,
            update,
            stream,
            # pyre-fixme[61]: `srcpeer` is undefined, or not always defined.
            srcpeer,
            destpeer,
            clonecodepath=clonecodepath,
        )
    finally:
        release(srclock, destlockw, destlock)
        # pyre-fixme[61]: `srcpeer` is undefined, or not always defined.
        if srcpeer is not None:
            srcpeer.close()
        if destpeer is not None:
            destpeer.close()
        if cleandir is not None:
            shutil.rmtree(cleandir, True)
    return destpeer


def _writehgrc(repo, abspath, configfiles) -> None:
    with repo.wlock(), repo.lock():
        template = _(uimod.samplehgrcs["cloned"])
        with repo.localvfs(repo.ui.identity.configrepofile(), "wb") as fp:
            u = util.url(abspath)
            u.passwd = None
            defaulturl = str(u)
            fp.write(pycompat.encodeutf8(util.tonativeeol(template % defaulturl)))

            if configfiles:
                fp.write(pycompat.encodeutf8(util.tonativeeol("\n")))
                for file in configfiles:
                    fp.write(
                        pycompat.encodeutf8(util.tonativeeol("%%include %s\n" % file))
                    )


def clonepreclose(
    ui,
    peeropts,
    source,
    dest=None,
    pull: bool = False,
    rev=None,
    update: Union[bool, str] = True,
    stream: Optional[bool] = False,
    srcpeer=None,
    destpeer=None,
    clonecodepath=None,
):
    """Wrapped by extensions like remotenames before closing the peers

    clonecodepath is one of:
    - "copy": The clone was done by copying local files.
    - "legacy-pull": The clone was done by the (legacy) pull code path.
    - "revlog": The clone was done by the clone.streamclone code path,
      which is less racy and writes remote bookmarks.
    - "segments": The clone was done by lazy changelog path.
    - "emergency": The clone was done by the emergency code path.
    """
    return srcpeer, destpeer


def _showstats(repo, stats: Iterable[object], quietempty: bool = False) -> None:
    if edenfs.requirement in repo.requirements:
        return _eden_showstats(repo, stats, quietempty)

    if quietempty and not any(stats):
        return
    repo.ui.status(
        _(
            "%d files updated, %d files merged, "
            "%d files removed, %d files unresolved\n"
        )
        % stats
    )


def _eden_showstats(repo, stats, quietempty: bool = False) -> None:
    # We hide the updated and removed counts, because they are not accurate
    # with eden.  One of the primary goals of eden is that the entire working
    # directory does not need to be accessed or traversed on update operations.
    (updated, merged, removed, unresolved) = stats
    if merged or unresolved:
        repo.ui.status(
            _("%d files merged, %d files unresolved\n") % (merged, unresolved)
        )
    elif not quietempty:
        repo.ui.status(_("update complete\n"))


def updaterepo(repo, node, overwrite, updatecheck=None):
    """Update the working directory to node.

    When overwrite is set, changes are clobbered, merged else

    returns stats (see pydoc merge.applyupdates)"""
    return mergemod.update(
        repo,
        node,
        False,
        overwrite,
        labels=["working copy", "destination"],
        updatecheck=updatecheck,
    )


def update(repo, node, quietempty: bool = False, updatecheck=None):
    """update the working directory to node

    Returns if any files were unresolved.
    """
    stats = updaterepo(repo, node, False, updatecheck=updatecheck)
    _showstats(repo, stats, quietempty)
    if stats[3]:
        repo.ui.status(_("use '@prog@ resolve' to retry unresolved file merges\n"))
    return stats[3] > 0


# naming conflict in clone()
_update = update


def clean(repo, node, show_stats: bool = True, quietempty: bool = False):
    """forcibly switch the working directory to node, clobbering changes

    Returns if any files were unresolved.
    """
    stats = updaterepo(repo, node, True)
    repo.localvfs.unlinkpath("graftstate", ignoremissing=True)
    if show_stats:
        _showstats(repo, stats, quietempty)
    return stats[3] > 0


# naming conflict in updatetotally()
_clean = clean


def updatetotally(
    ui, repo, checkout, brev, clean: bool = False, updatecheck: Optional[str] = None
):
    """Update the working directory with extra care for non-file components

    This takes care of non-file components below:

    :bookmark: might be advanced or (in)activated

    This takes arguments below:

    :checkout: to which revision the working directory is updated
    :brev: a name, which might be a bookmark to be activated after updating
    :clean: whether changes in the working directory can be discarded
    :updatecheck: how to deal with a dirty working directory

    Valid values for updatecheck are (None => linear):

     * abort: abort if the working directory is dirty
     * none: don't check (merge working directory changes into destination)
     * linear: check that update is linear before merging working directory
               changes into destination
     * noconflict: check that the update does not result in file merges

    This returns whether conflict is detected at updating or not.
    """
    if updatecheck is None:
        updatecheck = ui.config("commands", "update.check")
        if updatecheck not in ("abort", "none", "linear", "noconflict"):
            # If not configured, or invalid value configured
            updatecheck = "linear"
    with repo.wlock():
        movemarkfrom = None
        warndest = False
        if checkout is None:
            updata = destutil.destupdate(repo, clean=clean)
            checkout, movemarkfrom, brev = updata
            warndest = True

        if clean:
            hasunresolved = _clean(repo, checkout)
        else:
            if updatecheck == "abort":
                cmdutil.bailifchanged(repo, merge=False)
                updatecheck = "none"
            hasunresolved = _update(repo, checkout, updatecheck=updatecheck)
        if not hasunresolved and movemarkfrom:
            if movemarkfrom == repo["."].node():
                pass  # no-op update
            elif bookmarks.update(repo, [movemarkfrom], repo["."].node()):
                b = ui.label(repo._activebookmark, "bookmarks.active")
                ui.status(_("updating bookmark %s\n") % b)
            else:
                # this can happen with a non-linear update
                b = ui.label(repo._activebookmark, "bookmarks")
                ui.status(_("(leaving bookmark %s)\n") % b)
                bookmarks.deactivate(repo)
        elif brev in repo._bookmarks:
            if brev != repo._activebookmark:
                b = ui.label(brev, "bookmarks.active")
                ui.status(_("(activating bookmark %s)\n") % b)
            bookmarks.activate(repo, brev)
        else:
            if repo._activebookmark:
                b = ui.label(repo._activebookmark, "bookmarks")
                ui.status(_("(leaving bookmark %s)\n") % b)
            bookmarks.deactivate(repo)

        if warndest:
            destutil.statusotherdests(ui, repo)

    return hasunresolved


def merge(
    repo, node, force=None, remind: bool = True, mergeforce: bool = False, labels=None
):
    """Branch merge with node, resolving changes. Return true if any
    unresolved conflicts."""
    stats = mergemod.update(
        repo, node, True, force, mergeforce=mergeforce, labels=labels
    )
    _showstats(repo, stats)
    if stats[3]:
        repo.ui.status(
            _(
                "use '@prog@ resolve' to retry unresolved file merges "
                "or '@prog@ goto -C .' to abandon\n"
            )
        )
    elif remind:
        repo.ui.status(_("(branch merge, don't forget to commit)\n"))
    return stats[3] > 0


def remoteui(src, opts):
    "build a remote ui from ui or repo and opts"

    if util.safehasattr(src, "ui"):  # looks like a repository
        # drop repo-specific config
        dst = src.ui.copywithoutrepo()
        # to copy target options from repo
        src = src.ui
    else:
        # assume it's a global ui object
        dst = src.copy()

    # copy ssh-specific options
    for o in "ssh", "remotecmd":
        v = opts.get(o) or src.config("ui", o)
        if v:
            dst.setconfig("ui", o, v, "copied")

    # copy bundle-specific options
    r = src.config("bundle", "mainreporoot")
    if r:
        dst.setconfig("bundle", "mainreporoot", r, "copied")

    # copy selected local settings to the remote ui
    for sect in (
        "auth",
        "auth_proxy",
        "cats",
        "hostfingerprints",
        "hostsecurity",
        "http_proxy",
        "help",
        "edenapi",
        "infinitepush",
        "lfs",
        "mononokepeer",
    ):
        for key, val in src.configitems(sect):
            dst.setconfig(sect, key, val, "copied")
    v = src.config("web", "cacerts")
    if v:
        dst.setconfig("web", "cacerts", util.expandpath(v), "copied")

    return dst


# Files of interest
# Used to check if the repository has changed looking at mtime and size of
# these files.
foi = [
    ("spath", "00changelog.i"),
    ("spath", "phaseroots"),  # ! phase can change content at the same size
    ("path", "bookmarks"),  # ! bookmark can change content at the same size
]


class cachedlocalrepo(object):
    """Holds a localrepository that can be cached and reused."""

    def __init__(self, repo):
        """Create a new cached repo from an existing repo.

        We assume the passed in repo was recently created. If the
        repo has changed between when it was created and when it was
        turned into a cache, it may not refresh properly.
        """
        assert isinstance(repo, localrepo.localrepository)
        self._repo = repo
        self._state, self.mtime = self._repostate()

    def fetch(self):
        """Refresh (if necessary) and return a repository.

        If the cached instance is out of date, it will be recreated
        automatically and returned.

        Returns a tuple of the repo and a boolean indicating whether a new
        repo instance was created.
        """
        # We compare the mtimes and sizes of some well-known files to
        # determine if the repo changed. This is not precise, as mtimes
        # are susceptible to clock skew and imprecise filesystems and
        # file content can change while maintaining the same size.

        state, mtime = self._repostate()
        if state == self._state:
            return self._repo, False

        repo = repository(self._repo.baseui, self._repo.url())
        self._repo = repo
        self._state = state
        self.mtime = mtime

        return self._repo, True

    def _repostate(self):
        state = []
        maxmtime = -1
        for attr, fname in foi:
            prefix = getattr(self._repo, attr)
            p = os.path.join(prefix, fname)
            try:
                st = util.stat(p)
            except OSError:
                st = util.stat(prefix)
            state.append((st.st_mtime, st.st_size))
            maxmtime = max(maxmtime, st.st_mtime)

        return tuple(state), maxmtime

    def copy(self):
        """Obtain a copy of this class instance.

        A new localrepository instance is obtained. The new instance should be
        completely independent of the original.
        """
        repo = repository(self._repo.baseui, self._repo.origroot)
        c = cachedlocalrepo(repo)
        c._state = self._state
        c.mtime = self.mtime
        return c
