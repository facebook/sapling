# remotefilelog.py - extension for storing file contents remotely
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import fileserverclient, remoterevlog
from mercurial.node import bin, hex, nullid, nullrev
from mercurial.i18n import _
from mercurial.extensions import wrapfunction
from mercurial import ancestor, mdiff, parsers, error, util, dagutil, time
from mercurial import repair, extensions, filelog, revlog, wireproto, cmdutil
from mercurial import copies, traceback, store, context, changegroup, localrepo
from mercurial import commands, sshpeer, scmutil, dispatch, merge
import struct, zlib, errno, collections, time, os, pdb, socket, subprocess, lz4

shallowremote = False
localrepo.localrepository.supported.add('shallowrepo')

def uisetup(ui):
    entry = extensions.wrapcommand(commands.table, 'clone', cloneshallow)
    entry[1].append(('', 'shallow', None,
                     _("create a shallow clone which uses remote file history")))

def extsetup(ui):
    # the remote client communicates it's shallow capability via hello
    orig, args = wireproto.commands["hello"]
    def helloshallow(*args, **kwargs):
        global shallowremote
        shallowremote = True
        return orig(*args, **kwargs)
    wireproto.commands["hello_shallow"] = (helloshallow, args)

def cloneshallow(orig, ui, repo, *args, **opts):
    if opts.get('shallow'):
        addshallowcapability()
        def stream_in_shallow(orig, self, remote, requirements):
            # set up the client hooks so the post-clone update works
            setupclient(self.ui, self)

            requirements.add('shallowrepo')

            return orig(self, remote, requirements)
        wrapfunction(localrepo.localrepository, 'stream_in', stream_in_shallow)

    orig(ui, repo, *args, **opts)

def reposetup(ui, repo):
    if not repo.local():
        return

    isserverenabled = ui.configbool('remotefilelog', 'server')
    isshallowclient = "shallowrepo" in repo.requirements

    if isserverenabled and isshallowclient:
        raise Exception("Cannot be both a server and shallow client.")

    if isshallowclient:
        setupclient(ui, repo)

    if isserverenabled:
        # support file content requests
        wireproto.commands['getfiles'] = (getfiles, '')

    if isserverenabled or isshallowclient:
        # only put filelogs in changegroups when necessary
        def shouldaddfilegroups(source):
            if source == "push":
                return True
            if source == "serve":
                if isshallowclient:
                    # commits in a shallow repo may not exist in the master server
                    # so we need to return all the data on a pull
                    ui.warn("pulling from a shallow repo\n")
                    return True
                return not shallowremote

            return not isshallowclient

        def addfilegroups(orig, self):
            if shouldaddfilegroups(self.source):
                return orig(self)
            return []
        wrapfunction(changegroup.changegroupgen, 'addfilegroups', addfilegroups)

        # don't allow streaming clones from a shallow repo
        def stream(repo, proto):
            if isshallowclient:
                # don't allow cloning from a shallow repo since the cloned
                # repo would be unable to access local commits
                raise util.Abort(_("Cannot clone from a shallow repo."))

            return wireproto.stream(repo, proto)
        wireproto.commands['stream_out'] = (stream, '')

        # don't clone filelogs to shallow clients
        def _walkstreamfiles(orig, repo):
            if shallowremote:
                return repo.store.topfiles()
            return orig(repo)
        wrapfunction(wireproto, '_walkstreamfiles', _walkstreamfiles)

def setupclient(ui, repo):
    addshallowcapability();

    fileserverclient.client = fileserverclient.fileserverclient(ui)

    # replace filelog base class
    filelog.filelog.__bases__ = (remoterevlog.remoterevlog, )

    # prefetch files before update hook
    def applyupdates(orig, repo, actions, wctx, mctx, actx, overwrite):
        manifest = mctx.manifest()
        files = []
        for f, m, args, msg in [a for a in actions if a[1] == 'g']:
            files.append((f, hex(manifest[f])))
        # batch fetch the needed files from the server
        fileserverclient.client.prefetch(repo.sopener.vfs.base, files)
        return orig(repo, actions, wctx, mctx, actx, overwrite)
    wrapfunction(merge, 'applyupdates', applyupdates)

    # close connection
    def runcommand(orig, *args, **kwargs):
        try:
            return orig(*args, **kwargs)
        finally:
            fileserverclient.client.close()
    wrapfunction(dispatch, 'runcommand', runcommand)

    # disappointing hacks below

    # filelog & filectx
    def filelogsize(orig, self, node):
        if self.renamed(node):
            return len(self.read(node))
        return super(filelog.filelog, self).size(node)
    wrapfunction(filelog.filelog, 'size', filelogsize)

    def filectxsize(orig, self):
        return self._filelog.size(self._filenode)
    wrapfunction(context.filectx, 'size', filectxsize)

    wrapfunction(context.filectx, 'ancestors', ancestors)

    # prevent strip from considering filelogs
    def _collectbrokencsets(orig, repo, files, striprev):
        return orig(repo, [], striprev)
    wrapfunction(repair, '_collectbrokencsets', _collectbrokencsets)

    # tracing copies without rev numbers
    wrapfunction(copies, '_tracefile', tracefile)

    wrapfunction(copies, 'checkcopies', checkcopies)

    # changegroup creation
    changegroup.changegroupgen = partialchangegroupgen
    changegroup.subsetchangegroupgen.__bases__ = (partialchangegroupgen, )
    changegroup.bundle10.nodechunk = nodechunk

def getfiles(repo, proto):
    """A server api for requesting particular versions of particular files.
    """
    def streamer():
        fin = proto.fin
        opener = repo.sopener
        while True:
            request = fin.readline()[:-1]
            if not request:
                break

            node = request[:40]
            path = request[40:]
            try:
                temprevlog = revlog.revlog(opener, "data/" + path + ".i")

                text = temprevlog.revision(bin(node))
                p1, p2 = temprevlog.parents(bin(node))
                text = lz4.compressHC(p1 + p2 + text)
            except Exception, ex:
                text = ""

            yield '%d\n%s' % (len(text), text)

            # it would be better to only flush after processing a whole batch
            # but currently we don't know if there are more requests coming
            proto.fout.flush()

    return wireproto.streamres(streamer())

def addshallowcapability():
    def callstream(orig, self, cmd, **args):
        if cmd == 'hello':
            cmd += '_shallow'
        return orig(self, cmd, **args)
    wrapfunction(sshpeer.sshpeer, '_callstream', callstream)

def ancestors(orig, self, followfirst=False):
    visit = {}
    c = self
    cut = followfirst and 1 or None
    queue = []
    while True:
        for parent in c.parents()[:cut]:
            queue.append(parent)
        if not queue:
            break
        c = queue.pop(0)
        yield c

def tracefile(orig, fctx, actx):
    '''return file context that is the ancestor of fctx present in actx'''
    am = actx.manifest()
    for f in fctx.ancestors():
        if am.get(f.path(), None) == f.filenode():
            return f

def checkcopies(orig, ctx, f, m1, m2, ca, limit, diverge, copy, fullcopy):
    '''check possible copies of f from m1 to m2'''

    ma = ca.manifest()

    def related(f1, f2):
        # Walk back to common ancestor to see if the two files originate
        # from the same file.

        if f1 == f2:
            return f1 # a match

        g1, g2 = f1.ancestors(), f2.ancestors()

        seen1 = set()
        seen2 = set()
        seen1.add(f1.filenode())
        seen2.add(f2.filenode())
        while g1 != None or g2 != None:
            if g1 != None:
                try:
                    f1 = g1.next()
                    if f1.filenode() in seen2:
                        return f1
                    seen1.add(f1.filenode())
                except StopIteration:
                    g1 = None
                    if not seen1:
                        return False

            if g2 != None:
                try:
                    f2 = g2.next()
                    if f2.filenode() in seen1:
                        return f2
                    seen2.add(f2.filenode())
                except StopIteration:
                    g2 = None
                    if not seen2:
                        return False

        return False

    of = None
    seen = set([f])
    latestmaof = ma.get(f)
    for oc in ctx(f, m1[f]).ancestors():
        of = oc.path()
        if of in seen:
            # check limit late - grab last rename before
            # break if we reach common ancestor
            if latestmaof == oc.filenode():
                break
            continue
        seen.add(of)

        fullcopy[f] = of # remember for dir rename detection
        if of not in m2:
            continue # no match, keep looking
        latestmaof = ma.get(of)
        if m2[of] == latestmaof:
            break # no merge needed, quit early
        c2 = ctx(of, m2[of])
        cr = related(oc, c2)
        if cr and (of == f or of == c2.path()): # non-divergent
            copy[f] = of
            of = None
            break

    if of in ma:
        diverge.setdefault(of, []).append(f)

def nodechunk(self, revlog, node, prev):
    prefix = ''
    if prev == nullrev:
        delta = revlog.revision(node)
        prefix = mdiff.trivialdiffheader(len(delta))
    else:
        delta = revlog.revdiff(prev, node)
    linknode = self._lookup(revlog, node)
    p1, p2 = revlog.parents(node)
    meta = self.builddeltaheader(node, p1, p2, prev, linknode)
    meta += prefix
    l = len(meta) + len(delta)
    yield changegroup.chunkheader(l)
    yield meta
    yield delta

class partialchangegroupgen(changegroup.changegroupgen):
    def __init__(self, repo, csets, source, reorder):
        super(partialchangegroupgen, self).__init__(repo, csets, source, reorder)
        self.changedfilenodes = {}
        self.filecommitmap = {}

    def lookupmanifest(self, node):
        self.count[0] += 1
        self.progress(changegroup._bundling, self.count[0],
                 unit=changegroup._manifests, total=self.count[1])

        cl = self.cl
        mf = self.mf
        changedfilenodes = self.changedfilenodes
        filecommitmap = self.filecommitmap

        clnode = cl.node(mf.linkrev(mf.rev(node)))
        clfiles = set(cl.read(clnode)[3])
        for f, n in mf.readfast(node).iteritems():
            if f in clfiles:
                filenodes = changedfilenodes.setdefault(f, set())
                filenodes.add(n)
                if not (f, n) in filecommitmap:
                    filecommitmap[(f, n)] = clnode

        return clnode

    def outgoingfilemap(self, filerevlog, fname):
        # map of outgoing file nodes to changelog nodes
        fnodes = self.changedfilenodes.get(fname, set())
        filecommitmap = self.filecommitmap

        mapping = {}
        for fnode in fnodes:
            clnode = filecommitmap.get((fname, fnode), None)
            if clnode:
                mapping[fnode] = clnode
        return mapping
