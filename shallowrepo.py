# remotefilelog.py - extension for storing file contents remotely
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import fileserverclient, remotefilelog, remotefilectx
from mercurial.node import bin, hex, nullid, nullrev
from mercurial.i18n import _
from mercurial.extensions import wrapfunction
from mercurial import ancestor, mdiff, parsers, error, util, dagutil, time
from mercurial import repair, extensions, filelog, revlog, wireproto, cmdutil
from mercurial import copies, traceback, store, context, changegroup, localrepo
from mercurial import commands, sshpeer, scmutil, dispatch, merge, context, changelog
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

    # replace filelog & filectx
    filelog.filelog = remotefilelog.remotefilelog
    context.filectx = remotefilectx.remotefilectx

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

    # prevent strip from considering filelogs
    def _collectbrokencsets(orig, repo, files, striprev):
        return orig(repo, [], striprev)
    wrapfunction(repair, '_collectbrokencsets', _collectbrokencsets)

    # changegroup creation
    changegroup.changegroupgen = partialchangegroupgen
    changegroup.subsetchangegroupgen.__bases__ = (partialchangegroupgen, )
    changegroup.bundle10.nodechunk = nodechunk

    # adding changegroup files to the repo
    wrapfunction(localrepo.localrepository, 'addchangegroupfiles', addchangegroupfiles)

    pendingfilecommits = []
    def add(orig, self, text, meta, transaction, link, p1, p2):
        if isinstance(link, int):
            pendingfilecommits.append((self, text, meta, transaction, link, p1, p2))

            hashtext = remotefilelog._createrevlogtext(text, meta.get('copy'), meta.get('copyrev'))
            node = revlog.hash(hashtext, p1, p2)
            return node
        else:
            return orig(self, text, meta, transaction, link, p1, p2)
    wrapfunction(remotefilelog.remotefilelog, 'add', add)

    def changelogadd(orig, self, *args):
        node = orig(self, *args)
        for oldargs in pendingfilecommits:
            log, text, meta, transaction, link, p1, p2 = oldargs
            linknode = self.node(link)
            if linknode == node:
                log.add(text, meta, transaction, linknode, p1, p2)

        del pendingfilecommits[0:len(pendingfilecommits)]
        return node
    wrapfunction(changelog.changelog, 'add', changelogadd)

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

            node = bin(request[:40])
            if node == nullid:
                yield '0\n'
                continue

            path = request[40:]
            try:
                filectx = repo.filectx(path, fileid=node)

                text = filectx.data()

                ancestors = [filectx]
                ancestors.extend([f for f in filectx.ancestors()])

                ancestortext = ""
                for ancestorctx in ancestors:
                    parents = ancestorctx.parents()
                    p1 = nullid
                    p2 = nullid
                    if len(parents) > 0:
                        p1 = parents[0].filenode()
                    if len(parents) > 1:
                        p2 = parents[1].filenode()

                    copyname = ""
                    rename = ancestorctx.renamed()
                    if rename:
                        copyname = rename[0]
                    ancestortext += "%s%s%s%s%s\0" % (
                        ancestorctx.filenode(), p1, p2, ancestorctx.node(),
                        copyname)

                text = lz4.compressHC("%d\0%s%s" %
                    (len(text), text, ancestortext))

            except Exception, ex:
                raise ex
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

def addchangegroupfiles(orig, self, source, revmap, trp, pr, needfiles):
    revisions = 0
    files = 0
    while True:
        chunkdata = source.filelogheader()
        if not chunkdata:
            break
        f = chunkdata["filename"]
        self.ui.debug("adding %s revisions\n" % f)
        pr()
        files += 1
        fl = self.file(f)
        if not fl.addgroup(source, revmap, trp):
            raise util.Abort(_("received file revlog group is empty"))
        files += 1

    self.ui.progress(_('files'), None)

    return revisions, files
