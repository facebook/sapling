# __init__.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

testedwith = 'internal'

import fileserverclient, remotefilelog, remotefilectx, shallowstore, shallowrepo
import shallowbundle
from mercurial.node import bin, hex, nullid, nullrev, short
from mercurial.i18n import _
from mercurial.extensions import wrapfunction
from mercurial import ancestor, mdiff, parsers, error, util, dagutil, time
from mercurial import repair, extensions, filelog, revlog, wireproto, cmdutil
from mercurial import copies, traceback, store, context, changegroup, localrepo
from mercurial import commands, sshpeer, scmutil, dispatch, merge, context, changelog
from mercurial import templatekw, repoview, bundlerepo, revset
from mercurial import match as matchmod
import struct, zlib, errno, collections, time, os, pdb, socket, subprocess, lz4
import stat

shallowremote = False
localrepo.localrepository.supported.add('shallowrepo')

def uisetup(ui):
    entry = extensions.wrapcommand(commands.table, 'clone', cloneshallow)
    entry[1].append(('', 'shallow', None,
                     _("create a shallow clone which uses remote file history")))

    extensions.wrapcommand(commands.table, 'debugindex', debugindex)
    extensions.wrapcommand(commands.table, 'debugindexdot', debugindexdot)

def extsetup(ui):
    # the remote client communicates it's shallow capability via hello
    orig, args = wireproto.commands["hello"]
    def helloshallow(*args, **kwargs):
        global shallowremote
        shallowremote = True
        shallowbundle.shallowremote = True
        return orig(*args, **kwargs)
    wireproto.commands["hello_shallow"] = (helloshallow, args)

def cloneshallow(orig, ui, repo, *args, **opts):
    if opts.get('shallow'):
        addshallowcapability()
        def stream_in_shallow(orig, self, remote, requirements):
            # set up the client hooks so the post-clone update works
            setupclient(self.ui, self.unfiltered())

            # setupclient fixed the class on the repo itself
            # but we also need to fix it on the repoview
            if isinstance(self, repoview.repoview):
                self.__class__.__bases__ = (self.__class__.__bases__[0],
                                            self.unfiltered().__class__)

            requirements.add('shallowrepo')

            # if the repo was filtered, we need to refilter since
            # the class has changed
            return orig(self, remote, requirements)
        wrapfunction(localrepo.localrepository, 'stream_in', stream_in_shallow)

    try:
        orig(ui, repo, *args, **opts)
    finally:
        if opts.get('shallow'):
            fileserverclient.client.close()

def reposetup(ui, repo):
    if not isinstance(repo, localrepo.localrepository):
        return

    isserverenabled = ui.configbool('remotefilelog', 'server')
    isshallowclient = "shallowrepo" in repo.requirements

    if isserverenabled and isshallowclient:
        raise Exception("Cannot be both a server and shallow client.")

    if isshallowclient:
        setupclient(ui, repo)

    
    # support file content requests
    wireproto.commands['getfiles'] = (getfiles, '')

    if isserverenabled or isshallowclient:
        # don't clone filelogs to shallow clients
        def _walkstreamfiles(orig, repo):
            if shallowremote:
                # if we are shallow ourselves, stream our local commits
                if isshallowclient:
                    striplen = len(repo.store.path) + 1
                    readdir = repo.store.rawvfs.readdir
                    visit = [os.path.join(repo.store.path, 'data')]
                    while visit:
                        p = visit.pop()
                        for f, kind, st in readdir(p, stat=True):
                            fp = p + '/' + f
                            if kind == stat.S_IFREG:
                                n = util.pconvert(fp[striplen:])
                                yield (store.decodedir(n), n, st.st_size)
                            if kind == stat.S_IFDIR:
                                visit.append(fp)

                for x in repo.store.topfiles():
                    yield x
            elif isshallowclient:
                # don't allow cloning from a shallow repo to a full repo
                # since it would require fetching every version of every
                # file in order to create the revlogs.
                raise util.Abort(_("Cannot clone from a shallow repo "
                                 + "to a full repo."))
            else:
                for x in orig(repo):
                    yield x

        wrapfunction(wireproto, '_walkstreamfiles', _walkstreamfiles)

clientsetup = False
def setupclient(ui, repo):
    if (not isinstance(repo, localrepo.localrepository) or
        isinstance(repo, bundlerepo.bundlerepository)):
        return

    shallowrepo.wraprepo(repo)
    repo.store = shallowstore.wrapstore(repo.store)

    # one time setup below
    global clientsetup
    if clientsetup:
        return
    clientsetup = True

    addshallowcapability();

    fileserverclient.client = fileserverclient.fileserverclient(ui)

    changegroup.bundle10 = shallowbundle.shallowbundle

    def storewrapper(orig, requirements, path, vfstype):
        s = orig(requirements, path, vfstype)
        if 'shallowrepo' in requirements:
            s = shallowstore.wrapstore(s)

        return s
    wrapfunction(store, 'store', storewrapper)

    # prefetch files before update hook
    def applyupdates(orig, repo, actions, wctx, mctx, actx, overwrite):
        if 'shallowrepo' in repo.requirements:
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
    templatekw.getrenamedfn = getrenamedfn

    wrapfunction(cmdutil, 'walkfilerevs', walkfilerevs)

    # prevent strip from considering filelogs
    def _collectbrokencsets(orig, repo, files, striprev):
        if 'shallowrepo' in repo.requirements:
            files = []
        return orig(repo, files, striprev)
    wrapfunction(repair, '_collectbrokencsets', _collectbrokencsets)

    # hold on to filelogs until we know the commit hash
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

    # changectx wrappers
    def filectx(orig, self, path, fileid=None, filelog=None):
        if fileid is None:
            fileid = self.filenode(path)
        if 'shallowrepo' in self._repo.requirements:
            return remotefilectx.remotefilectx(self._repo, path,
                fileid=fileid, changectx=self, filelog=filelog)
        return orig(self, path, fileid=fileid, filelog=filelog)
    wrapfunction(context.changectx, 'filectx', filectx)

    def workingfilectx(orig, self, path, filelog=None):
        if 'shallowrepo' in self._repo.requirements:
            return remotefilectx.remoteworkingfilectx(self._repo,
                path, workingctx=self, filelog=filelog)
        return orig(self, path, filelog=filelog)
    wrapfunction(context.workingctx, 'filectx', workingfilectx)

    wrapfunction(revset, 'filelog', filelogrevset)
    revset.symbols['filelog'] = revset.filelog

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
            cachepath = os.path.join('/data/users/durham/cache', path, hex(node))
            if not os.path.exists(cachepath):
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

                dirname = os.path.dirname(cachepath)
                if not os.path.exists(dirname):
                    os.makedirs(dirname)
                f = open(cachepath, "w")
                try:
                    f.write(text)
                finally:
                    f.close()

            f = open(cachepath, "r")
            try:
                text = f.read()
            finally:
                f.close()

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

def getrenamedfn(repo, endrev=None):
    rcache = {}

    def getrenamed(fn, rev):
        '''looks up all renames for a file (up to endrev) the first
        time the file is given. It indexes on the changerev and only
        parses the manifest if linkrev != changerev.
        Returns rename info for fn at changerev rev.'''
        if rev in rcache.setdefault(fn, {}):
            return rcache[fn][rev]

        try:
            fctx = repo[rev].filectx(fn)
            for ancestor in fctx.ancestors():
                if ancestor.path() == fn:
                    renamed = ancestor.renamed()
                    rcache[fn][ancestor.rev()] = renamed

            return fctx.renamed()
        except error.LookupError:
            return None

    return getrenamed

def walkfilerevs(orig, repo, match, follow, revs, fncache):
    if not "shallowrepo" in repo.requirements:
        return orig(repo, match, follow, revs, fncache)

    copies = []
    wanted = set()
    minrev, maxrev = min(revs), max(revs)
    def filerevgen(filectx):
        """
        Only files, no patterns.  Check the history of each file.

        Examines filelog entries within minrev, maxrev linkrev range
        Returns an iterator yielding (linkrev, parentlinkrevs, copied)
        tuples in backwards order
        """
        cl_count = len(repo)
        revs = []
        ancestors = [f for f in filectx.ancestors()]
        ancestors.insert(0, filectx)
        for ancestor in ancestors:
            linkrev = ancestor.linkrev()
            if linkrev < minrev:
                continue
            # only yield rev for which we have the changelog, it can
            # happen while doing "hg log" during a pull or commit
            if linkrev >= cl_count:
                break

            parentlinkrevs = []
            for pctx in ancestor.parents():
                parentlinkrevs.append(pctx.linkrev())

            renamed = ancestor.renamed()
            if not follow and renamed:
                parentlinkrevs = []
            revs.append((linkrev, parentlinkrevs,
                         follow and renamed))

            if not follow and renamed:
                break

        return revs
    def iterfiles():
        pctx = repo['.']
        for filename in match.files():
            if follow:
                if filename not in pctx:
                    raise util.Abort(_('cannot follow file not in parent '
                                       'revision: "%s"') % filename)
                yield filename, pctx[filename].filenode()
            else:
                yield filename, None
        for filename_node in copies:
            yield filename_node

    for file_, node in iterfiles():
        # keep track of all ancestors of the file
        if node:
            filectx = repo.filectx(file_, fileid=node)
        else:
            raise cmdutil.FileWalkError("Cannot walk via filelog")

        ancestors = set([filectx.linkrev()])

        # iterate from latest to oldest revision
        for rev, flparentlinkrevs, copied in filerevgen(filectx):
            if not follow:
                if rev > maxrev:
                    continue
            else:
                # Note that last might not be the first interesting
                # rev to us:
                # if the file has been changed after maxrev, we'll
                # have linkrev(last) > maxrev, and we still need
                # to explore the file graph
                if rev not in ancestors:
                    continue
                # XXX insert 1327 fix here
                if flparentlinkrevs:
                    ancestors.update(flparentlinkrevs)

            fncache.setdefault(rev, []).append(file_)
            wanted.add(rev)
            if copied:
                copies.append(copied)
    return wanted

def filelogrevset(orig, repo, subset, x):
    """``filelog(pattern)``
    Changesets connected to the specified filelog.

    For performance reasons, ``filelog()`` does not show every changeset
    that affects the requested file(s). See :hg:`help log` for details. For
    a slower, more accurate result, use ``file()``.
    """

    if not "shallowrepo" in repo.requirements:
        return orig(repo, subset, x)

    # i18n: "filelog" is a keyword
    pat = revset.getstring(x, _("filelog requires a pattern"))
    m = matchmod.match(repo.root, repo.getcwd(), [pat], default='relpath',
                       ctx=repo[None])
    s = set()

    if not matchmod.patkind(pat):
        # slow
        for r in subset:
            ctx = repo[r]
            cfiles = ctx.files()
            for f in m.files():
                if f in cfiles:
                    s.add(ctx.rev())
                    break
    else:
        # partial
        for f in repo[None]:
            filenode = m(f)
            if filenode:
                fctx = repo.filectx(f, fileid=filenode)
                s.add(fctx.linkrev())
                for actx in fctx.ancestors():
                    s.add(actx.linkrev())

    return [r for r in subset if r in s]

def buildtemprevlog(repo, file):
    # get filename key
    filekey = util.sha1(file).hexdigest()
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
    r = filelog.filelog(repo.sopener, 'temprevlog')

    class faket(object):
        def add(self, a,b,c):
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

def debugindex(orig, ui, repo, file_ = None, **opts):
    """dump the contents of an index file"""
    if opts.get('changelog') or opts.get('manifest'):
        return orig(ui, repo, file_, **opts)

    r = buildtemprevlog(repo, file_)

    # debugindex like normal
    format = opts.get('format', 0)
    if format not in (0, 1):
        raise util.Abort(_("unknown format %d") % format)

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
    r = buildtemprevlog(repo, os.path.basename(file_)[:-2])

    ui.write(("digraph G {\n"))
    for i in r:
        node = r.node(i)
        pp = r.parents(node)
        ui.write("\t%d -> %d\n" % (r.rev(pp[0]), i))
        if pp[1] != nullid:
            ui.write("\t%d -> %d\n" % (r.rev(pp[1]), i))
    ui.write("}\n")
