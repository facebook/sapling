# __init__.py - remotefilelog extension
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

testedwith = 'internal'

import fileserverclient, remotefilelog, remotefilectx, shallowstore, shallowrepo
import shallowbundle, debugcommands, remotefilelogserver
from mercurial.node import bin, hex, nullid, nullrev, short
from mercurial.i18n import _
from mercurial.extensions import wrapfunction
from mercurial import ancestor, mdiff, parsers, error, util, dagutil
from mercurial import repair, extensions, filelog, revlog, wireproto, cmdutil
from mercurial import copies, store, context, changegroup, localrepo
from mercurial import commands, sshpeer, scmutil, dispatch, merge, context, changelog
from mercurial import templatekw, repoview, revset, hg, patch, verify
from mercurial import match, exchange
import struct, zlib, errno, collections, time, os, socket, subprocess, lz4
import stat

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = ''

repoclass = localrepo.localrepository
if util.safehasattr(repoclass, '_basesupported'):
    repoclass._basesupported.add(shallowrepo.requirement)
else:
    # hg <= 2.7
    repoclass.supported.add(shallowrepo.requirement)

def uisetup(ui):
    """Wraps user facing Mercurial commands to swap them out with shallow versions.
    """
    hg.wirepeersetupfuncs.append(fileserverclient.peersetup)

    entry = extensions.wrapcommand(commands.table, 'clone', cloneshallow)
    entry[1].append(('', 'shallow', None,
                     _("create a shallow clone which uses remote file history")))

    extensions.wrapcommand(commands.table, 'debugindex',
        debugcommands.debugindex)
    extensions.wrapcommand(commands.table, 'debugindexdot',
        debugcommands.debugindexdot)
    extensions.wrapcommand(commands.table, 'log', log)
    extensions.wrapcommand(commands.table, 'pull', pull)

    # Prevent 'hg manifest --all'
    def _manifest(orig, ui, repo, *args, **opts):
        if shallowrepo.requirement in repo.requirements and opts.get('all'):
            raise util.Abort(_("--all is not supported in a shallow repo"))

        return orig(ui, repo, *args, **opts)
    extensions.wrapcommand(commands.table, "manifest", _manifest)

def cloneshallow(orig, ui, repo, *args, **opts):
    if opts.get('shallow'):
        repos = []
        def clone_shallow(orig, self, *args, **kwargs):
            repos.append(self.unfiltered())
            # set up the client hooks so the post-clone update works
            setupclient(self.ui, self.unfiltered())

            # setupclient fixed the class on the repo itself
            # but we also need to fix it on the repoview
            if isinstance(self, repoview.repoview):
                self.__class__.__bases__ = (self.__class__.__bases__[0],
                                            self.unfiltered().__class__)
            self.requirements.add(shallowrepo.requirement)
            self._writerequirements()
            return orig(self, *args, **kwargs)
        wrapfunction(localrepo.localrepository, 'clone', clone_shallow)

        def stream_in_shallow(orig, self, remote, requirements):
            requirements.add(shallowrepo.requirement)

            # Replace remote.stream_out with a version that sends file
            # patterns.
            def stream_out_shallow(orig):
                if shallowrepo.requirement in remote._capabilities():
                    opts = {}
                    if self.includepattern:
                        opts['includepattern'] = '\0'.join(self.includepattern)
                    if self.excludepattern:
                        opts['excludepattern'] = '\0'.join(self.excludepattern)
                    return remote._callstream('stream_out_shallow', **opts)
                else:
                    return orig()
            wrapfunction(remote, 'stream_out', stream_out_shallow)

            return orig(self, remote, requirements)
        wrapfunction(localrepo.localrepository, 'stream_in', stream_in_shallow)

    try:
        orig(ui, repo, *args, **opts)
    finally:
        if opts.get('shallow'):
            for r in repos:
                if util.safehasattr(r, 'fileservice'):
                    r.fileservice.close()

def reposetup(ui, repo):
    if not isinstance(repo, localrepo.localrepository):
        return

    isserverenabled = ui.configbool('remotefilelog', 'server')
    isshallowclient = shallowrepo.requirement in repo.requirements

    if isserverenabled and isshallowclient:
        raise Exception("Cannot be both a server and shallow client.")

    if isshallowclient:
        setupclient(ui, repo)

    if isserverenabled:
        remotefilelogserver.setupserver(ui, repo)

def setupclient(ui, repo):
    if not isinstance(repo, localrepo.localrepository):
        return

    # Even clients get the server setup since they need to have the
    # wireprotocol endpoints registered.
    remotefilelogserver.onetimesetup(ui)
    onetimeclientsetup(ui)

    shallowrepo.wraprepo(repo)
    repo.store = shallowstore.wrapstore(repo.store)

clientonetime = False
def onetimeclientsetup(ui):
    global clientonetime
    if clientonetime:
        return
    clientonetime = True

    # some users in core still call changegroup.cg1packer directly
    changegroup.cg1packer = shallowbundle.shallowcg1packer
    if util.safehasattr(changegroup, 'packermap'):
        # Mercurial >= 3.3
        packermap01 = changegroup.packermap['01']
        packermap02 = changegroup.packermap['02']
        changegroup.packermap['01'] = (shallowbundle.shallowcg1packer,
                                       packermap01[1])
        changegroup.packermap['02'] = (shallowbundle.shallowcg2packer,
                                       packermap02[1])
    wrapfunction(changegroup, 'addchangegroupfiles', shallowbundle.addchangegroupfiles)
    wrapfunction(changegroup, 'getchangegroup', shallowbundle.getchangegroup)

    def storewrapper(orig, requirements, path, vfstype):
        s = orig(requirements, path, vfstype)
        if shallowrepo.requirement in requirements:
            s = shallowstore.wrapstore(s)

        return s
    wrapfunction(store, 'store', storewrapper)

    extensions.wrapfunction(exchange, 'pull', exchangepull)

    # prefetch files before update
    def applyupdates(orig, repo, actions, wctx, mctx, overwrite, labels=None):
        if shallowrepo.requirement in repo.requirements:
            manifest = mctx.manifest()
            files = []
            for f, args, msg in actions['g']:
                files.append((f, hex(manifest[f])))
            # batch fetch the needed files from the server
            repo.fileservice.prefetch(files)
        return orig(repo, actions, wctx, mctx, overwrite, labels=labels)
    wrapfunction(merge, 'applyupdates', applyupdates)

    # prefetch files before mergecopies check
    def computenonoverlap(orig, repo, c1, c2, addedinm1, addedinm2):
        u1, u2 = orig(repo, c1, c2, addedinm1, addedinm2)
        if shallowrepo.requirement in repo.requirements:
            m1 = c1.manifest()
            m2 = c2.manifest()
            files = []

            sparsematch1 = repo.sparsematch(c1.rev())
            if sparsematch1:
                sparseu1 = []
                for f in u1:
                    if sparsematch1(f):
                        files.append((f, hex(m1[f])))
                        sparseu1.append(f)
                u1 = sparseu1

            sparsematch2 = repo.sparsematch(c2.rev())
            if sparsematch2:
                sparseu2 = []
                for f in u2:
                    if sparsematch2(f):
                        files.append((f, hex(m2[f])))
                        sparseu2.append(f)
                u2 = sparseu2

            # batch fetch the needed files from the server
            repo.fileservice.prefetch(files)
        return u1, u2
    wrapfunction(copies, '_computenonoverlap', computenonoverlap)

    # prefetch files before pathcopies check
    def computeforwardmissing(orig, a, b, match=None):
        missing = list(orig(a, b, match=match))
        repo = a._repo
        if shallowrepo.requirement in repo.requirements:
            mb = b.manifest()

            files = []
            sparsematch = repo.sparsematch(b.rev())
            if sparsematch:
                sparsemissing = []
                for f in missing:
                    if sparsematch(f):
                        files.append((f, hex(mb[f])))
                        sparsemissing.append(f)
                missing = sparsemissing

            # batch fetch the needed files from the server
            repo.fileservice.prefetch(files)
        return missing
    wrapfunction(copies, '_computeforwardmissing', computeforwardmissing)

    # close cache miss server connection after the command has finished
    def runcommand(orig, lui, repo, *args, **kwargs):
        try:
            return orig(lui, repo, *args, **kwargs)
        finally:
            repo.fileservice.close()
    wrapfunction(dispatch, 'runcommand', runcommand)

    # disappointing hacks below
    templatekw.getrenamedfn = getrenamedfn
    wrapfunction(revset, 'filelog', filelogrevset)
    revset.symbols['filelog'] = revset.filelog
    wrapfunction(cmdutil, 'walkfilerevs', walkfilerevs)

    # prevent strip from stripping remotefilelogs
    def _collectbrokencsets(orig, repo, files, striprev):
        if shallowrepo.requirement in repo.requirements:
            files = list([f for f in files if not repo.shallowmatch(f)])
        return orig(repo, files, striprev)
    wrapfunction(repair, '_collectbrokencsets', _collectbrokencsets)

    # Don't commit filelogs until we know the commit hash, since the hash
    # is present in the filelog blob.
    # This violates Mercurial's filelog->manifest->changelog write order,
    # but is generally fine for client repos.
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
        if (shallowrepo.requirement in self._repo.requirements and
            self._repo.shallowmatch(path)):
            return remotefilectx.remotefilectx(self._repo, path,
                fileid=fileid, changectx=self, filelog=filelog)
        return orig(self, path, fileid=fileid, filelog=filelog)
    wrapfunction(context.changectx, 'filectx', filectx)

    def workingfilectx(orig, self, path, filelog=None):
        if (shallowrepo.requirement in self._repo.requirements and
            self._repo.shallowmatch(path)):
            return remotefilectx.remoteworkingfilectx(self._repo,
                path, workingctx=self, filelog=filelog)
        return orig(self, path, filelog=filelog)
    wrapfunction(context.workingctx, 'filectx', workingfilectx)

    # prefetch required revisions before a diff
    def trydiff(orig, repo, revs, ctx1, ctx2, modified, added, removed,
                copy, getfilectx, *args, **kwargs):
        if shallowrepo.requirement in repo.requirements:
            prefetch = []
            mf1 = ctx1.manifest()
            for fname in modified + added + removed:
                if fname in mf1:
                    fnode = getfilectx(fname, ctx1).filenode()
                    # fnode can be None if it's a edited working ctx file
                    if fnode:
                        prefetch.append((fname, hex(fnode)))
                if fname not in removed:
                    fnode = getfilectx(fname, ctx2).filenode()
                    if fnode:
                        prefetch.append((fname, hex(fnode)))

            repo.fileservice.prefetch(prefetch)

        return orig(repo, revs, ctx1, ctx2, modified, added, removed,
            copy, getfilectx, *args, **kwargs)
    wrapfunction(patch, 'trydiff', trydiff)

    # Prevent verify from processing files
    def _verify(orig, repo):
        # terrible, terrible hack:
        # To prevent verify from checking files, we throw an exception when
        # it tries to access a filelog. We then catch the exception and
        # exit gracefully.
        class FakeException(Exception):
            pass
        def emptylen(*args, **kwargs):
            raise FakeException()
        remotefilelog.remotefilelog.__len__ = emptylen
        try:
            return orig(repo)
        except FakeException:
            ui.progress(_('checking'), None)
            pass
    wrapfunction(verify, '_verify', _verify)

    if util.safehasattr(cmdutil, '_revertprefetch'):
        wrapfunction(cmdutil, '_revertprefetch', _revertprefetch)
    else:
        wrapfunction(cmdutil, 'revert', revert)

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
    if not shallowrepo.requirement in repo.requirements:
        return orig(repo, match, follow, revs, fncache)

    # remotefilelog's can't be walked in rev order, so throw.
    # The caller will see the exception and walk the commit tree instead.
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

    if not shallowrepo.requirement in repo.requirements:
        return orig(repo, subset, x)

    # i18n: "filelog" is a keyword
    pat = revset.getstring(x, _("filelog requires a pattern"))
    m = match.match(repo.root, repo.getcwd(), [pat], default='relpath',
                       ctx=repo[None])
    s = set()

    if not match.patkind(pat):
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


commands.norepo += " gc"

@command('gc', [], _('hg gc [REPO...]'))
def gc(ui, *args, **opts):
    '''garbage collect the client and server filelog caches
    '''
    cachepaths = set()

    # get the system client cache
    systemcache = ui.config("remotefilelog", "cachepath")
    if systemcache:
        systemcache = util.expandpath(systemcache)
        cachepaths.add(systemcache)

    # get repo client and server cache
    repopaths = [ui.environ['PWD']]
    repopaths.extend(args)
    repos = []
    for repopath in repopaths:
        try:
            repo = hg.peer(ui, {}, repopath)
            repos.append(repo)

            repocache = repo.ui.config("remotefilelog", "cachepath")
            if repocache:
                repocache = util.expandpath(repocache)
                cachepaths.add(repocache)
        except error.RepoError:
            pass

    # gc client cache
    for cachepath in cachepaths:
        gcclient(ui, cachepath)

    # gc server cache
    for repo in repos:
        remotefilelogserver.gcserver(ui, repo._repo)

def gcclient(ui, cachepath):
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
    keepkeys = set()

    _analyzing = _("analyzing repositories")

    localcache = None

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

        reponame = peer._repo.name
        if not localcache:
            localcache = peer._repo.fileservice.localcache
        keep = peer._repo.revs("(parents(draft()) + heads(all())) & public()")
        for r in keep:
            m = peer._repo[r].manifest()
            for filename, filenode in m.iteritems():
                key = fileserverclient.getcachekey(reponame, filename,
                    hex(filenode))
                keepkeys.add(key)

    ui.progress(_analyzing, None)

    # write list of valid repos back
    oldumask = os.umask(0o002)
    try:
        reposfile = open(repospath, 'w')
        reposfile.writelines([("%s\n" % r) for r in validrepos])
        reposfile.close()
    finally:
        os.umask(oldumask)

    # prune cache
    localcache.gc(keepkeys)

def log(orig, ui, repo, *pats, **opts):
    if pats and not opts.get("follow"):
        # Force slowpath for non-follow patterns
        opts['removed'] = True
        match, pats = scmutil.matchandpats(repo['.'], pats, opts)
        isfile = not match.anypats()
        if isfile:
            for file in match.files():
                if not os.path.isfile(repo.wjoin(file)):
                    isfile = False
                    break

        if isfile:
            ui.warn(_("warning: file log can be slow on large repos - " +
                      "use -f to speed it up\n"))

    return orig(ui, repo, *pats, **opts)

def pull(orig, ui, repo, *pats, **opts):
    result = orig(ui, repo, *pats, **opts)

    if shallowrepo.requirement in repo.requirements:
        # prefetch if it's configured
        prefetchrevset = ui.config('remotefilelog', 'pullprefetch', None)
        if prefetchrevset:
            ui.status("prefetching file contents\n")
            revs = repo.revs(prefetchrevset)
            base = repo['.'].rev()
            repo.prefetch(revs, base=base)

    return result

def exchangepull(orig, repo, remote, *args, **kwargs):
    # Hook into the callstream/getbundle to insert bundle capabilities
    # during a pull.
    def remotecallstream(orig, command, **opts):
        if command == 'getbundle' and 'remotefilelog' in remote._capabilities():
            bundlecaps = opts.get('bundlecaps')
            if bundlecaps:
                bundlecaps = [bundlecaps]
            else:
                bundlecaps = []
            bundlecaps.append('remotefilelog')
            if repo.includepattern:
                bundlecaps.append("includepattern=" + '\0'.join(repo.includepattern))
            if repo.excludepattern:
                bundlecaps.append("excludepattern=" + '\0'.join(repo.excludepattern))
            opts['bundlecaps'] = ','.join(bundlecaps)
        return orig(command, **opts)

    def localgetbundle(orig, source, heads=None, common=None, bundlecaps=None,
                       **kwargs):
        if not bundlecaps:
            bundlecaps = set()
        bundlecaps.add('remotefilelog')
        return orig(source, heads=heads, common=common, bundlecaps=bundlecaps,
                    **kwargs)

    if hasattr(remote, '_callstream'):
        wrapfunction(remote, '_callstream', remotecallstream)
    elif hasattr(remote, 'getbundle'):
        wrapfunction(remote, 'getbundle', localgetbundle)

    return orig(repo, remote, *args, **kwargs)

def revert(orig, ui, repo, ctx, parents, *pats, **opts):
    # prefetch prior to reverting
    # used for old mercurial version
    if shallowrepo.requirement in repo.requirements:
        files = []
        m = scmutil.match(ctx, pats, opts)
        mf = ctx.manifest()
        m.bad = lambda x, y: False
        for path in ctx.walk(m):
            files.append((path, hex(mf[path])))
        repo.fileservice.prefetch(files)

    return orig(ui, repo, ctx, parents, *pats, **opts)

def _revertprefetch(orig, repo, ctx, *files):
    # prefetch data that needs to be reverted
    # used for new mercurial version
    if shallowrepo.requirement in repo.requirements:
        allfiles = []
        mf = ctx.manifest()
        sparsematch = repo.sparsematch(ctx.rev())
        for f in files:
            for path in f:
                if not sparsematch or sparsematch(path):
                    allfiles.append((path, hex(mf[path])))
        repo.fileservice.prefetch(allfiles)
    return orig(repo, ctx, *files)

commands.norepo += " debugremotefilelog"

@command('debugremotefilelog', [
    ('d', 'decompress', None, _('decompress the filelog first')),
    ], _('hg debugremotefilelog <path>'))
def debugremotefilelog(ui, *args, **opts):
    return debugcommands.debugremotefilelog(ui, *args, **opts)

commands.norepo += " verifyremotefilelog"

@command('verifyremotefilelog', [
    ('d', 'decompress', None, _('decompress the filelogs first')),
    ], _('hg verifyremotefilelogs <directory>'))
def verifyremotefilelog(ui, *args, **opts):
    return debugcommands.verifyremotefilelog(ui, *args, **opts)

@command('prefetch', [
    ('r', 'rev', [], _('prefetch the specified revisions'), _('REV')),
    ] + commands.walkopts, _('hg prefetch [OPTIONS] [FILE...]'))
def prefetch(ui, repo, *pats, **opts):
    """prefetch file revisions from the server

    Prefetchs file revisions for the specified revs and stores them in the
    local remotefilelog cache.  If no rev is specified, it uses your current
    commit. File names or patterns can be used to limit which files are
    downloaded.

    Return 0 on success.
    """
    if not shallowrepo.requirement in repo.requirements:
        raise util.Abort(_("repo is not shallow"))

    if not opts.get('rev'):
        opts['rev'] = '.'

    m = scmutil.matchall(repo)
    revs = scmutil.revrange(repo, opts.get('rev'))

    repo.prefetch(revs, pats=pats, opts=opts)
