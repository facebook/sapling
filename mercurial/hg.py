# hg.py - repository classes for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
# Copyright 2006 Vadim Gelfer <vadim.gelfer@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import os
import shutil

from .i18n import _
from .node import nullid

from . import (
    bookmarks,
    bundlerepo,
    cmdutil,
    destutil,
    discovery,
    error,
    exchange,
    extensions,
    httppeer,
    localrepo,
    lock,
    merge as mergemod,
    node,
    phases,
    repoview,
    scmutil,
    sshpeer,
    statichttprepo,
    ui as uimod,
    unionrepo,
    url,
    util,
    verify as verifymod,
)

release = lock.release

def _local(path):
    path = util.expandpath(util.urllocalpath(path))
    return (os.path.isfile(path) and bundlerepo or localrepo)

def addbranchrevs(lrepo, other, branches, revs):
    peer = other.peer() # a courtesy to callers using a localrepo for other
    hashbranch, branches = branches
    if not hashbranch and not branches:
        x = revs or None
        if util.safehasattr(revs, 'first'):
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

    if not peer.capable('branchmap'):
        if branches:
            raise error.Abort(_("remote branch lookup not supported"))
        revs.append(hashbranch)
        return revs, revs[0]
    branchmap = peer.branchmap()

    def primary(branch):
        if branch == '.':
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
    '''parse url#branch, returning (url, (branch, branches))'''

    u = util.url(path)
    branch = None
    if u.fragment:
        branch = u.fragment
        u.fragment = None
    return str(u), (branch, branches or [])

schemes = {
    'bundle': bundlerepo,
    'union': unionrepo,
    'file': _local,
    'http': httppeer,
    'https': httppeer,
    'ssh': sshpeer,
    'static-http': statichttprepo,
}

def _peerlookup(path):
    u = util.url(path)
    scheme = u.scheme or 'file'
    thing = schemes.get(scheme) or schemes['file']
    try:
        return thing(path)
    except TypeError:
        # we can't test callable(thing) because 'thing' can be an unloaded
        # module that implements __call__
        if not util.safehasattr(thing, 'instance'):
            raise
        return thing

def islocal(repo):
    '''return true if repo (or path pointing to repo) is local'''
    if isinstance(repo, str):
        try:
            return _peerlookup(repo).islocal(repo)
        except AttributeError:
            return False
    return repo.local()

def openpath(ui, path):
    '''open path with open if local, url.open if remote'''
    pathurl = util.url(path, parsequery=False, parsefragment=False)
    if pathurl.islocal():
        return util.posixfile(pathurl.localpath(), 'rb')
    else:
        return url.open(ui, path)

# a list of (ui, repo) functions called for wire peer initialization
wirepeersetupfuncs = []

def _peerorrepo(ui, path, create=False):
    """return a repository object for the specified path"""
    obj = _peerlookup(path).instance(ui, path, create)
    ui = getattr(obj, "ui", ui)
    for name, module in extensions.extensions(ui):
        hook = getattr(module, 'reposetup', None)
        if hook:
            hook(ui, obj)
    if not obj.local():
        for f in wirepeersetupfuncs:
            f(ui, obj)
    return obj

def repository(ui, path='', create=False):
    """return a repository object for the specified path"""
    peer = _peerorrepo(ui, path, create)
    repo = peer.local()
    if not repo:
        raise error.Abort(_("repository '%s' is not local") %
                         (path or peer.url()))
    return repo.filtered('visible')

def peer(uiorrepo, opts, path, create=False):
    '''return a repository peer for the specified path'''
    rui = remoteui(uiorrepo, opts)
    return _peerorrepo(rui, path, create).peer()

def defaultdest(source):
    '''return default destination of clone if none is given

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
    '''
    path = util.url(source).path
    if not path:
        return ''
    return os.path.basename(os.path.normpath(path))

def share(ui, source, dest=None, update=True, bookmarks=True):
    '''create a shared repository'''

    if not islocal(source):
        raise error.Abort(_('can only share local repositories'))

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

    sharedpath = srcrepo.sharedpath # if our source is already sharing

    destwvfs = scmutil.vfs(dest, realpath=True)
    destvfs = scmutil.vfs(os.path.join(destwvfs.base, '.hg'), realpath=True)

    if destvfs.lexists():
        raise error.Abort(_('destination already exists'))

    if not destwvfs.isdir():
        destwvfs.mkdir()
    destvfs.makedir()

    requirements = ''
    try:
        requirements = srcrepo.vfs.read('requires')
    except IOError as inst:
        if inst.errno != errno.ENOENT:
            raise

    requirements += 'shared\n'
    destvfs.write('requires', requirements)
    destvfs.write('sharedpath', sharedpath)

    r = repository(ui, destwvfs.base)
    postshare(srcrepo, r, bookmarks=bookmarks)
    _postshareupdate(r, update, checkout=checkout)

def postshare(sourcerepo, destrepo, bookmarks=True):
    """Called after a new shared repo is created.

    The new repo only has a requirements file and pointer to the source.
    This function configures additional shared data.

    Extensions can wrap this function and write additional entries to
    destrepo/.hg/shared to indicate additional pieces of data to be shared.
    """
    default = sourcerepo.ui.config('paths', 'default')
    if default:
        fp = destrepo.vfs("hgrc", "w", text=True)
        fp.write("[paths]\n")
        fp.write("default = %s\n" % default)
        fp.close()

    if bookmarks:
        fp = destrepo.vfs('shared', 'w')
        fp.write('bookmarks\n')
        fp.close()

def _postshareupdate(repo, update, checkout=None):
    """Maybe perform a working directory update after a shared repo is created.

    ``update`` can be a boolean or a revision to update to.
    """
    if not update:
        return

    repo.ui.status(_("updating working directory\n"))
    if update is not True:
        checkout = update
    for test in (checkout, 'default', 'tip'):
        if test is None:
            continue
        try:
            uprev = repo.lookup(test)
            break
        except error.RepoLookupError:
            continue
    _update(repo, uprev)

def copystore(ui, srcrepo, destpath):
    '''copy files from store of srcrepo in destpath

    returns destlock
    '''
    destlock = None
    try:
        hardlink = None
        num = 0
        closetopic = [None]
        def prog(topic, pos):
            if pos is None:
                closetopic[0] = topic
            else:
                ui.progress(topic, pos + num)
        srcpublishing = srcrepo.publishing()
        srcvfs = scmutil.vfs(srcrepo.sharedpath)
        dstvfs = scmutil.vfs(destpath)
        for f in srcrepo.store.copylist():
            if srcpublishing and f.endswith('phaseroots'):
                continue
            dstbase = os.path.dirname(f)
            if dstbase and not dstvfs.exists(dstbase):
                dstvfs.mkdir(dstbase)
            if srcvfs.exists(f):
                if f.endswith('data'):
                    # 'dstbase' may be empty (e.g. revlog format 0)
                    lockfile = os.path.join(dstbase, "lock")
                    # lock to avoid premature writing to the target
                    destlock = lock.lock(dstvfs, lockfile)
                hardlink, n = util.copyfiles(srcvfs.join(f), dstvfs.join(f),
                                             hardlink, progress=prog)
                num += n
        if hardlink:
            ui.debug("linked %d files\n" % num)
            if closetopic[0]:
                ui.progress(closetopic[0], None)
        else:
            ui.debug("copied %d files\n" % num)
            if closetopic[0]:
                ui.progress(closetopic[0], None)
        return destlock
    except: # re-raises
        release(destlock)
        raise

def clonewithshare(ui, peeropts, sharepath, source, srcpeer, dest, pull=False,
                   rev=None, update=True, stream=False):
    """Perform a clone using a shared repo.

    The store for the repository will be located at <sharepath>/.hg. The
    specified revisions will be cloned or pulled from "source". A shared repo
    will be created at "dest" and a working copy will be created if "update" is
    True.
    """
    revs = None
    if rev:
        if not srcpeer.capable('lookup'):
            raise error.Abort(_("src repository does not support "
                               "revision lookup and so doesn't "
                               "support clone by revision"))
        revs = [srcpeer.lookup(r) for r in rev]

    # Obtain a lock before checking for or cloning the pooled repo otherwise
    # 2 clients may race creating or populating it.
    pooldir = os.path.dirname(sharepath)
    # lock class requires the directory to exist.
    try:
        util.makedir(pooldir, False)
    except OSError as e:
        if e.errno != errno.EEXIST:
            raise

    poolvfs = scmutil.vfs(pooldir)
    basename = os.path.basename(sharepath)

    with lock.lock(poolvfs, '%s.lock' % basename):
        if os.path.exists(sharepath):
            ui.status(_('(sharing from existing pooled repository %s)\n') %
                      basename)
        else:
            ui.status(_('(sharing from new pooled repository %s)\n') % basename)
            # Always use pull mode because hardlinks in share mode don't work
            # well. Never update because working copies aren't necessary in
            # share mode.
            clone(ui, peeropts, source, dest=sharepath, pull=True,
                  rev=rev, update=False, stream=stream)

    sharerepo = repository(ui, path=sharepath)
    share(ui, sharerepo, dest=dest, update=False, bookmarks=False)

    # We need to perform a pull against the dest repo to fetch bookmarks
    # and other non-store data that isn't shared by default. In the case of
    # non-existing shared repo, this means we pull from the remote twice. This
    # is a bit weird. But at the time it was implemented, there wasn't an easy
    # way to pull just non-changegroup data.
    destrepo = repository(ui, path=dest)
    exchange.pull(destrepo, srcpeer, heads=revs)

    _postshareupdate(destrepo, update)

    return srcpeer, peer(ui, peeropts, dest)

def clone(ui, peeropts, source, dest=None, pull=False, rev=None,
          update=True, stream=False, branch=None, shareopts=None):
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

    branch: branches to clone

    shareopts: dict of options to control auto sharing behavior. The "pool" key
    activates auto sharing mode and defines the directory for stores. The
    "mode" key determines how to construct the directory name of the shared
    repository. "identity" means the name is derived from the node of the first
    changeset in the repository. "remote" means the name is derived from the
    remote's path/URL. Defaults to "identity."
    """

    if isinstance(source, str):
        origsource = ui.expandpath(source)
        source, branch = parseurl(origsource, branch)
        srcpeer = peer(ui, peeropts, source)
    else:
        srcpeer = source.peer() # in case we were called with a localrepo
        branch = (None, branch or [])
        origsource = source = srcpeer.url()
    rev, checkout = addbranchrevs(srcpeer, srcpeer, branch, rev)

    if dest is None:
        dest = defaultdest(source)
        if dest:
            ui.status(_("destination directory: %s\n") % dest)
    else:
        dest = ui.expandpath(dest)

    dest = util.urllocalpath(dest)
    source = util.urllocalpath(source)

    if not dest:
        raise error.Abort(_("empty destination path is not valid"))

    destvfs = scmutil.vfs(dest, expandpath=True)
    if destvfs.lexists():
        if not destvfs.isdir():
            raise error.Abort(_("destination '%s' already exists") % dest)
        elif destvfs.listdir():
            raise error.Abort(_("destination '%s' is not empty") % dest)

    shareopts = shareopts or {}
    sharepool = shareopts.get('pool')
    sharenamemode = shareopts.get('mode')
    if sharepool and islocal(dest):
        sharepath = None
        if sharenamemode == 'identity':
            # Resolve the name from the initial changeset in the remote
            # repository. This returns nullid when the remote is empty. It
            # raises RepoLookupError if revision 0 is filtered or otherwise
            # not available. If we fail to resolve, sharing is not enabled.
            try:
                rootnode = srcpeer.lookup('0')
                if rootnode != node.nullid:
                    sharepath = os.path.join(sharepool, node.hex(rootnode))
                else:
                    ui.status(_('(not using pooled storage: '
                                'remote appears to be empty)\n'))
            except error.RepoLookupError:
                ui.status(_('(not using pooled storage: '
                            'unable to resolve identity of remote)\n'))
        elif sharenamemode == 'remote':
            sharepath = os.path.join(sharepool, util.sha1(source).hexdigest())
        else:
            raise error.Abort('unknown share naming mode: %s' % sharenamemode)

        if sharepath:
            return clonewithshare(ui, peeropts, sharepath, source, srcpeer,
                                  dest, pull=pull, rev=rev, update=update,
                                  stream=stream)

    srclock = destlock = cleandir = None
    srcrepo = srcpeer.local()
    try:
        abspath = origsource
        if islocal(origsource):
            abspath = os.path.abspath(util.urllocalpath(origsource))

        if islocal(dest):
            cleandir = dest

        copy = False
        if (srcrepo and srcrepo.cancopy() and islocal(dest)
            and not phases.hassecret(srcrepo)):
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
            srcrepo.hook('preoutgoing', throw=True, source='clone')
            hgdir = os.path.realpath(os.path.join(dest, ".hg"))
            if not os.path.exists(dest):
                os.mkdir(dest)
            else:
                # only clean up directories we create ourselves
                cleandir = hgdir
            try:
                destpath = hgdir
                util.makedir(destpath, notindexed=True)
            except OSError as inst:
                if inst.errno == errno.EEXIST:
                    cleandir = None
                    raise error.Abort(_("destination '%s' already exists")
                                     % dest)
                raise

            destlock = copystore(ui, srcrepo, destpath)
            # copy bookmarks over
            srcbookmarks = srcrepo.join('bookmarks')
            dstbookmarks = os.path.join(destpath, 'bookmarks')
            if os.path.exists(srcbookmarks):
                util.copyfile(srcbookmarks, dstbookmarks)

            # Recomputing branch cache might be slow on big repos,
            # so just copy it
            def copybranchcache(fname):
                srcbranchcache = srcrepo.join('cache/%s' % fname)
                dstbranchcache = os.path.join(dstcachedir, fname)
                if os.path.exists(srcbranchcache):
                    if not os.path.exists(dstcachedir):
                        os.mkdir(dstcachedir)
                    util.copyfile(srcbranchcache, dstbranchcache)

            dstcachedir = os.path.join(destpath, 'cache')
            # In local clones we're copying all nodes, not just served
            # ones. Therefore copy all branch caches over.
            copybranchcache('branch2')
            for cachename in repoview.filtertable:
                copybranchcache('branch2-%s' % cachename)

            # we need to re-init the repo after manually copying the data
            # into it
            destpeer = peer(srcrepo, peeropts, dest)
            srcrepo.hook('outgoing', source='clone',
                          node=node.hex(node.nullid))
        else:
            try:
                destpeer = peer(srcrepo or ui, peeropts, dest, create=True)
                                # only pass ui when no srcrepo
            except OSError as inst:
                if inst.errno == errno.EEXIST:
                    cleandir = None
                    raise error.Abort(_("destination '%s' already exists")
                                     % dest)
                raise

            revs = None
            if rev:
                if not srcpeer.capable('lookup'):
                    raise error.Abort(_("src repository does not support "
                                       "revision lookup and so doesn't "
                                       "support clone by revision"))
                revs = [srcpeer.lookup(r) for r in rev]
                checkout = revs[0]
            local = destpeer.local()
            if local:
                if not stream:
                    if pull:
                        stream = False
                    else:
                        stream = None
                # internal config: ui.quietbookmarkmove
                quiet = local.ui.backupconfig('ui', 'quietbookmarkmove')
                try:
                    local.ui.setconfig(
                        'ui', 'quietbookmarkmove', True, 'clone')
                    exchange.pull(local, srcpeer, revs,
                                  streamclonerequested=stream)
                finally:
                    local.ui.restoreconfig(quiet)
            elif srcrepo:
                exchange.push(srcrepo, destpeer, revs=revs,
                              bookmarks=srcrepo._bookmarks.keys())
            else:
                raise error.Abort(_("clone from remote to remote not supported")
                                 )

        cleandir = None

        destrepo = destpeer.local()
        if destrepo:
            template = uimod.samplehgrcs['cloned']
            fp = destrepo.vfs("hgrc", "w", text=True)
            u = util.url(abspath)
            u.passwd = None
            defaulturl = str(u)
            fp.write(template % defaulturl)
            fp.close()

            destrepo.ui.setconfig('paths', 'default', defaulturl, 'clone')

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
                        uprev = destrepo._bookmarks['@']
                        update = '@'
                        bn = destrepo[uprev].branch()
                        if bn == 'default':
                            status = _("updating to bookmark @\n")
                        else:
                            status = (_("updating to bookmark @ on branch %s\n")
                                      % bn)
                    except KeyError:
                        try:
                            uprev = destrepo.branchtip('default')
                        except error.RepoLookupError:
                            uprev = destrepo.lookup('tip')
                if not status:
                    bn = destrepo[uprev].branch()
                    status = _("updating to branch %s\n") % bn
                destrepo.ui.status(status)
                _update(destrepo, uprev)
                if update in destrepo._bookmarks:
                    bookmarks.activate(destrepo, update)
    finally:
        release(srclock, destlock)
        if cleandir is not None:
            shutil.rmtree(cleandir, True)
        if srcpeer is not None:
            srcpeer.close()
    return srcpeer, destpeer

def _showstats(repo, stats, quietempty=False):
    if quietempty and not any(stats):
        return
    repo.ui.status(_("%d files updated, %d files merged, "
                     "%d files removed, %d files unresolved\n") % stats)

def updaterepo(repo, node, overwrite):
    """Update the working directory to node.

    When overwrite is set, changes are clobbered, merged else

    returns stats (see pydoc mercurial.merge.applyupdates)"""
    return mergemod.update(repo, node, False, overwrite,
                           labels=['working copy', 'destination'])

def update(repo, node, quietempty=False):
    """update the working directory to node, merging linear changes"""
    stats = updaterepo(repo, node, False)
    _showstats(repo, stats, quietempty)
    if stats[3]:
        repo.ui.status(_("use 'hg resolve' to retry unresolved file merges\n"))
    return stats[3] > 0

# naming conflict in clone()
_update = update

def clean(repo, node, show_stats=True, quietempty=False):
    """forcibly switch the working directory to node, clobbering changes"""
    stats = updaterepo(repo, node, True)
    util.unlinkpath(repo.join('graftstate'), ignoremissing=True)
    if show_stats:
        _showstats(repo, stats, quietempty)
    return stats[3] > 0

# naming conflict in updatetotally()
_clean = clean

def updatetotally(ui, repo, checkout, brev, clean=False, check=False):
    """Update the working directory with extra care for non-file components

    This takes care of non-file components below:

    :bookmark: might be advanced or (in)activated

    This takes arguments below:

    :checkout: to which revision the working directory is updated
    :brev: a name, which might be a bookmark to be activated after updating
    :clean: whether changes in the working directory can be discarded
    :check: whether changes in the working directory should be checked

    This returns whether conflict is detected at updating or not.
    """
    with repo.wlock():
        movemarkfrom = None
        warndest = False
        if checkout is None:
            updata = destutil.destupdate(repo, clean=clean, check=check)
            checkout, movemarkfrom, brev = updata
            warndest = True

        if clean:
            ret = _clean(repo, checkout)
        else:
            ret = _update(repo, checkout)

        if not ret and movemarkfrom:
            if movemarkfrom == repo['.'].node():
                pass # no-op update
            elif bookmarks.update(repo, [movemarkfrom], repo['.'].node()):
                ui.status(_("updating bookmark %s\n") % repo._activebookmark)
            else:
                # this can happen with a non-linear update
                ui.status(_("(leaving bookmark %s)\n") %
                          repo._activebookmark)
                bookmarks.deactivate(repo)
        elif brev in repo._bookmarks:
            if brev != repo._activebookmark:
                ui.status(_("(activating bookmark %s)\n") % brev)
            bookmarks.activate(repo, brev)
        elif brev:
            if repo._activebookmark:
                ui.status(_("(leaving bookmark %s)\n") %
                          repo._activebookmark)
            bookmarks.deactivate(repo)

        if warndest:
            destutil.statusotherdests(ui, repo)

    return ret

def merge(repo, node, force=None, remind=True, mergeforce=False):
    """Branch merge with node, resolving changes. Return true if any
    unresolved conflicts."""
    stats = mergemod.update(repo, node, True, force, mergeforce=mergeforce)
    _showstats(repo, stats)
    if stats[3]:
        repo.ui.status(_("use 'hg resolve' to retry unresolved file merges "
                         "or 'hg update -C .' to abandon\n"))
    elif remind:
        repo.ui.status(_("(branch merge, don't forget to commit)\n"))
    return stats[3] > 0

def _incoming(displaychlist, subreporecurse, ui, repo, source,
        opts, buffered=False):
    """
    Helper for incoming / gincoming.
    displaychlist gets called with
        (remoterepo, incomingchangesetlist, displayer) parameters,
    and is supposed to contain only code that can't be unified.
    """
    source, branches = parseurl(ui.expandpath(source), opts.get('branch'))
    other = peer(repo, opts, source)
    ui.status(_('comparing with %s\n') % util.hidepassword(source))
    revs, checkout = addbranchrevs(repo, other, branches, opts.get('rev'))

    if revs:
        revs = [other.lookup(rev) for rev in revs]
    other, chlist, cleanupfn = bundlerepo.getremotechanges(ui, repo, other,
                                revs, opts["bundle"], opts["force"])
    try:
        if not chlist:
            ui.status(_("no changes found\n"))
            return subreporecurse()

        displayer = cmdutil.show_changeset(ui, other, opts, buffered)
        displaychlist(other, chlist, displayer)
        displayer.close()
    finally:
        cleanupfn()
    subreporecurse()
    return 0 # exit code is zero since we found incoming changes

def incoming(ui, repo, source, opts):
    def subreporecurse():
        ret = 1
        if opts.get('subrepos'):
            ctx = repo[None]
            for subpath in sorted(ctx.substate):
                sub = ctx.sub(subpath)
                ret = min(ret, sub.incoming(ui, source, opts))
        return ret

    def display(other, chlist, displayer):
        limit = cmdutil.loglimit(opts)
        if opts.get('newest_first'):
            chlist.reverse()
        count = 0
        for n in chlist:
            if limit is not None and count >= limit:
                break
            parents = [p for p in other.changelog.parents(n) if p != nullid]
            if opts.get('no_merges') and len(parents) == 2:
                continue
            count += 1
            displayer.show(other[n])
    return _incoming(display, subreporecurse, ui, repo, source, opts)

def _outgoing(ui, repo, dest, opts):
    dest = ui.expandpath(dest or 'default-push', dest or 'default')
    dest, branches = parseurl(dest, opts.get('branch'))
    ui.status(_('comparing with %s\n') % util.hidepassword(dest))
    revs, checkout = addbranchrevs(repo, repo, branches, opts.get('rev'))
    if revs:
        revs = [repo.lookup(rev) for rev in scmutil.revrange(repo, revs)]

    other = peer(repo, opts, dest)
    outgoing = discovery.findcommonoutgoing(repo.unfiltered(), other, revs,
                                            force=opts.get('force'))
    o = outgoing.missing
    if not o:
        scmutil.nochangesfound(repo.ui, repo, outgoing.excluded)
    return o, other

def outgoing(ui, repo, dest, opts):
    def recurse():
        ret = 1
        if opts.get('subrepos'):
            ctx = repo[None]
            for subpath in sorted(ctx.substate):
                sub = ctx.sub(subpath)
                ret = min(ret, sub.outgoing(ui, dest, opts))
        return ret

    limit = cmdutil.loglimit(opts)
    o, other = _outgoing(ui, repo, dest, opts)
    if not o:
        cmdutil.outgoinghooks(ui, repo, other, opts, o)
        return recurse()

    if opts.get('newest_first'):
        o.reverse()
    displayer = cmdutil.show_changeset(ui, repo, opts)
    count = 0
    for n in o:
        if limit is not None and count >= limit:
            break
        parents = [p for p in repo.changelog.parents(n) if p != nullid]
        if opts.get('no_merges') and len(parents) == 2:
            continue
        count += 1
        displayer.show(repo[n])
    displayer.close()
    cmdutil.outgoinghooks(ui, repo, other, opts, o)
    recurse()
    return 0 # exit code is zero since we found outgoing changes

def verify(repo):
    """verify the consistency of a repository"""
    ret = verifymod.verify(repo)

    # Broken subrepo references in hidden csets don't seem worth worrying about,
    # since they can't be pushed/pulled, and --hidden can be used if they are a
    # concern.

    # pathto() is needed for -R case
    revs = repo.revs("filelog(%s)",
                     util.pathto(repo.root, repo.getcwd(), '.hgsubstate'))

    if revs:
        repo.ui.status(_('checking subrepo links\n'))
        for rev in revs:
            ctx = repo[rev]
            try:
                for subpath in ctx.substate:
                    try:
                        ret = (ctx.sub(subpath, allowcreate=False).verify()
                               or ret)
                    except error.RepoError as e:
                        repo.ui.warn(_('%s: %s\n') % (rev, e))
            except Exception:
                repo.ui.warn(_('.hgsubstate is corrupt in revision %s\n') %
                             node.short(ctx.node()))

    return ret

def remoteui(src, opts):
    'build a remote ui from ui or repo and opts'
    if util.safehasattr(src, 'baseui'): # looks like a repository
        dst = src.baseui.copy() # drop repo-specific config
        src = src.ui # copy target options from repo
    else: # assume it's a global ui object
        dst = src.copy() # keep all global options

    # copy ssh-specific options
    for o in 'ssh', 'remotecmd':
        v = opts.get(o) or src.config('ui', o)
        if v:
            dst.setconfig("ui", o, v, 'copied')

    # copy bundle-specific options
    r = src.config('bundle', 'mainreporoot')
    if r:
        dst.setconfig('bundle', 'mainreporoot', r, 'copied')

    # copy selected local settings to the remote ui
    for sect in ('auth', 'hostfingerprints', 'http_proxy'):
        for key, val in src.configitems(sect):
            dst.setconfig(sect, key, val, 'copied')
    v = src.config('web', 'cacerts')
    if v == '!':
        dst.setconfig('web', 'cacerts', v, 'copied')
    elif v:
        dst.setconfig('web', 'cacerts', util.expandpath(v), 'copied')

    return dst

# Files of interest
# Used to check if the repository has changed looking at mtime and size of
# these files.
foi = [('spath', '00changelog.i'),
       ('spath', 'phaseroots'), # ! phase can change content at the same size
       ('spath', 'obsstore'),
       ('path', 'bookmarks'), # ! bookmark can change content at the same size
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
        self._filtername = repo.filtername

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
        if self._filtername:
            self._repo = repo.filtered(self._filtername)
        else:
            self._repo = repo.unfiltered()
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
                st = os.stat(p)
            except OSError:
                st = os.stat(prefix)
            state.append((st.st_mtime, st.st_size))
            maxmtime = max(maxmtime, st.st_mtime)

        return tuple(state), maxmtime

    def copy(self):
        """Obtain a copy of this class instance.

        A new localrepository instance is obtained. The new instance should be
        completely independent of the original.
        """
        repo = repository(self._repo.baseui, self._repo.origroot)
        if self._filtername:
            repo = repo.filtered(self._filtername)
        else:
            repo = repo.unfiltered()
        c = cachedlocalrepo(repo)
        c._state = self._state
        c.mtime = self.mtime
        return c
