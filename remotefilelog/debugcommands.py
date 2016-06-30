# debugcommands.py - debug logic for remotefilelog
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import filelog, revlog
from mercurial.node import bin, hex, nullid, nullrev, short
from mercurial.i18n import _
import datapack, historypack, shallowrepo
import hashlib, os, lz4

def debugremotefilelog(ui, path, **opts):
    decompress = opts.get('decompress')

    size, firstnode, mapping = parsefileblob(path, decompress)

    ui.status(_("size: %s bytes\n") % (size))
    ui.status(_("path: %s \n") % (path))
    ui.status(_("key: %s \n") % (short(firstnode)))
    ui.status(_("\n"))
    ui.status(_("%12s => %12s %13s %13s %12s\n") %
              ("node", "p1", "p2", "linknode", "copyfrom"))

    queue = [firstnode]
    while queue:
        node = queue.pop(0)
        p1, p2, linknode, copyfrom = mapping[node]
        ui.status(_("%s => %s  %s  %s  %s\n") %
            (short(node), short(p1), short(p2), short(linknode), copyfrom))
        if p1 != nullid:
            queue.append(p1)
        if p2 != nullid:
            queue.append(p2)

def buildtemprevlog(repo, file):
    # get filename key
    filekey = hashlib.sha1(file).hexdigest()
    filedir = os.path.join(repo.path, 'store/data', filekey)

    # sort all entries based on linkrev
    fctxs = []
    for filenode in os.listdir(filedir):
        fctxs.append(repo.filectx(file, fileid=bin(filenode)))

    fctxs = sorted(fctxs, key=lambda x: x.linkrev())

    # add to revlog
    temppath = repo.sjoin('data/temprevlog.i')
    if os.path.exists(temppath):
        os.remove(temppath)
    r = filelog.filelog(repo.svfs, 'temprevlog')

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
            meta['copy'] = fctx.renamed()[0]
            meta['copyrev'] = hex(fctx.renamed()[1])

        r.add(fctx.data(), meta, t, fctx.linkrev(), p[0], p[1])

    return r

def debugindex(orig, ui, repo, file_=None, **opts):
    """dump the contents of an index file"""
    if (opts.get('changelog') or
        opts.get('manifest') or
        opts.get('dir') or
        not shallowrepo.requirement in repo.requirements or
        not repo.shallowmatch(file_)):
        return orig(ui, repo, file_, **opts)

    r = buildtemprevlog(repo, file_)

    # debugindex like normal
    format = opts.get('format', 0)
    if format not in (0, 1):
        raise error.Abort(_("unknown format %d") % format)

    generaldelta = r.version & revlog.REVLOGGENERALDELTA
    if generaldelta:
        basehdr = ' delta'
    else:
        basehdr = '  base'

    if format == 0:
        ui.write("   rev    offset  length " + basehdr + " linkrev"
                 " nodeid       p1           p2\n")
    elif format == 1:
        ui.write("   rev flag   offset   length"
                 "     size " + basehdr + "   link     p1     p2"
                 "       nodeid\n")

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
            ui.write("% 6d % 9d % 7d % 6d % 7d %s %s %s\n" % (
                    i, r.start(i), r.length(i), base, r.linkrev(i),
                    short(node), short(pp[0]), short(pp[1])))
        elif format == 1:
            pr = r.parentrevs(i)
            ui.write("% 6d %04x % 8d % 8d % 8d % 6d % 6d % 6d % 6d %s\n" % (
                    i, r.flags(i), r.start(i), r.length(i), r.rawsize(i),
                    base, r.linkrev(i), pr[0], pr[1], short(node)))

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
    decompress = opts.get('decompress')

    for root, dirs, files in os.walk(path):
        for file in files:
            if file == "repos":
                continue
            filepath = os.path.join(root, file)
            size, firstnode, mapping = parsefileblob(filepath, decompress)
            for p1, p2, linknode, copyfrom in mapping.itervalues():
                if linknode == nullid:
                    actualpath = os.path.relpath(root, path)
                    key = fileserverclient.getcachekey(repo.name, actualpath,
                                                       file)
                    ui.status("%s %s\n" % (key, os.path.relpath(filepath,
                                                                path)))

def parsefileblob(path, decompress):
    raw = None
    f = open(path, "r")
    try:
        raw = f.read()
    finally:
        f.close()

    if decompress:
        raw = lz4.decompress(raw)

    index = raw.index('\0')
    size = int(raw[:index])
    data = raw[(index + 1):(index + 1 + size)]
    start = index + 1 + size

    firstnode = None

    mapping = {}
    while start < len(raw):
        divider = raw.index('\0', start + 80)

        currentnode = raw[start:(start + 20)]
        if not firstnode:
            firstnode = currentnode

        p1 = raw[(start + 20):(start + 40)]
        p2 = raw[(start + 40):(start + 60)]
        linknode = raw[(start + 60):(start + 80)]
        copyfrom = raw[(start + 80):divider]

        mapping[currentnode] = (p1, p2, linknode, copyfrom)
        start = divider + 1

    return size, firstnode, mapping

def debugdatapack(ui, path):
    dpack = datapack.datapack(path)

    lastfilename = None
    for filename, node, deltabase, deltalen in dpack.iterentries():
        if filename != lastfilename:
            ui.write("\n%s\n" % filename)
            ui.write("%s%s%s\n" % (
                "Node".ljust(14),
                "Delta Base".ljust(14),
                "Delta Length".ljust(6)))
            lastfilename = filename
        ui.write("%s  %s  %s\n" % (short(node), short(deltabase), deltalen))

def debughistorypack(ui, path):
    hpack = historypack.historypack(path)

    lastfilename = None
    for entry in hpack.iterentries():
        filename, node, p1node, p2node, linknode, copyfrom = entry
        if filename != lastfilename:
            ui.write("\n%s\n" % filename)
            ui.write("%s%s%s%s%s\n" % (
                "Node".ljust(14),
                "P1 Node".ljust(14),
                "P2 Node".ljust(14),
                "Link Node".ljust(14),
                "Copy From"))
            lastfilename = filename
        ui.write("%s  %s  %s  %s  %s\n" % (short(node), short(p1node),
            short(p2node), short(linknode), copyfrom))

def debugwaitonrepack(repo):
    with repo._lock(repo.svfs, "repacklock", True, None,
                         None, _('repacking %s') % repo.origroot):
        pass
