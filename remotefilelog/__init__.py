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
from mercurial import templatekw, repoview, bundlerepo, revset, hg
from mercurial import match as matchmod
import struct, zlib, errno, collections, time, os, pdb, socket, subprocess, lz4
import stat

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

shallowremote = False
remotefilelogreq = "remotefilelog"
localrepo.localrepository.supported.add(remotefilelogreq)
shallowcommands = ["stream_out", "changegroup", "changegroupsubset", "getbundle"]

def uisetup(ui):
    entry = extensions.wrapcommand(commands.table, 'clone', cloneshallow)
    entry[1].append(('', 'shallow', None,
                     _("create a shallow clone which uses remote file history")))

    extensions.wrapcommand(commands.table, 'debugindex', debugindex)
    extensions.wrapcommand(commands.table, 'debugindexdot', debugindexdot)

    # Prevent 'hg manifest --all'
    def _manifest(orig, ui, repo, *args, **opts):
        if remotefilelogreq in repo.requirements and opts.get('all'):
            raise util.Abort(_("--all is not supported in a shallow repo"))

        return orig(ui, repo, *args, **opts)
    extensions.wrapcommand(commands.table, "manifest", _manifest)

def cloneshallow(orig, ui, repo, *args, **opts):
    if opts.get('shallow'):
        def stream_in_shallow(orig, self, remote, requirements):
            # set up the client hooks so the post-clone update works
            setupclient(self.ui, self.unfiltered())

            # setupclient fixed the class on the repo itself
            # but we also need to fix it on the repoview
            if isinstance(self, repoview.repoview):
                self.__class__.__bases__ = (self.__class__.__bases__[0],
                                            self.unfiltered().__class__)

            requirements.add(remotefilelogreq)

            # if the repo was filtered, we need to refilter since
            # the class has changed
            return orig(self, remote, requirements)
        wrapfunction(localrepo.localrepository, 'stream_in', stream_in_shallow)

    try:
        orig(ui, repo, *args, **opts)
    finally:
        if opts.get('shallow') and fileserverclient.client:
            fileserverclient.client.close()

def reposetup(ui, repo):
    if not isinstance(repo, localrepo.localrepository):
        return

    isserverenabled = ui.configbool('remotefilelog', 'server')
    isshallowclient = remotefilelogreq in repo.requirements

    if isserverenabled and isshallowclient:
        raise Exception("Cannot be both a server and shallow client.")

    if isshallowclient:
        setupclient(ui, repo)

    if isserverenabled:
        setupserver(ui, repo)

def setupserver(ui, repo):
    onetimesetup(ui)

    # don't send files to shallow clients
    def generatefiles(orig, self, changedfiles, linknodes, commonrevs, source):
        if shallowremote:
            return iter([])
        return orig(self, changedfiles, linknodes, commonrevs, source)

    wrapfunction(changegroup.bundle10, 'generatefiles', generatefiles)

    # add incoming hook to continuously generate file blobs
    ui.setconfig("hooks", "changegroup.remotefilelog", incominghook)

def setupclient(ui, repo):
    if (not isinstance(repo, localrepo.localrepository) or
        isinstance(repo, bundlerepo.bundlerepository)):
        return

    onetimesetup(ui)
    onetimeclientsetup(ui)

    shallowrepo.wraprepo(repo)
    repo.store = shallowstore.wrapstore(repo.store)

onetime = False
def onetimesetup(ui):
    global onetime
    if onetime:
        return
    onetime = True

    # support file content requests
    wireproto.commands['getfiles'] = (getfiles, '')

    # don't clone filelogs to shallow clients
    def _walkstreamfiles(orig, repo):
        if shallowremote:
            # if we are shallow ourselves, stream our local commits
            if remotefilelogreq in repo.requirements:
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
        elif remotefilelogreq in repo.requirements:
            # don't allow cloning from a shallow repo to a full repo
            # since it would require fetching every version of every
            # file in order to create the revlogs.
            raise util.Abort(_("Cannot clone from a shallow repo "
                             + "to a full repo."))
        else:
            for x in orig(repo):
                yield x

    wrapfunction(wireproto, '_walkstreamfiles', _walkstreamfiles)

    # add shallow commands
    for cmd in shallowcommands:
        func, args = wireproto.commands[cmd]
        def wrap(func):
            def wrapper(*args, **kwargs):
                global shallowremote
                shallowremote = True
                shallowbundle.shallowremote = True
                return func(*args, **kwargs)
            return wrapper

        wireproto.commands[cmd + "_shallow"] = (wrap(func), args)

clientonetime = False
def onetimeclientsetup(ui):
    global clientonetime
    if clientonetime:
        return
    clientonetime = True

    fileserverclient.client = fileserverclient.fileserverclient(ui)

    changegroup.bundle10 = shallowbundle.shallowbundle

    def storewrapper(orig, requirements, path, vfstype):
        s = orig(requirements, path, vfstype)
        if remotefilelogreq in requirements:
            s = shallowstore.wrapstore(s)

        return s
    wrapfunction(store, 'store', storewrapper)

    # prefetch files before update hook
    def applyupdates(orig, repo, actions, wctx, mctx, actx, overwrite):
        if remotefilelogreq in repo.requirements:
            manifest = mctx.manifest()
            files = []
            for f, m, args, msg in [a for a in actions if a[1] == 'g']:
                files.append((f, hex(manifest[f])))
            # batch fetch the needed files from the server
            fileserverclient.client.prefetch(repo, files)
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
    wrapfunction(revset, 'filelog', filelogrevset)
    revset.symbols['filelog'] = revset.filelog
    wrapfunction(cmdutil, 'walkfilerevs', walkfilerevs)

    # prevent strip from considering filelogs
    def _collectbrokencsets(orig, repo, files, striprev):
        if remotefilelogreq in repo.requirements:
            files = []
        return orig(repo, files, striprev)
    wrapfunction(repair, '_collectbrokencsets', _collectbrokencsets)

    # hold on to filelogs until we know the commit hash
    pendingfilecommits = []
    def filelogadd(orig, self, text, meta, transaction, link, p1, p2):
        if isinstance(link, int):
            pendingfilecommits.append((self, text, meta, transaction, link, p1, p2))

            hashtext = remotefilelog._createrevlogtext(text, meta.get('copy'), meta.get('copyrev'))
            node = revlog.hash(hashtext, p1, p2)
            return node
        else:
            return orig(self, text, meta, transaction, link, p1, p2)
    wrapfunction(remotefilelog.remotefilelog, 'add', filelogadd)

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
        if remotefilelogreq in self._repo.requirements:
            return remotefilectx.remotefilectx(self._repo, path,
                fileid=fileid, changectx=self, filelog=filelog)
        return orig(self, path, fileid=fileid, filelog=filelog)
    wrapfunction(context.changectx, 'filectx', filectx)

    def workingfilectx(orig, self, path, filelog=None):
        if remotefilelogreq in self._repo.requirements:
            return remotefilectx.remoteworkingfilectx(self._repo,
                path, workingctx=self, filelog=filelog)
        return orig(self, path, filelog=filelog)
    wrapfunction(context.workingctx, 'filectx', workingfilectx)

    def _callstream(orig, self, cmd, **args):
        if cmd in shallowcommands:
            return orig(self, cmd + "_shallow", **args)
        return orig(self, cmd, **args)
    wrapfunction(sshpeer.sshpeer, '_callstream', _callstream)

def createfileblob(filectx):
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

    return "%d\0%s%s" % (len(text), text, ancestortext)

def getfiles(repo, proto):
    """A server api for requesting particular versions of particular files.
    """
    def streamer():
        fin = proto.fin
        opener = repo.sopener

        cachepath = repo.ui.config("remotefilelog", "servercachepath")
        if not cachepath:
            cachepath = os.path.join(repo.path, "remotefilelogcache")

        # everything should be user & group read/writable
        oldumask = os.umask(0o002)
        try:
            while True:
                request = fin.readline()[:-1]
                if not request:
                    break

                node = bin(request[:40])
                if node == nullid:
                    yield '0\n'
                    continue

                path = request[40:]

                filecachepath = os.path.join(cachepath, path, hex(node))
                if not os.path.exists(filecachepath):
                    filectx = repo.filectx(path, fileid=node)

                    text = createfileblob(filectx)
                    text = lz4.compressHC(text)

                    dirname = os.path.dirname(filecachepath)
                    if not os.path.exists(dirname):
                        os.makedirs(dirname)
                    f = open(filecachepath, "w")
                    try:
                        f.write(text)
                    finally:
                        f.close()

                f = open(filecachepath, "r")
                try:
                    text = f.read()
                finally:
                    f.close()

                yield '%d\n%s' % (len(text), text)

                # it would be better to only flush after processing a whole batch
                # but currently we don't know if there are more requests coming
                proto.fout.flush()
        finally:
            os.umask(oldumask)

    return wireproto.streamres(streamer())

def incominghook(ui, repo, node, source, url, **kwargs):
    cachepath = repo.ui.config("remotefilelog", "servercachepath")
    if not cachepath:
        cachepath = os.path.join(repo.path, "remotefilelogcache")

    heads = repo.revs("heads(%s::)" % node)

    # everything should be user & group read/writable
    oldumask = os.umask(0o002)
    try:
        count = 0
        for head in heads:
            mf = repo[head].manifest()
            for filename, filenode in mf.iteritems():
                filecachepath = os.path.join(cachepath, filename, hex(filenode))
                if os.path.exists(filecachepath):
                    continue

                # This can be a bit slow. Don't block the commit returning
                # for large commits.
                if count > 500:
                    break
                count += 1

                filectx = repo.filectx(filename, fileid=filenode)

                text = createfileblob(filectx)
                text = lz4.compressHC(text)

                dirname = os.path.dirname(filecachepath)
                if not os.path.exists(dirname):
                    os.makedirs(dirname)
                f = open(filecachepath, "w")
                try:
                    f.write(text)
                finally:
                    f.close()
    finally:
        os.umask(oldumask)

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
    if not remotefilelogreq in repo.requirements:
        return orig(repo, match, follow, revs, fncache)

    if not follow:
        raise cmdutil.FileWalkError("Cannot walk via filelog")

    wanted = set()
    minrev, maxrev = min(revs), max(revs)

    pctx = repo['.']
    for filename in match.files():
        if filename not in pctx:
            raise util.Abort(_('cannot follow file not in parent '
                               'revision: "%s"') % filename)
        fctx = pctx[filename]

        linkrev = fctx.linkrev()
        if linkrev >= minrev and linkrev <= maxrev:
            fncache.setdefault(linkrev, []).append(filename)
            wanted.add(linkrev)

        for ancestor in fctx.ancestors():
            linkrev = ancestor.linkrev()
            if linkrev >= minrev and linkrev <= maxrev:
                fncache.setdefault(linkrev, []).append(ancestor.path())
                wanted.add(linkrev)

    return wanted

def filelogrevset(orig, repo, subset, x):
    """``filelog(pattern)``
    Changesets connected to the specified filelog.

    For performance reasons, ``filelog()`` does not show every changeset
    that affects the requested file(s). See :hg:`help log` for details. For
    a slower, more accurate result, use ``file()``.
    """

    if not remotefilelogreq in repo.requirements:
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
    if (opts.get('changelog') or opts.get('manifest') or
        not remotefilelogreq in repo.requirements):
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
    if not remotefilelogreq in repo.requirements:
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

commands.norepo += " gc"

@command('^gc', [], _('hg gc [CACHEPATH]'))
def gc(ui, *args, **opts):
    '''garbage collect the filelog cache
    '''
    if len(args) > 0:
        cachepath = args[0]
    else:
        repo = hg.peer(ui, {}, ui.environ['PWD'])
        cachepath = ui.config("remotefilelog", "cachepath")
        if not cachepath:
            return

    # get list of repos that use this cache
    repospath = os.path.join(cachepath, 'repos')
    if not os.path.exists(repospath):
        ui.warn("no known cache at %s\n" % cachepath)
        return

    reposfile = open(repospath, 'r')
    repos = set([r[:-1] for r in reposfile.readlines()])
    reposfile.close()

    # build list of useful files
    validrepos = []
    keepfiles = set()

    _analyzing = _("analyzing repositories")
    _removing = _("removing unnecessary files")
    _truncating = _("enforcing cache limit")

    count = 0
    for path in repos:
        ui.progress(_analyzing, count, unit="repos", total=len(repos))
        count += 1
        path = ui.expandpath(path)
        try:
            peer = hg.peer(ui, {}, path)
        except error.RepoError:
            continue

        validrepos.append(path)

        keep = peer._repo.revs("(parents(draft()) + heads(all())) & public()")
        for r in keep:
            m = peer._repo[r].manifest()
            for filename, filenode in m.iteritems():
                key = fileserverclient.getcachekey(filename, hex(filenode))
                keepfiles.add(key)

    ui.progress(_analyzing, None)

    # write list of valid repos back
    reposfile = open(repospath, 'w')
    reposfile.writelines([("%s\n" % r) for r in validrepos])
    reposfile.close()

    # prune cache
    import Queue
    queue = Queue.PriorityQueue()
    originalsize = 0
    size = 0
    count = 0
    removed = 0

    ui.progress(_removing, count, unit="files")
    for root, dirs, files in os.walk(cachepath):
        for file in files:
            if file == 'repos':
                continue

            ui.progress(_removing, count, unit="files")
            path = os.path.join(root, file)
            key = os.path.basename(root) + "/" + file
            count += 1
            stat = os.stat(path)
            originalsize += stat.st_size
            if key not in keepfiles:
                os.remove(path)
                removed += 1
            else:
                queue.put((stat.st_atime, path, stat))
                size += stat.st_size
    ui.progress(_removing, None)

    # remove oldest files until under limit
    limit = ui.configbytes("remotefilelog", "cachelimit", "1000 GB")
    if size > limit:
        excess = size - limit
        removedexcess = 0
        while queue and size > limit and size > 0:
            ui.progress(_truncating, removedexcess, unit="bytes", total=excess)
            atime, oldpath, stat = queue.get()
            os.remove(oldpath)
            size -= stat.st_size
            removed += 1
            removedexcess += stat.st_size
    ui.progress(_truncating, None)

    ui.status("finished: removed %s of %s files (%0.2f GB to %0.2f GB)\n" %
              (removed, count, float(originalsize) / 1024.0 / 1024.0 / 1024.0,
              float(size) / 1024.0 / 1024.0 / 1024.0))
