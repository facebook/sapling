# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# lfs - hash-preserving large file support using Git-LFS protocol

"""lfs - large file support (EXPERIMENTAL)

Configs::

    [lfs]
    # Remote endpoint. Multiple protocols are supported:
    # - http(s)://user:pass@example.com/path
    #   git-lfs endpoint
    # - file:///tmp/path
    #   local filesystem, usually for testing
    # if unset, lfs will prompt setting this when it must use this value.
    # (default: unset)
    url = https://example.com/lfs

    # Size of a file to make it use LFS
    threshold = 10M

    # How many times to retry before giving up on transferring an object
    retry = 5

    # The local directory to store lfs files for sharing across local clones.
    # If not set, the cache is disabled (default).
    usercache = /path/to/global/cache

    # Verify incoming LFS objects. Could be "none" (not verified), "existance"
    # (LFS object existence check). In the future we might also introduce
    # "hash". This is mostly useful on server-side.
    # (default: none)
    verify = none

    # Enable local on-disk store. This can be disabled to save disk space,
    # at the cost every LFS request will hit the server.
    # (default: true)
    localstore = true
"""

from __future__ import absolute_import

import hashlib

from edenscm.mercurial import (
    blobstore as blobstoremod,
    bundle2,
    changegroup,
    context,
    error,
    exchange,
    extensions,
    filelog,
    hg,
    localrepo,
    registrar,
    revlog,
    scmutil,
    upgrade,
    util,
    vfs as vfsmod,
)
from edenscm.mercurial.i18n import _

from . import blobstore, pointer, wrapper


# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = "ships-with-hg-core"

configtable = {}
configitem = registrar.configitem(configtable)

configitem("experimental", "lfs.user-agent", default=None)

configitem("lfs", "localstore", default=True)
configitem("lfs", "retry", default=5)
configitem("lfs", "threshold", default=None)
configitem("lfs", "url", default="")
configitem("lfs", "usercache", default=None)
configitem("lfs", "verify", default="none")

cmdtable = {}
command = registrar.command(cmdtable)

templatekeyword = registrar.templatekeyword()


def featuresetup(ui, supported):
    # don't die on seeing a repo with the lfs requirement
    supported |= {"lfs"}


def uisetup(ui):
    localrepo.localrepository.featuresetupfuncs.add(featuresetup)


def reposetup(ui, repo):
    # Nothing to do with a remote repo
    if not repo.local():
        return

    threshold = repo.ui.configbytes("lfs", "threshold")

    repo.svfs.options["lfsthreshold"] = threshold
    if repo.ui.configbool("lfs", "localstore"):
        localstore = blobstore.local(repo)
    else:
        localstore = blobstoremod.memlocal()
    repo.svfs.lfslocalblobstore = localstore
    repo.svfs.lfsremoteblobstore = blobstore.remote(repo.ui)

    # Push hook
    repo.prepushoutgoinghooks.add("lfs", wrapper.prepush)

    if "lfs" not in repo.requirements:

        def checkrequireslfs(ui, repo, **kwargs):
            if "lfs" not in repo.requirements:
                ctx = repo[kwargs["node"]]
                # TODO: is there a way to just walk the files in the commit?
                if any(ctx[f].islfs() for f in ctx.files() if f in ctx):
                    repo.requirements.add("lfs")
                    repo._writerequirements()

        ui.setconfig("hooks", "commit.lfs", checkrequireslfs, "lfs")


def wrapfilelog(filelog):
    wrapfunction = extensions.wrapfunction

    wrapfunction(filelog, "addrevision", wrapper.filelogaddrevision)
    wrapfunction(filelog, "renamed", wrapper.filelogrenamed)
    wrapfunction(filelog, "size", wrapper.filelogsize)


def extsetup(ui):
    wrapfilelog(filelog.filelog)

    wrapfunction = extensions.wrapfunction

    try:
        # Older Mercurial does not have wrapconvertsink. These methods are
        # "convert"-related and missing them is not fatal.  Ignore them so
        # bootstrapping from an old Mercurial works.
        wrapfunction(scmutil, "wrapconvertsink", wrapper.convertsink)
        wrapfunction(
            upgrade, "_finishdatamigration", wrapper.upgradefinishdatamigration
        )
        wrapfunction(upgrade, "preservedrequirements", wrapper.upgraderequirements)
        wrapfunction(upgrade, "supporteddestrequirements", wrapper.upgraderequirements)
    except AttributeError:
        pass

    wrapfunction(
        changegroup, "supportedoutgoingversions", wrapper.supportedoutgoingversions
    )
    wrapfunction(changegroup, "allsupportedversions", wrapper.allsupportedversions)

    wrapfunction(context.basefilectx, "cmp", wrapper.filectxcmp)
    wrapfunction(context.basefilectx, "isbinary", wrapper.filectxisbinary)
    context.basefilectx.islfs = wrapper.filectxislfs

    revlog.addflagprocessor(
        revlog.REVIDX_EXTSTORED,
        (wrapper.readfromstore, wrapper.writetostore, wrapper.bypasscheckhash),
    )

    wrapfunction(hg, "clone", wrapper.hgclone)
    wrapfunction(hg, "postshare", wrapper.hgpostshare)

    # Make bundle choose changegroup3 instead of changegroup2. This affects
    # "hg bundle" command. Note: it does not cover all bundle formats like
    # "packed1". Using "packed1" with lfs will likely cause trouble.
    names = [k for k, v in exchange._bundlespeccgversions.items() if v == "02"]
    for k in names:
        exchange._bundlespeccgversions[k] = "03"

    # bundlerepo uses "vfsmod.readonlyvfs(othervfs)", we need to make sure lfs
    # options and blob stores are passed from othervfs to the new readonlyvfs.
    wrapfunction(vfsmod.readonlyvfs, "__init__", wrapper.vfsinit)

    # when writing a bundle via "hg bundle" command, upload related LFS blobs
    wrapfunction(bundle2, "writenewbundle", wrapper.writenewbundle)

    # when "hg push" uses bundle2, upload related LFS blobs
    wrapfunction(exchange, "_pushbundle2", wrapper._pushbundle2)

    # verify LFS objects were uploaded when receiving pushes
    wrapfunction(changegroup, "checkrevs", wrapper.checkrevs)


@templatekeyword("lfs_files")
def lfsfiles(repo, ctx, **args):
    """List of strings. LFS files added or modified by the changeset."""
    pointers = wrapper.pointersfromctx(ctx)  # {path: pointer}
    return sorted(pointers.keys())


@command(
    "debuglfsupload", [("r", "rev", [], _("upload large files introduced by REV"))]
)
def debuglfsupload(ui, repo, **opts):
    """upload lfs blobs added by the working copy parent or given revisions"""
    revs = opts.get("rev", [])
    pointers = wrapper.extractpointers(repo, scmutil.revrange(repo, revs))
    wrapper.uploadblobs(repo, pointers)


# Ad-hoc commands to upload / download blobs without requiring an hg repo


def _adhocstores(ui, url):
    """return local and remote stores for ad-hoc (outside repo) uses"""
    if url is not None:
        ui.setconfig("lfs", "url", url)
    return blobstoremod.memlocal(), blobstore.remote(ui)


@command("debuglfssend", [], _("hg debuglfssend [URL]"), norepo=True)
def debuglfssend(ui, url=None):
    """read from stdin, send it as a single file to LFS server

    Print oid and size.
    """
    local, remote = _adhocstores(ui, url)

    data = ui.fin.read()
    oid = hashlib.sha256(data).hexdigest()
    longoid = "sha256:%s" % oid
    size = len(data)
    pointers = [pointer.gitlfspointer(oid=longoid, size=str(size))]

    local.write(oid, data)
    remote.writebatch(pointers, local)
    ui.write(("%s %s\n") % (oid, size))


@command(
    "debuglfsreceive|debuglfsrecv",
    [],
    _("hg debuglfsreceive OID SIZE [URL]"),
    norepo=True,
)
def debuglfsreceive(ui, oid, size, url=None):
    """receive a single object from LFS server, write it to stdout"""
    local, remote = _adhocstores(ui, url)

    longoid = "sha256:%s" % oid
    pointers = [pointer.gitlfspointer(oid=longoid, size=size)]
    remote.readbatch(pointers, local)

    ui.write((local.read(oid)))


@command(
    "debuglfsreceiveall|debuglfsrecvall",
    [],
    _("hg debuglfsreceiveall URL OID SIZE [OID SIZE]"),
    norepo=True,
)
def debuglfsreceiveall(ui, url, *objs):
    """receive a bunch of objects from LFS server, write them to stdout"""
    local, remote = _adhocstores(ui, url)

    if len(objs) == 0:
        raise error.Abort(_("Empty LFS objects list"))
    if len(objs) % 2 != 0:
        raise error.Abort(_("Every LFS object should have 2 fields"))

    lfsobjects = [("sha256:%s" % oid, size) for oid, size in zip(objs[::2], objs[1::2])]
    pointers = [
        pointer.gitlfspointer(oid=longoid, size=size) for longoid, size in lfsobjects
    ]
    remote.readbatch(pointers, local)

    for oid in objs[::2]:
        ui.write((local.read(oid)))


@command(
    "debuglfsdownload",
    [
        ("r", "rev", [], _("revision"), _("REV")),
        (
            "",
            "sparse",
            True,
            _("respect sparse profile, " "(otherwise check all files)"),
        ),
    ],
    _("hg debuglfsdownload -r REV1 -r REV2"),
    norepo=False,
)
def debuglfsdownload(ui, repo, *pats, **opts):
    """calculate the LFS download size when updating between REV1 and REV2

    If --no-sparse is provided, this operation would ignore any sparse
    profile that might be present and report data for the full checkout.

    With -v also prints which files are to be downloaded and the size of
    each file."""
    revs = opts.get("rev")

    node1, node2 = scmutil.revpair(repo, revs)
    match = lambda s: True
    if not opts.get("sparse"):
        ui.debug("will ignore sparse profile in this repo\n")
    else:
        if not util.safehasattr(repo, "sparsematch"):
            raise error.Abort(
                _("--ignore-sparse makes no sense in a non-sparse" " repository")
            )
        match = repo.sparsematch(node2)

    with ui.configoverride({("remotefilelog", "dolfsprefetch"): False}):
        ctx1, ctx2 = repo[node1], repo[node2]
        mfdiff = ctx2.manifest().diff(ctx1.manifest())
        lfsflogs = util.sortdict()  # LFS filelogs
        for fname in mfdiff:
            if not match(fname):
                continue
            flog = repo.file(fname)
            try:
                node = ctx2.filenode(fname)
            except error.ManifestLookupError:
                continue
            if wrapper._islfs(flog, node=node):
                lfsflogs[fname] = flog

        totalsize = 0
        presentsize = 0
        for fname, flog in lfsflogs.items():
            rawtext = flog.revision(ctx2.filenode(fname), raw=True)
            p = pointer.deserialize(rawtext)
            present = repo.svfs.lfslocalblobstore.has(p.oid())
            lfssize = int(p["size"])
            ui.note(_("%s: %i (present=%r)\n") % (fname, lfssize, present))
            totalsize += lfssize
            presentsize += lfssize if present else 0
        ui.status(
            _("Total size: %i, to download: %i, already exists: %r\n")
            % (totalsize, totalsize - presentsize, presentsize)
        )
