# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# debugcommands.py - debug logic for remotefilelog
from __future__ import absolute_import

import hashlib
import os
import sys

from bindings import revisionstore
from edenscm.hgext import extutil
from edenscm.mercurial import error, filelog, progress, revlog, util
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import bin, hex, nullid, short

from . import (
    constants,
    edenapi,
    fileserverclient,
    historypack,
    shallowrepo,
    shallowutil,
)
from .contentstore import unioncontentstore
from .lz4wrapper import lz4decompress
from .repack import repacklockvfs


def debugremotefilelog(ui, path, **opts):
    decompress = opts.get("decompress")

    size, firstnode, mapping = parsefileblob(path, decompress)
    filename = None
    filenamepath = os.path.join(os.path.dirname(path), "filename")
    if os.path.exists(filenamepath):
        with open(filenamepath, "rb") as f:
            filename = f.read()

    ui.status(_("size: %s bytes\n") % (size))
    ui.status(_("path: %s \n") % (path))
    ui.status(_("key: %s \n") % (short(firstnode)))
    if filename is not None:
        ui.status(_("filename: %s \n") % filename)
    ui.status(_("\n"))
    ui.status(
        _("%12s => %12s %13s %13s %12s\n")
        % ("node", "p1", "p2", "linknode", "copyfrom")
    )

    queue = [firstnode]
    while queue:
        node = queue.pop(0)
        p1, p2, linknode, copyfrom = mapping[node]
        ui.status(
            _("%s => %s  %s  %s  %s\n")
            % (short(node), short(p1), short(p2), short(linknode), copyfrom)
        )
        if p1 != nullid:
            queue.append(p1)
        if p2 != nullid:
            queue.append(p2)


def buildtemprevlog(repo, file):
    # get filename key
    filekey = hashlib.sha1(file).hexdigest()
    filedir = os.path.join(repo.path, "store/data", filekey)

    # sort all entries based on linkrev
    fctxs = []
    for filenode in os.listdir(filedir):
        if "_old" not in filenode:
            fctxs.append(repo.filectx(file, fileid=bin(filenode)))

    fctxs = sorted(fctxs, key=lambda x: x.linkrev())

    # add to revlog
    temppath = repo.sjoin("data/temprevlog.i")
    if os.path.exists(temppath):
        os.remove(temppath)
    r = filelog.filelog(repo.svfs, "temprevlog")

    class faket(object):
        def add(self, a, b, c):
            pass

    t = faket()
    for fctx in fctxs:
        if fctx.node() not in repo:
            continue

        p = fctx.filelog().parents(fctx.filenode())
        meta = {}
        if fctx.renamed():
            meta["copy"] = fctx.renamed()[0]
            meta["copyrev"] = hex(fctx.renamed()[1])

        r.add(fctx.data(), meta, t, fctx.linkrev(), p[0], p[1])

    return r


def debugindex(orig, ui, repo, file_=None, **opts):
    """dump the contents of an index file"""
    if (
        opts.get("changelog")
        or opts.get("manifest")
        or opts.get("dir")
        or not shallowrepo.requirement in repo.requirements
        or not repo.shallowmatch(file_)
    ):
        return orig(ui, repo, file_, **opts)

    r = buildtemprevlog(repo, file_)

    # debugindex like normal
    format = opts.get("format", 0)
    if format not in (0, 1):
        raise error.Abort(_("unknown format %d") % format)

    generaldelta = r.version & revlog.FLAG_GENERALDELTA
    if generaldelta:
        basehdr = " delta"
    else:
        basehdr = "  base"

    if format == 0:
        ui.write(
            (
                "   rev    offset  length " + basehdr + " linkrev"
                " nodeid       p1           p2\n"
            )
        )
    elif format == 1:
        ui.write(
            (
                "   rev flag   offset   length"
                "     size " + basehdr + "   link     p1     p2"
                "       nodeid\n"
            )
        )

    for i in r:
        node = r.node(i)
        if generaldelta:
            base = r.deltaparent(i)
        else:
            base = r.chainbase(i)
        if format == 0:
            try:
                pp = r.parents(node)
            except Exception:
                pp = [nullid, nullid]
            ui.write(
                "% 6d % 9d % 7d % 6d % 7d %s %s %s\n"
                % (
                    i,
                    r.start(i),
                    r.length(i),
                    base,
                    r.linkrev(i),
                    short(node),
                    short(pp[0]),
                    short(pp[1]),
                )
            )
        elif format == 1:
            pr = r.parentrevs(i)
            ui.write(
                "% 6d %04x % 8d % 8d % 8d % 6d % 6d % 6d % 6d %s\n"
                % (
                    i,
                    r.flags(i),
                    r.start(i),
                    r.length(i),
                    r.rawsize(i),
                    base,
                    r.linkrev(i),
                    pr[0],
                    pr[1],
                    short(node),
                )
            )


def debugindexdot(orig, ui, repo, file_):
    """dump an index DAG as a graphviz dot file"""
    if not shallowrepo.requirement in repo.requirements:
        return orig(ui, repo, file_)

    r = buildtemprevlog(repo, os.path.basename(file_)[:-2])

    ui.write(("digraph G {\n"))
    for i in r:
        node = r.node(i)
        pp = r.parents(node)
        ui.write("\t%d -> %d\n" % (r.rev(pp[0]), i))
        if pp[1] != nullid:
            ui.write("\t%d -> %d\n" % (r.rev(pp[1]), i))
    ui.write("}\n")


def verifyremotefilelog(ui, path, **opts):
    decompress = opts.get("decompress")

    for root, dirs, files in os.walk(path):
        for file in files:
            if file == "repos":
                continue
            filepath = os.path.join(root, file)
            size, firstnode, mapping = parsefileblob(filepath, decompress)
            for p1, p2, linknode, copyfrom in mapping.itervalues():
                if linknode == nullid:
                    actualpath = os.path.relpath(root, path)
                    key = fileserverclient.getcachekey("reponame", actualpath, file)
                    ui.status("%s %s\n" % (key, os.path.relpath(filepath, path)))


def parsefileblob(path, decompress):
    raw = None
    f = open(path, "r")
    try:
        raw = f.read()
    finally:
        f.close()

    if decompress:
        raw = lz4decompress(raw)

    offset, size, flags = shallowutil.parsesizeflags(raw)
    start = offset + size

    firstnode = None

    mapping = {}
    while start < len(raw):
        divider = raw.index("\0", start + 80)

        currentnode = raw[start : (start + 20)]
        if not firstnode:
            firstnode = currentnode

        p1 = raw[(start + 20) : (start + 40)]
        p2 = raw[(start + 40) : (start + 60)]
        linknode = raw[(start + 60) : (start + 80)]
        copyfrom = raw[(start + 80) : divider]

        mapping[currentnode] = (p1, p2, linknode, copyfrom)
        start = divider + 1

    return size, firstnode, mapping


def debugdatastore(ui, store, verifynoduplicate=True, **opts):
    nodedelta = opts.get("node_delta")
    if nodedelta:
        deltachain = store.getdeltachain("", bin(nodedelta))
        dumpdeltachain(ui, deltachain, **opts)
        return
    node = opts.get("node")
    if node:
        unionstore = unioncontentstore(store)
        try:
            content = unionstore.get("", bin(node))
        except KeyError:
            ui.write(("(not found)\n"))
            return
        else:
            ui.write(("%s") % content)
            return

    if opts.get("long"):
        hashformatter = hex
        hashlen = 42
    else:
        hashformatter = short
        hashlen = 14

    lastfilename = None
    totaldeltasize = 0
    totalblobsize = 0

    def printtotals():
        if lastfilename is not None:
            ui.write("\n")
        if not totaldeltasize or not totalblobsize:
            return
        difference = totalblobsize - totaldeltasize
        deltastr = "%0.1f%% %s" % (
            (100.0 * abs(difference) / totalblobsize),
            ("smaller" if difference > 0 else "bigger"),
        )

        ui.write(
            ("Total:%s%s  %s (%s)\n")
            % (
                "".ljust(2 * hashlen - len("Total:")),
                str(totaldeltasize).ljust(12),
                str(totalblobsize).ljust(9),
                deltastr,
            )
        )

    bases = {}
    nodes = set()
    failures = 0
    for filename, node, deltabase, deltalen in store.iterentries():
        bases[node] = deltabase
        if verifynoduplicate and node in nodes:
            ui.write(("Bad entry: %s appears twice\n" % short(node)))
            failures += 1
        nodes.add(node)
        if filename != lastfilename:
            printtotals()
            name = "(empty name)" if filename == "" else filename
            ui.write("%s:\n" % name)
            ui.write(
                "%s%s%s%s\n"
                % (
                    "Node".ljust(hashlen),
                    "Delta Base".ljust(hashlen),
                    "Delta Length".ljust(14),
                    "Blob Size".ljust(9),
                )
            )
            lastfilename = filename
            totalblobsize = 0
            totaldeltasize = 0

        # Metadata could be missing, in which case it will be an empty dict.
        meta = store.getmeta(filename, node)
        if constants.METAKEYSIZE in meta:
            blobsize = meta[constants.METAKEYSIZE]
            totaldeltasize += deltalen
            totalblobsize += blobsize
        else:
            blobsize = "(missing)"
        ui.write(
            "%s  %s  %s%s\n"
            % (
                hashformatter(node),
                hashformatter(deltabase),
                str(deltalen).ljust(14),
                blobsize,
            )
        )

    if filename is not None:
        printtotals()

    failures += _sanitycheck(ui, set(nodes), bases)
    if failures > 1:
        ui.warn(("%d failures\n" % failures))
        return 1


def debugdatapack(ui, *paths, **opts):
    for path in paths:
        if ".data" in path:
            path = path[: path.index(".data")]
        ui.write("%s:\n" % path)
        dpack = revisionstore.datapack(path)
        debugdatastore(ui, dpack, **opts)


def debugindexedlogdatastore(ui, *paths, **opts):
    for path in paths:
        ui.write("%s:\n" % path)
        store = revisionstore.indexedlogdatastore(path)
        debugdatastore(ui, store, verifynoduplicate=False, **opts)


def _sanitycheck(ui, nodes, bases):
    """
    Does some basic sanity checking on a packfiles with ``nodes`` ``bases`` (a
    mapping of node->base):

    - Each deltabase must itself be a node elsewhere in the pack
    - There must be no cycles
    """
    failures = 0
    for node in nodes:
        seen = set()
        current = node
        deltabase = bases[current]

        while deltabase != nullid:
            if deltabase not in nodes:
                ui.warn(
                    (
                        "Bad entry: %s has an unknown deltabase (%s)\n"
                        % (short(node), short(deltabase))
                    )
                )
                failures += 1
                break

            if deltabase in seen:
                ui.warn(
                    (
                        "Bad entry: %s has a cycle (at %s)\n"
                        % (short(node), short(deltabase))
                    )
                )
                failures += 1
                break

            current = deltabase
            seen.add(current)
            deltabase = bases[current]
        # Since ``node`` begins a valid chain, reset/memoize its base to nullid
        # so we don't traverse it again.
        bases[node] = nullid
    return failures


def dumpdeltachain(ui, deltachain, **opts):
    hashformatter = hex
    hashlen = 40

    lastfilename = None
    for filename, node, filename, deltabasenode, delta in deltachain:
        if filename != lastfilename:
            ui.write("\n%s\n" % filename)
            lastfilename = filename
        ui.write(
            "%s  %s  %s  %s\n"
            % (
                "Node".ljust(hashlen),
                "Delta Base".ljust(hashlen),
                "Delta SHA1".ljust(hashlen),
                "Delta Length".ljust(6),
            )
        )

        ui.write(
            "%s  %s  %s  %s\n"
            % (
                hashformatter(node),
                hashformatter(deltabasenode),
                hashlib.sha1(delta).hexdigest(),
                len(delta),
            )
        )


def debughistorystore(ui, store, **opts):
    if opts.get("long"):
        hashformatter = hex
        hashlen = 42
    else:
        hashformatter = short
        hashlen = 14

    lastfilename = None
    for entry in store.iterentries():
        filename, node, p1node, p2node, linknode, copyfrom = entry
        if filename != lastfilename:
            ui.write("\n%s\n" % filename)
            ui.write(
                "%s%s%s%s%s\n"
                % (
                    "Node".ljust(hashlen),
                    "P1 Node".ljust(hashlen),
                    "P2 Node".ljust(hashlen),
                    "Link Node".ljust(hashlen),
                    "Copy From",
                )
            )
            lastfilename = filename
        ui.write(
            "%s  %s  %s  %s  %s\n"
            % (
                hashformatter(node),
                hashformatter(p1node),
                hashformatter(p2node),
                hashformatter(linknode),
                copyfrom,
            )
        )


def debughistorypack(ui, paths, **opts):
    for path in paths:
        if ".hist" in path:
            path = path[: path.index(".hist")]
        hpack = revisionstore.historypack(path)
        debughistorystore(ui, hpack, **opts)


def debugindexedloghistorystore(ui, paths, **opts):
    for path in paths:
        store = revisionstore.indexedloghistorystore(path)
        debughistorystore(ui, store, **opts)


def debugwaitonrepack(repo):
    with extutil.flock(repacklockvfs(repo).join("repacklock"), ""):
        return


def debugwaitonprefetch(repo):
    with repo._lock(
        repo.svfs,
        "prefetchlock",
        True,
        None,
        None,
        _("prefetching in %s") % repo.origroot,
    ):
        pass


def debuggetfiles(ui, repo, **opts):
    edenapi.bailifdisabled(ui)

    input = (line.split() for line in sys.stdin.readlines())
    keys = [(path, node) for node, path in input]

    dpack, __ = repo.fileslog.getmutablesharedpacks()

    msg = _("fetching %d files") % len(keys)
    with progress.bar(
        ui, msg, start=0, unit=_("bytes"), formatfunc=util.bytecount
    ) as prog:

        def progcb(dl, dlt, ul, ult):
            if dl > 0:
                prog._total = dlt
                prog.value = dl

        stats = repo.edenapi.get_files(keys, dpack, progcb)

    ui.write(stats.to_str() + "\n")

    packpath, __ = repo.fileslog._mutablesharedpacks.commit()
    ui.write(_("wrote datapack: %s\n") % packpath)


def debugserialgetfiles(ui, repo, **opts):
    edenapi.bailifdisabled(ui)

    input = (line.split() for line in sys.stdin.readlines())
    keys = [(path, node) for node, path in input]

    dpack, __ = repo.fileslog.getmutablesharedpacks()

    start = util.timer()
    for key in keys:
        stats = repo.edenapi.get_files([key], dpack)
        ui.write(stats.to_str() + "\n")
    end = util.timer()

    ui.write(_("elapsed time: %f s\n") % (end - start))

    packpath, __ = repo.fileslog._mutablesharedpacks.commit()
    ui.write(_("wrote datapack: %s\n") % packpath)


def debuggethistory(ui, repo, **opts):
    edenapi.bailifdisabled(ui)

    input = (line.split() for line in sys.stdin.readlines())
    keys = [(path, node) for node, path in input]
    depth = opts.get("depth") or None

    __, hpack = repo.fileslog.getmutablesharedpacks()

    msg = _("fetching history for %d files") % len(keys)
    with progress.bar(
        ui, msg, start=0, unit=_("bytes"), formatfunc=util.bytecount
    ) as prog:

        def progcb(dl, dlt, ul, ult):
            if dl > 0:
                prog._total = dlt
                prog.value = dl

        stats = repo.edenapi.get_history(keys, hpack, depth, progcb)

    ui.write(stats.to_str() + "\n")

    __, packpath = repo.fileslog._mutablesharedpacks.commit()
    ui.write(_("wrote historypack: %s\n") % packpath)


def debuggettrees(ui, repo, **opts):
    edenapi.bailifdisabled(ui)

    keys = []
    for line in sys.stdin.readlines():
        parts = line.split()
        (node, path) = parts if len(parts) > 1 else (parts[0], "")
        keys.append((path, node))

    dpack, __ = repo.manifestlog.getmutablesharedpacks()

    msg = _("fetching %d trees") % len(keys)
    with progress.bar(
        ui, msg, start=0, unit=_("bytes"), formatfunc=util.bytecount
    ) as prog:

        def progcb(dl, dlt, ul, ult):
            if dl > 0:
                prog._total = dlt
                prog.value = dl

        stats = repo.edenapi.get_trees(keys, dpack, progcb)

    ui.write(stats.to_str() + "\n")

    packpath, __ = repo.manifestlog._mutablesharedpacks.commit()
    ui.write(_("wrote datapack: %s\n") % packpath)
