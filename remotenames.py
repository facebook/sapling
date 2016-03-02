"""
remotenames: a mercurial extension for improving client/server workflows

The remotenames extension provides additional information to clients that is
particularly useful when pushing and pulling to peer repositories.

Before diving in to using remotebookmarks, we suggest you read the included
README file, which explains the changes to expect, the configuration knobs
available (note: almost everything is configurable), and gives examples of
how to set up the configuration options in useful ways.

This extension is the work of Sean Farley forked from Augie Fackler's seminal
remotebranches extension. Ryan McElroy of Facebook also contributed.
"""

import os
import re
import errno
import shutil
import UserDict

from mercurial import bookmarks
from mercurial import commands
from mercurial import discovery
from mercurial import encoding
from mercurial import error
from mercurial import exchange
from mercurial import extensions
from mercurial import hg
from mercurial import localrepo
from mercurial import lock as lockmod
from mercurial import namespaces
from mercurial import obsolete
from mercurial import repair
from mercurial import repoview
from mercurial import revset
from mercurial import scmutil
from mercurial import templatekw
from mercurial import url
from mercurial import util
from mercurial.i18n import _
from mercurial.node import hex, short, bin
from hgext import schemes
from hgext.convert import hg as converthg

def exbookcalcupdate(orig, ui, repo, checkout):
    '''Return a tuple (targetrev, movemarkfrom) indicating the rev to
    check out and where to move the active bookmark from, if needed.'''
    movemarkfrom = None
    if checkout is None:
        activemark = bmactive(repo)
        if not activemark:
            # if no active bookmark then keep using the old code path for now
            return orig(ui, repo, checkout)
        if bookmarks.isactivewdirparent(repo):
            movemarkfrom = repo['.'].node()
        ui.status(_("updating to active bookmark %s\n") % activemark)
        checkout = activemark
    return (checkout, movemarkfrom)

def expush(orig, repo, remote, *args, **kwargs):
    res = orig(repo, remote, *args, **kwargs)
    pullremotenames(repo, remote)
    return res

def expushop(orig, pushop, repo, remote, force=False, revs=None,
             newbranch=False, bookmarks=(), **kwargs):
    orig(pushop, repo, remote, force, revs, newbranch, bookmarks)

    for flag in ['to', 'delete', 'create', 'allowanon', 'nonforwardmove']:
        setattr(pushop, flag, kwargs.get(flag))

def expull(orig, repo, remote, *args, **kwargs):
    res = orig(repo, remote, *args, **kwargs)
    pullremotenames(repo, remote)
    return res

def pullremotenames(repo, remote):
    path = activepath(repo.ui, remote)
    if path:
        # on a push, we don't want to keep obsolete heads since
        # they won't show up as heads on the next pull, so we
        # remove them here otherwise we would require the user
        # to issue a pull to refresh .hg/remotenames
        bmap = {}
        repo = repo.unfiltered()
        for branch, nodes in remote.branchmap().iteritems():
            bmap[branch] = []
            for node in nodes:
                if node in repo and not repo[node].obsolete():
                    bmap[branch].append(node)
        saveremotenames(repo, path, bmap, remote.listkeys('bookmarks'))

    precachedistance(repo)

def blockerhook(orig, repo, *args, **kwargs):
    blockers = orig(repo)

    unblock = util.safehasattr(repo, '_unblockhiddenremotenames')
    if not unblock:
        return blockers

    # add remotenames to blockers by looping over all names in our own cache
    cl = repo.changelog
    for remotename in repo._remotenames.keys():
        rname = 'remote' + remotename
        try:
            ns = repo.names[rname]
        except KeyError:
            continue
        for name in ns.listnames(repo):
            blockers.update(cl.rev(node) for node in ns.nodes(repo, name))

    return blockers

def exupdatefromremote(orig, ui, repo, remotemarks, path, trfunc, explicit=()):
    if ui.configbool('remotenames', 'syncbookmarks', False):
        return orig(ui, repo, remotemarks, path, trfunc, explicit)

    ui.debug('remotenames: skipped syncing local bookmarks\n')

def exclone(orig, ui, *args, **opts):
    """
    We may not want local bookmarks on clone... but we always want remotenames!
    """
    srcpeer, dstpeer = orig(ui, *args, **opts)

    pullremotenames(dstpeer.local(), srcpeer)

    if not ui.configbool('remotenames', 'syncbookmarks', False):
        ui.debug('remotenames: removing cloned bookmarks\n')
        repo = dstpeer.local()
        wlock = repo.wlock()
        try:
            try:
                vfs = shareawarevfs(repo)
                vfs.unlink('bookmarks')
            except OSError as inst:
                if inst.errno != errno.ENOENT:
                    raise
        finally:
            wlock.release()

    return (srcpeer, dstpeer)

def excommit(orig, repo, *args, **opts):
    res = orig(repo, *args, **opts)
    precachedistance(repo)
    return res

def exupdate(orig, repo, *args, **opts):
    res = orig(repo, *args, **opts)
    precachedistance(repo)
    return res

def exactivate(orig, repo, mark):
    res = orig(repo, mark)
    precachedistance(repo)
    return res

def exconvertbookmarks(orig, source):
    """Make hg convert map remote bookmarks in the source to normal bookmarks in
    the target.

    This is useful for instance if you need to convert a repo from server A to
    server B. You clone the repo from A (now you have remote bookmarks), convert
    to a local version of B, and push those bookmarks to server B.
    """
    bookmarks = orig(source)

    repo = source.repo
    n = 'remotebookmarks'
    if n in repo.names:
        ns = repo.names[n]
        for name in ns.listnames(repo):
            nodes = ns.nodes(repo, name)
            if nodes:
                bookmarks.setdefault(name, hex(nodes[0]))

    return bookmarks

def updatecmd(orig, ui, repo, node=None, rev=None, **kwargs):
    book = kwargs.get('bookmark')
    if book:
        del kwargs['bookmark']
        if book in repo._bookmarks:
            raise error.Abort("bookmark '%s' already exists" % book)
        ret = orig(ui, repo, node=node, rev=rev, **kwargs)
        commands.bookmark(ui, repo, book)

        if not _tracking(ui):
            return ret

        oldtracking = _readtracking(repo)
        tracking = dict(oldtracking)

        if node:
            tracking[book] = node
        elif rev:
            tracking[book] = rev

        if tracking != oldtracking:
            _writetracking(repo, tracking)
            # update the cache
            precachedistance(repo)
        return ret
    if 'bookmark' in kwargs:
        del kwargs['bookmark']
    return orig(ui, repo, node=node, rev=rev, **kwargs)

class lazyremotenamedict(UserDict.DictMixin):
    """Read-only dict-like Class to lazily resolve remotename entries

    We are doing that because remotenames startup was slow.
    We read the remotenames file once to figure out the potential entries
    and store them in self.potentialentries. Then when asked to resolve an
    entry, if it is not in self.potentialentries, then it isn't there, if it
    is in self.potentialentries we resolve it and store the result in
    self.cache. We cannot be lazy is when asked all the entries (keys).
    """
    def __init__(self, kind, repo):
        self.cache = {}
        self.potentialentries = {}
        self._kind = kind # bookmarks or branches
        self._repo = repo
        self._load()

    def _load(self):
        """Read the remotenames file, store entries matching selected kind"""
        repo = self._repo
        alias_default = repo.ui.configbool('remotenames', 'alias.default')
        for node, nametype, remote, rname in readremotenames(repo):
            if nametype != self._kind:
                continue
            # handle alias_default here
            if remote != "default" and rname == "default" and alias_default:
                name = remote
            else:
                name = joinremotename(remote, rname)
            self.potentialentries[name] = (node, nametype, remote, rname)

    def _resolvedata(self, potentialentry):
        """Check that the node for potentialentry exists and return it"""
        if not potentialentry in self.potentialentries:
            return None
        node, nametype, remote, rname = self.potentialentries[potentialentry]
        repo = self._repo
        binnode = bin(node)
        # if the node doesn't exist, skip it
        try:
            repo.changelog.rev(binnode)
        except LookupError:
            return None
        # Skip closed branches
        if (nametype == 'branches' and _branchesenabled(repo.ui) and
                repo[binnode].closesbranch()):
            return None
        return [binnode]

    def __getitem__(self, key):
        val = self._fetchandcache(key)
        if val is not None:
            return val
        else:
            raise KeyError()

    def _fetchandcache(self, key):
        if key in self.cache:
            return self.cache[key]
        val = self._resolvedata(key)
        if val is not None:
            self.cache[key] = val
            return val
        else:
            return None

    def keys(self):
        for u in self.potentialentries.keys():
            self._fetchandcache(u)
        return self.cache.keys()

class remotenames(dict):
    """This class encapsulates all the remotenames state. It also contains
    methods to access that state in convenient ways. Remotenames are lazy
    loaded. Whenever client code needs to ensure the freshest copy of
    remotenames, use the `clearnames` method to force an eventual load.
    """

    def __init__(self, repo, *args):
        dict.__init__(self, *args)
        self._repo = repo
        self.clearnames()

    def clearnames(self):
        """Clear all remote names state"""
        self['bookmarks'] = lazyremotenamedict("bookmarks", self._repo)
        self['branches'] = lazyremotenamedict("branches", self._repo)
        self._invalidatecache()
        self._loadednames = False

    def _invalidatecache(self):
        self._node2marks = None
        self._hoist2nodes = None
        self._node2hoists = None
        self._node2branch = None

    def mark2nodes(self):
        return self['bookmarks']

    def node2marks(self):
        if not self._node2marks:
            mark2nodes = self.mark2nodes()
            self._node2marks = {}
            for name, node in mark2nodes.iteritems():
                self._node2marks.setdefault(node[0], []).append(name)
        return self._node2marks

    def hoist2nodes(self, hoist):
        if not self._hoist2nodes:
            mark2nodes = self.mark2nodes()
            self._hoist2nodes = {}
            hoist += '/'
            for name, node in mark2nodes.iteritems():
                if name.startswith(hoist):
                    name = name[len(hoist):]
                    self._hoist2nodes[name] = node
        return self._hoist2nodes

    def node2hoists(self, hoist):
        if not self._node2hoists:
            mark2nodes = self.mark2nodes()
            self._node2hoists = {}
            hoist += '/'
            for name, node in mark2nodes.iteritems():
                if name.startswith(hoist):
                    name = name[len(hoist):]
                    self._node2hoists.setdefault(node[0], []).append(name)
        return self._node2hoists

    def branch2nodes(self):
        return self['branches']

    def node2branch(self):
        if not self._node2branch:
            branch2nodes = self.branch2nodes()
            self._node2branch = {}
            for name, nodes in branch2nodes.iteritems():
                for node in nodes:
                    self._node2branch[node] = [name]
        return self._node2branch


def reposetup(ui, repo):
    if not repo.local():
        return

    repo._remotenames = remotenames(repo)
    ns = namespaces.namespace

    if ui.configbool('remotenames', 'bookmarks', True):
        remotebookmarkns = ns(
            'remotebookmarks',
            templatename='remotebookmarks',
            logname='bookmark',
            colorname='remotebookmark',
            listnames=lambda repo: repo._remotenames.mark2nodes().keys(),
            namemap=lambda repo, name:
                repo._remotenames.mark2nodes().get(name, None),
            nodemap=lambda repo, node:
                repo._remotenames.node2marks().get(node, []))
        repo.names.addnamespace(remotebookmarkns)

        # hoisting only works if there are remote bookmarks
        hoist = ui.config('remotenames', 'hoist', 'default')
        if hoist:
            hoistednamens = ns(
                'hoistednames',
                templatename='hoistednames',
                logname='hoistedname',
                colorname='hoistedname',
                listnames = lambda repo:
                    repo._remotenames.hoist2nodes(hoist).keys(),
                namemap = lambda repo, name:
                    repo._remotenames.hoist2nodes(hoist).get(name, None),
                nodemap = lambda repo, node:
                    repo._remotenames.node2hoists(hoist).get(node, []))
            repo.names.addnamespace(hoistednamens)

    if _branchesenabled(ui):
        remotebranchns = ns(
            'remotebranches',
            templatename='remotebranches',
            logname='branch',
            colorname='remotebranch',
            listnames = lambda repo: repo._remotenames.branch2nodes().keys(),
            namemap = lambda repo, name:
                repo._remotenames.branch2nodes().get(name, None),
            nodemap = lambda repo, node:
                repo._remotenames.node2branch().get(node, []))
        repo.names.addnamespace(remotebranchns)

def _tracking(ui):
    # omg default true
    return ui.configbool('remotenames', 'tracking', True)

def _branchesenabled(ui):
    return ui.configbool('remotenames', 'branches', True)

def exrebasecmd(orig, ui, repo, **opts):
    dest = opts['dest']
    source = opts['source']
    revs = opts['rev']
    base = opts['base']
    cont = opts['continue']
    abort = opts['abort']

    current = bmactive(repo)

    if not (cont or abort or dest or source or revs or base) and current:
        tracking = _readtracking(repo)
        if current in tracking:
            opts['dest'] = tracking[current]

    ret = orig(ui, repo, **opts)
    precachedistance(repo)
    return ret

def exstrip(orig, ui, repo, *args, **opts):
    ret = orig(ui, repo, *args, **opts)
    precachedistance(repo)
    return ret

def exhistedit(orig, ui, repo, *args, **opts):
    ret = orig(ui, repo, *args, **opts)
    precachedistance(repo)
    return ret

def expaths(orig, ui, repo, *args, **opts):
    """allow adding and removing remote paths

    This is very hacky and only exists as an experimentation.

    """
    delete = opts.get('delete')
    add = opts.get('add')
    if delete:
        # find the first section and remote path that matches, and delete that
        foundpaths = False
        if not repo.vfs.isfile('hgrc'):
            raise error.Abort(_("could not find hgrc file"))
        oldhgrc = repo.vfs.read('hgrc').splitlines(True)
        f = repo.vfs('hgrc', 'w')
        for line in oldhgrc:
            if '[paths]' in line:
                foundpaths = True
            if not (foundpaths and line.strip().startswith(delete)):
                f.write(line)
        f.close()
        saveremotenames(repo, delete)
        precachedistance(repo)
        return

    if add:
        # find the first section that matches, then look for previous value; if
        # not found add a new entry
        foundpaths = False
        oldhgrc = []
        if repo.vfs.isfile("hgrc"):
            oldhgrc = repo.vfs.read('hgrc').splitlines(True)
        f = repo.vfs('hgrc', 'w')
        done = False
        for line in oldhgrc:
            if '[paths]' in line:
                foundpaths = True
            if foundpaths and line.strip().startswith(add):
                done = True
                line = '%s = %s\n' % (add, args[0])
            f.write(line)

        # did we not find an existing path?
        if not done:
            done = True
            f.write("[paths]\n")
            f.write("%s = %s\n" % (add, args[0]))

        f.close()
        return

    return orig(ui, repo, *args)

def exnowarnheads(orig, pushop):
    heads = orig(pushop)
    if pushop.to:
        repo = pushop.repo.unfiltered()
        rev = pushop.revs[0]
        heads.add(repo[rev].node())
    return heads

def exstripbmrevset(orig, repo, mark):
    return orig(repo, mark) - repo.revs("ancestors(remotenames() and "
                                        "not bookmark(%s))", mark)

def extsetup(ui):
    extensions.wrapfunction(bookmarks, 'calculateupdate', exbookcalcupdate)
    extensions.wrapfunction(exchange.pushoperation, '__init__', expushop)
    extensions.wrapfunction(exchange, 'push', expush)
    extensions.wrapfunction(exchange, 'pull', expull)
    extensions.wrapfunction(repoview, '_getdynamicblockers', blockerhook)
    extensions.wrapfunction(bookmarks, 'updatefromremote', exupdatefromremote)
    extensions.wrapfunction(repair, 'stripbmrevset', exstripbmrevset)
    if util.safehasattr(bookmarks, 'activate'):
        extensions.wrapfunction(bookmarks, 'activate', exactivate)
    else:
        extensions.wrapfunction(bookmarks, 'setcurrent', exactivate)
    extensions.wrapfunction(hg, 'clone', exclone)
    extensions.wrapfunction(hg, 'updaterepo', exupdate)
    extensions.wrapfunction(localrepo.localrepository, 'commit', excommit)

    extensions.wrapfunction(converthg.mercurial_source, 'getbookmarks',
                            exconvertbookmarks)

    if util.safehasattr(discovery, '_nowarnheads'):
        extensions.wrapfunction(discovery, '_nowarnheads', exnowarnheads)

    if _tracking(ui):
        try:
            rebase = extensions.find('rebase')
            extensions.wrapcommand(rebase.cmdtable, 'rebase', exrebasecmd)
        except KeyError:
            # rebase isn't on, that's fine
            pass

    entry = extensions.wrapcommand(commands.table, 'log', exlog)
    entry[1].append(('', 'remote', None, 'show remote names even if hidden'))

    entry = extensions.wrapcommand(commands.table, 'paths', expaths)
    entry[1].append(('d', 'delete', '', 'delete remote path', 'NAME'))
    entry[1].append(('a', 'add', '', 'add remote path', 'NAME PATH'))

    extensions.wrapcommand(commands.table, 'pull', expullcmd)

    entry = extensions.wrapcommand(commands.table, 'clone', exclonecmd)
    entry[1].append(('', 'mirror', None, 'sync all bookmarks'))

    entry = extensions.wrapcommand(commands.table, 'update', updatecmd)
    entry[1].append(('B', 'bookmark', '', 'create new bookmark'))

    exchange.pushdiscoverymapping['bookmarks'] = expushdiscoverybookmarks

    templatekw.keywords['remotenames'] = remotenameskw

    try:
        strip = extensions.find('strip')
        if strip:
            extensions.wrapcommand(strip.cmdtable, 'strip', exstrip)
    except KeyError:
        # strip isn't on
        pass

    try:
        histedit = extensions.find('histedit')
        if histedit:
            extensions.wrapcommand(histedit.cmdtable, 'histedit', exhistedit)
    except KeyError:
        # histedit isn't on
        pass

    bookcmd = extensions.wrapcommand(commands.table, 'bookmarks', exbookmarks)
    branchcmd = extensions.wrapcommand(commands.table, 'branches', exbranches)
    pushcmd = extensions.wrapcommand(commands.table, 'push', expushcmd)

    if _tracking(ui):
        bookcmd[1].append(('t', 'track', '',
                          'track this bookmark or remote name', 'BOOKMARK'))
        bookcmd[1].append(('u', 'untrack', None,
                           'remove tracking for this bookmark', 'BOOKMARK'))

    newopts = [
        (bookcmd, ('a', 'all', None, 'show both remote and local bookmarks')),
        (bookcmd, ('', 'remote', None, 'show only remote bookmarks')),
        (branchcmd, ('a', 'all', None, 'show both remote and local branches')),
        (branchcmd, ('', 'remote', None, 'show only remote branches')),
        (pushcmd, ('t', 'to', '', 'push revs to this bookmark', 'BOOKMARK')),
        (pushcmd, ('d', 'delete', '', 'delete remote bookmark', 'BOOKMARK')),
        (pushcmd, ('', 'create', None, 'create a new remote bookmark')),
        (pushcmd, ('', 'allow-anon', None, 'allow a new unbookmarked head')),
        (pushcmd, ('', 'non-forward-move', None,
                   'allows moving a remote bookmark to an arbitrary place')),
    ]

    def afterload(loaded):
        if loaded:
            raise ValueError('nonexistant extension should not be loaded')

        for cmd, newopt in newopts:
            # avoid adding duplicate optionms
            skip = False
            for opt in cmd[1]:
                if opt[1] == newopt[1]:
                    skip = True
            if not skip:
                cmd[1].append(newopt)

    extensions.afterloaded('nonexistant', afterload)

def exlog(orig, ui, repo, *args, **opts):
    # hack for logging that turns on the dynamic blockerhook
    if opts.get('remote'):
        repo.__setattr__('_unblockhiddenremotenames', True)
    res = orig(ui, repo, *args, **opts)
    if opts.get('remote'):
        repo.__setattr__('_unblockhiddenremotenames', False)
    return res

def expushdiscoverybookmarks(pushop):
    repo = pushop.repo.unfiltered()
    remotemarks = pushop.remote.listkeys('bookmarks')

    if pushop.delete:
        if pushop.delete not in remotemarks:
            raise error.Abort(_('remote bookmark %s does not exist') %
                             pushop.delete)
        pushop.outbookmarks.append([pushop.delete, remotemarks[pushop.delete],
                                    ''])
        return exchange._pushdiscoverybookmarks(pushop)

    if not pushop.to:
        ret = exchange._pushdiscoverybookmarks(pushop)
        if not pushop.allowanon:
            # check to make sure we don't push an anonymous head
            if pushop.revs:
                revs = set(pushop.revs)
            else:
                revs = set(repo.lookup(r) for r in repo.revs('head()'))
            revs -= set(pushop.remoteheads)
            # find heads that don't have a bookmark going with them
            for bookmark in pushop.bookmarks:
                rev = repo.lookup(bookmark)
                if rev in revs:
                    revs.remove(rev)
            # remove heads that advance bookmarks (old mercurial behavior)
            for bookmark, old, new in pushop.outbookmarks:
                rev = repo.lookup(new)
                if rev in revs:
                    revs.remove(rev)

            # we use known() instead of lookup() due to lookup throwing an
            # aborting error causing the connection to close
            anonheads = []
            knownlist = pushop.remote.known(revs)
            for node, known in zip(revs, knownlist):
                obs = repo[node].obsolete()
                closes = repo[node].closesbranch()
                if known or obs or closes:
                    continue
                anonheads.append(short(node))

            if anonheads:
                msg = _("push would create new anonymous heads (%s)")
                hint = _("use --allow-anon to override this warning")
                raise error.Abort(msg % ', '.join(sorted(anonheads)), hint=hint)
        return ret

    bookmark = pushop.bookmarks[0]
    rev = pushop.revs[0]

    # allow new bookmark only if --create is specified
    old = ''
    if bookmark in remotemarks:
        old = remotemarks[bookmark]
    elif not pushop.create:
        msg = _('not creating new remote bookmark')
        hint = _('use --create to create a new bookmark')
        raise error.Abort(msg, hint=hint)

    # allow non-fg bookmark move only if --non-forward-move is specified
    if not pushop.nonforwardmove and old != '':
        # the first check isn't technically about non-fg moves, but the non-fg
        # check relies on the old bm location being in the local repo
        if old not in repo:
            msg = _('remote bookmark revision is not in local repo')
            hint = _('pull and merge or rebase or use --non-forward-move')
            raise error.Abort(msg, hint=hint)
        foreground = obsolete.foreground(repo, [repo.lookup(old)])
        if repo[rev].node() not in foreground:
            msg = _('pushed rev is not in the foreground of remote bookmark')
            hint = _('use --non-forward-move flag to complete arbitrary moves')
            raise error.Abort(msg, hint=hint)
        if repo[old] == repo[rev]:
            repo.ui.status(_('remote bookmark already points at pushed rev\n'))
            return

    pushop.outbookmarks.append((bookmark, old, hex(rev)))

def _pushrevs(repo, ui, rev):
    """Given configuration and default rev, return the revs to be pushed"""
    pushrev = ui.config('remotenames', 'pushrev')
    if pushrev == '!':
        return []
    elif pushrev:
        return [repo[pushrev].rev()]
    if rev:
        return [repo[rev].rev()]
    return []

def expullcmd(orig, ui, repo, source="default", **opts):
    revrenames = dict((v, k) for k, v in _getrenames(ui).iteritems())
    source = revrenames.get(source, source)

    if opts.get('update') and opts.get('rebase'):
        raise error.Abort(_('specify either rebase or update, not both'))

    if not opts.get('rebase'):
        return orig(ui, repo, source, **opts)

    rebasemodule = extensions.find('rebase')

    if not rebasemodule:
        return orig(ui, repo, source, **opts)

    if not _tracking(ui):
        return orig(ui, repo, source, **opts)

    dest = _getrebasedest(repo, opts)

    if dest:
        # Let `pull` do its thing without `rebase.py->pullrebase()`
        del opts['rebase']
        ret = orig(ui, repo, source, **opts)
        return ret or rebasemodule.rebase(ui, repo, dest=dest)
    else:
        return orig(ui, repo, source, **opts)

def _getrebasedest(repo, opts):
    """opts is passed in for extensibility"""
    tracking = _readtracking(repo)
    active = bmactive(repo)
    return tracking.get(active)

def expushcmd(orig, ui, repo, dest=None, **opts):
    # during the upgrade from old to new remotenames, tooling that uses --force
    # will continue working if remotenames.forcecompat is enabled
    forcecompat = ui.configbool('remotenames', 'forcecompat')

    # needed for discovery method
    opargs = {
        'delete': opts.get('delete'),
        'to': opts.get('to'),
        'create': opts.get('create') or (opts.get('force') and forcecompat),
        'allowanon': opts.get('allow_anon') or
                     repo.ui.configbool('remotenames', 'pushanonheads') or
                     (opts.get('force') and forcecompat),
        'nonforwardmove': opts.get('non_forward_move') or
                repo.ui.configbool('remotenames', 'allownonfastforward') or
                (opts.get('force') and forcecompat),
    }

    if opargs['delete']:
        flag = None
        for f in ('to', 'bookmark', 'branch', 'rev'):
            if opts.get(f):
                flag = f
                break
        if flag:
            msg = _('do not specify --delete and '
                    '--%s at the same time') % flag
            raise error.Abort(msg)
        # we want to skip pushing any changesets while deleting a remote
        # bookmark, so we send the null revision
        opts['rev'] = ['null']
        return orig(ui, repo, dest, opargs=opargs, **opts)

    revs = opts.get('rev')

    paths = dict((path, url) for path, url in ui.configitems('paths'))
    revrenames = dict((v, k) for k, v in _getrenames(ui).iteritems())

    origdest = dest
    if not dest and not opargs['to'] and not revs and _tracking(ui):
        current = bmactive(repo)
        tracking = _readtracking(repo)
        # print "tracking on %s %s" % (current, tracking)
        if current and current in tracking:
            track = tracking[current]
            path, book = splitremotename(track)
            # un-rename a path, if needed
            path = revrenames.get(path, path)
            if book and path in paths:
                dest = path
                opargs['to'] = book

    # un-rename passed path
    dest = revrenames.get(dest, dest)

    # if dest was renamed to default but we aren't specifically requesting
    # to push to default, change dest to default-push, if available
    if not origdest and dest == 'default' and 'default-push' in paths:
        dest = 'default-push'

    try:
        # hgsubversion and hggit do funcky things on push. Just call it
        # directly
        path = paths[dest]
        if path.startswith('svn+') or path.startswith('git+'):
            return orig(ui, repo, dest, opargs=opargs, **opts)
    except KeyError:
        pass

    if not opargs['to']:
        if ui.configbool('remotenames', 'forceto', False):
            msg = _('must specify --to when pushing')
            hint = _('see configuration option %s') % 'remotenames.forceto'
            raise error.Abort(msg, hint=hint)

        if not revs:
            opts['rev'] = _pushrevs(repo, ui, None)

        return orig(ui, repo, dest, opargs=opargs, **opts)

    if opts.get('bookmark'):
        msg = _('do not specify --to/-t and --bookmark/-B at the same time')
        raise error.Abort(msg)
    if opts.get('branch'):
        msg = _('do not specify --to/-t and --branch/-b at the same time')
        raise error.Abort(msg)

    if revs:
        revs = [repo.lookup(r) for r in scmutil.revrange(repo, revs)]
    else:
        revs = _pushrevs(repo, ui, '.')
    if len(revs) != 1:
        msg = _('--to requires exactly one rev to push')
        hint = _('use --rev BOOKMARK or omit --rev for current commit (.)')
        raise error.Abort(msg, hint=hint)
    rev = revs[0]

    # big can o' copypasta from commands.push
    dest = ui.expandpath(dest or 'default-push', dest or 'default')
    dest, branches = hg.parseurl(dest, opts.get('branch'))
    try:
        other = hg.peer(repo, opts, dest)
    except error.RepoError:
        if dest == "default-push":
            hint = _('see the "path" section in "hg help config"')
            raise error.Abort(_("default repository not configured!"),
                             hint=hint)
        else:
            raise

    # all checks pass, go for it!
    node = repo.lookup(rev)
    ui.status(_('pushing rev %s to destination %s bookmark %s\n') % (
              short(node), dest, opargs['to']))

    # TODO: subrepo stuff

    force = opts.get('force')
    bookmark = opargs['to']
    pattern = ui.config("remotenames", "disallowedto")
    if pattern and re.match(pattern, bookmark):
        msg = _("this remote bookmark name is not allowed")
        hint = ui.config("remotenames", "disallowedhint") or \
                       _("use another bookmark name")
        raise error.Abort(msg, hint=hint)
    # NB: despite the name, 'revs' doesn't work if it's a numeric rev
    pushop = exchange.push(repo, other, force, revs=[node],
                           bookmarks=(opargs['to'],), opargs=opargs)

    result = not pushop.cgresult
    if pushop.bkresult is not None:
        if pushop.bkresult == 2:
            result = 2
        elif not result and pushop.bkresult:
            result = 2

    return result

def exclonecmd(orig, ui, *args, **opts):
    if opts['mirror']:
        ui.setconfig('remotenames', 'syncbookmarks', True, 'mirror-clone')
    orig(ui, *args, **opts)

def exbranches(orig, ui, repo, *args, **opts):
    if not opts.get('remote'):
        orig(ui, repo, *args, **opts)

    if opts.get('all') or opts.get('remote'):
        # exit early if namespace doesn't even exist
        namespace = 'remotebranches'
        if namespace not in repo.names:
            return

        ns = repo.names[namespace]
        label = 'log.' + ns.colorname
        fm = ui.formatter('branches', opts)

        # it seems overkill to hide displaying hidden remote branches
        repo = repo.unfiltered()

        # create a sorted by descending rev list
        revs = set()
        for name in ns.listnames(repo):
            for n in ns.nodes(repo, name):
                revs.add(repo.changelog.rev(n))

        for r in sorted(revs, reverse=True):
            ctx = repo[r]
            for name in ns.names(repo, ctx.node()):
                fm.startitem()
                padsize = max(31 - len(str(r)) - encoding.colwidth(name), 0)

                tmplabel = label
                if ctx.obsolete():
                    tmplabel = tmplabel + ' changeset.obsolete'
                fm.write(ns.colorname, '%s', name, label=label)
                fmt = ' ' * padsize + ' %d:%s'
                fm.condwrite(not ui.quiet, 'rev node', fmt, r,
                             fm.hexfunc(ctx.node()), label=tmplabel)
                fm.plain('\n')
        fm.end()

def _readtracking(repo):
    tracking = {}
    try:
        vfs = shareawarevfs(repo)
        for line in vfs.read('bookmarks.tracking').strip().split('\n'):
            try:
                book, track = line.strip().split(' ', 1)
                tracking[book] = track
            except ValueError:
                # corrupt file, ignore entry
                pass
    except IOError:
        pass
    return tracking

def _writetracking(repo, tracking):
    data = ''
    for book, track in tracking.iteritems():
        data += '%s %s\n' % (book, track)
    vfs = shareawarevfs(repo)
    vfs.write('bookmarks.tracking', data)

def _removetracking(repo, bookmarks):
    tracking = _readtracking(repo)
    needwrite = False
    for bmark in bookmarks:
        try:
            del tracking[bmark]
            needwrite = True
        except KeyError:
            pass
    if needwrite:
        _writetracking(repo, tracking)

def exbookmarks(orig, ui, repo, *args, **opts):
    """Bookmark output is sorted by bookmark name.

    This has the side benefit of grouping all remote bookmarks by remote name.

    """
    delete = opts.get('delete')
    rename = opts.get('rename')
    inactive = opts.get('inactive')
    remote = opts.get('remote')
    track = opts.get('track')
    untrack = opts.get('untrack')

    disallowed = set(ui.configlist('remotenames', 'disallowedbookmarks'))
    # Adds local bookmark if one of the options is called and args is empty
    if not args and (track or untrack):
        book = repo._bookmarks.active
        if book:
            args = (book,)

    if not delete:
        for name in args:
            if name in disallowed:
                msg = _("bookmark '%s' not allowed by configuration")
                raise error.Abort(msg % name)

    if untrack:
        if track:
            msg = _('do not specify --untrack and --track at the same time')
            raise error.Abort(msg)
        _removetracking(repo, args)
        return

    if delete or rename or args or inactive:
        if delete and track:
            msg = _('do not specifiy --track and --delete at the same time')
            raise error.Abort(msg)

        ret = orig(ui, repo, *args, **opts)

        oldtracking = _readtracking(repo)
        tracking = dict(oldtracking)

        if rename and not track:
            if rename in tracking:
                tracked = tracking[rename]
                del tracking[rename]
                for arg in args:
                    tracking[arg] = tracked

        if track:
            for arg in args:
                tracking[arg] = track

        if delete:
            for arg in args:
                if arg in tracking:
                    del tracking[arg]

        if tracking != oldtracking:
            _writetracking(repo, tracking)
            # update the cache
            precachedistance(repo)

        return ret

    if not remote:
        displaylocalbookmarks(ui, repo, opts)

    if remote or opts.get('all'):
        displayremotebookmarks(ui, repo, opts)

def displaylocalbookmarks(ui, repo, opts):
    # copy pasta from commands.py; need to patch core
    fm = ui.formatter('bookmarks', opts)
    hexfn = fm.hexfunc
    marks = repo._bookmarks
    if len(marks) == 0 and not fm:
        ui.status(_("no bookmarks set\n"))

    tracking = _readtracking(repo)
    distances = readdistancecache(repo)
    nq = not ui.quiet

    for bmark, n in sorted(marks.iteritems()):
        current = bmactive(repo)
        if bmark == current:
            prefix, label = '*', 'bookmarks.current bookmarks.active'
        else:
            prefix, label = ' ', ''

        fm.startitem()
        if nq:
            fm.plain(' %s ' % prefix, label=label)
        fm.write('bookmark', '%s', bmark, label=label)
        pad = " " * (25 - encoding.colwidth(bmark))
        rev = repo.changelog.rev(n)
        h = hexfn(n)
        fm.condwrite(nq, 'rev node', pad + ' %d:%s', rev, h, label=label)
        if ui.verbose and bmark in tracking:
            tracked = tracking[bmark]
            if bmark in distances:
                distance = distances[bmark]
            else:
                distance = calculatenamedistance(repo, bmark, tracked)
            if tracked:
                fmt = '%s'
                args = (tracked,)
                fields = ['tracking']
                if distance != (0, 0) and distance != (None, None):
                    ahead, behind = distance
                    fmt += ': %s ahead, %s behind'
                    args += ahead, behind
                    fields += ['ahead', 'behind']
                pad = " " * (25 - encoding.colwidth(str(rev)) -
                             encoding.colwidth(str(h)))
                fm.write(' '.join(fields), '%s[%s]' % (pad, fmt), *args,
                    label=label)
                if distance != (None, None):
                    distances[bmark] = distance
        fm.data(active=(bmark == current))
        fm.plain('\n')
    fm.end()

    # write distance cache
    writedistancecache(repo, distances)

def displayremotebookmarks(ui, repo, opts):
    n = 'remotebookmarks'
    if n not in repo.names:
        return
    ns = repo.names[n]
    color = ns.colorname
    label = 'log.' + color

    fm = ui.formatter('bookmarks', opts)

    # it seems overkill to hide displaying hidden remote bookmarks
    repo = repo.unfiltered()

    for name in sorted(ns.listnames(repo)):
        node = ns.nodes(repo, name)[0]
        ctx = repo[node]
        fm.startitem()

        if not ui.quiet:
            fm.plain('   ')

        padsize = max(25 - encoding.colwidth(name), 0)
        fmt = ' ' * padsize + ' %d:%s'

        tmplabel = label
        if ctx.obsolete():
            tmplabel = tmplabel + ' changeset.obsolete'
        fm.write(color, '%s', name, label=label)
        fm.condwrite(not ui.quiet, 'rev node', fmt, ctx.rev(),
                     fm.hexfunc(node), label=tmplabel)
        fm.plain('\n')
    fm.end()

def activepath(ui, remote):
    local = None
    try:
        local = remote.local()
    except AttributeError:
        pass

    # determine the remote path from the repo, if possible; else just
    # use the string given to us
    rpath = remote
    if local:
        rpath = getattr(remote, 'root', None)
        if rpath is None:
            # Maybe a localpeer? (hg@1ac628cd7113, 2.3)
            rpath = getattr(getattr(remote, '_repo', None),
                            'root', None)
    elif not isinstance(remote, str):
        try:
            rpath = remote._url
        except AttributeError:
            rpath = remote.url

    candidates = []
    for path, uri in ui.configitems('paths'):
        uri = ui.expandpath(expandscheme(ui, uri))
        if local:
            uri = os.path.realpath(uri)
        else:
            if uri.startswith('http'):
                try:
                    uri = util.url(uri).authinfo()[0]
                except AttributeError:
                    uri = url.getauthinfo(uri)[0]
        uri = uri.rstrip('/')
        # guard against hgsubversion nonsense
        if not isinstance(rpath, basestring):
            continue
        rpath = rpath.rstrip('/')
        if uri == rpath:
            candidates.append(path)

    if not candidates:
        return ''

    # be stable under different orderings of paths in config files
    # prefer any name other than 'default' and 'default-push' if available
    # prefer shortest name of remaining names, and break ties by alphabetizing
    cset = set(candidates)
    cset.discard('default')
    cset.discard('default-push')
    if cset:
        candidates = list(cset)

    candidates.sort()         # alphabetical
    candidates.sort(key=len)  # sort is stable so first will be the correct one
    bestpath = candidates[0]

    renames = _getrenames(ui)
    realpath = renames.get(bestpath, bestpath)
    return realpath

# memoization
_renames = None
def _getrenames(ui):
    global _renames
    if _renames is None:
        _renames = {}
        for k, v in ui.configitems('remotenames'):
            if k.startswith('rename.'):
                _renames[k[7:]] = v
    return _renames

def expandscheme(ui, uri):
    '''For a given uri, expand the scheme for it'''
    urischemes = [s for s in schemes.schemes.iterkeys()
                  if uri.startswith('%s://' % s)]
    for s in urischemes:
        # TODO: refactor schemes so we don't
        # duplicate this logic
        ui.note(_('performing schemes expansion with '
                  'scheme %s\n') % s)
        scheme = hg.schemes[s]
        parts = uri.split('://', 1)[1].split('/', scheme.parts)
        if len(parts) > scheme.parts:
            tail = parts[-1]
            parts = parts[:-1]
        else:
            tail = ''
        ctx = dict((str(i + 1), v) for i, v in enumerate(parts))
        uri = ''.join(scheme.templater.process(scheme.url, ctx)) + tail
    return uri

def splitremotename(remote):
    name = ''
    if '/' in remote:
        remote, name = remote.split('/', 1)
    return remote, name

def joinremotename(remote, ref):
    if ref:
        remote += '/' + ref
    return remote

def shareawarevfs(repo):
    if repo.shared():
        return scmutil.vfs(repo.sharedpath)
    else:
        return repo.vfs

def readremotenames(repo):
    vfs = shareawarevfs(repo)
    # exit early if there is nothing to do
    if not vfs.exists('remotenames'):
        return

    # needed to heuristically determine if a file is in the old format
    branches = repo.names['branches'].listnames(repo)
    bookmarks = repo.names['bookmarks'].listnames(repo)

    f = vfs('remotenames')
    for line in f:
        nametype = None
        line = line.strip()
        if not line:
            continue
        nametype = None
        remote, rname = None, None

        node, name = line.split(' ', 1)

        # check for nametype being written into the file format
        if ' ' in name:
            nametype, name = name.split(' ', 1)

        remote, rname = splitremotename(name)

        # skip old data that didn't write the name (only wrote the alias)
        if not rname:
            continue

        # old format didn't save the nametype, so check for the name in
        # branches and bookmarks
        if nametype is None:
            if rname in branches:
                nametype = 'branches'
            elif rname in bookmarks:
                nametype = 'bookmarks'

        yield node, nametype, remote, rname

    f.close()

def transition(repo, ui):
    """
    Help with transitioning to using a remotenames workflow.

    Allows deleting matching local bookmarks defined in a config file:

    [remotenames]
    transitionbookmarks = master
        stable
    """
    transmarks = ui.configlist('remotenames', 'transitionbookmarks')
    localmarks = repo._bookmarks
    for mark in transmarks:
        if mark in localmarks:
            del localmarks[mark]
    lock = tr = None
    try:
        lock = repo.lock()
        tr = repo.transaction("remotenames")
        localmarks.recordchange(tr)
        tr.close()
    finally:
        lockmod.release(lock, tr)

    message = ui.config('remotenames', 'transitionmessage')
    if message:
        ui.warn(message + '\n')

def saveremotenames(repo, remotepath, branches=None, bookmarks=None):
    vfs = shareawarevfs(repo)
    wlock = repo.wlock()
    if branches is None:
        branches = {}
    if bookmarks is None:
        bookmarks = {}
    try:
        # delete old files
        try:
            vfs.unlink('remotedistance')
        except OSError as inst:
            if inst.errno != errno.ENOENT:
                raise

        if not vfs.exists('remotenames'):
            transition(repo, repo.ui)

        # read in all data first before opening file to write
        olddata = set(readremotenames(repo))
        oldbooks = {}

        f = vfs('remotenames', 'w')

        # only update the given 'remote path'; iterate over
        # old data and re-save it
        for node, nametype, oldremote, rname in olddata:
            if oldremote != remotepath:
                n = joinremotename(oldremote, rname)
                f.write('%s %s %s\n' % (node, nametype, n))
            elif nametype == 'bookmarks':
                oldbooks[rname] = node

        for branch, nodes in branches.iteritems():
            for n in nodes:
                rname = joinremotename(remotepath, branch)
                f.write('%s branches %s\n' % (hex(n), rname))

        nm = repo.unfiltered().changelog.nodemap
        for bookmark, n in bookmarks.iteritems():
            bookhex = n
            if not bin(bookhex) in nm:
                # node is unknown locally, don't change the bookmark
                bookhex = oldbooks.get(bookmark)
            if bookhex:
                f.write('%s bookmarks %s\n' %
                    (bookhex, joinremotename(remotepath, bookmark)))
        f.close()

        # Old paths have been deleted, refresh remotenames
        repo._remotenames.clearnames()

    finally:
        wlock.release()

def calculatedistance(repo, fromrev, torev):
    """
    Return the (ahead, behind) distance between `fromrev` and `torev`.
    The returned tuple will contain ints if calculated, Nones otherwise.
    """
    if not repo.ui.configbool('remotenames', 'calculatedistance', True):
        return (None, None)

    ahead = len(repo.revs('only(%d, %d)' % (fromrev, torev)))
    behind = len(repo.revs('only(%d, %d)' % (torev, fromrev)))

    return (ahead, behind)

def calculatenamedistance(repo, fromname, toname):
    """
    Similar to calculatedistance, but accepts names such as local and remote
    bookmarks, and will return (None, None) if any of the names do not resolve
    in the given repository.
    """
    distance = (None, None)
    if fromname and fromname in repo and toname in repo:
        rev1 = repo[fromname].rev()
        rev2 = repo[toname].rev()
        distance = calculatedistance(repo, rev1, rev2)
    return distance

def writedistancecache(repo, distance):
    try:
        vfs = shareawarevfs(repo)
        f = vfs('cache/distance', 'w')
        for k, v in distance.iteritems():
            f.write('%s %d %d\n' % (k, v[0], v[1]))
    except (IOError, OSError):
        pass

def readdistancecache(repo):
    distances = {}
    try:
        vfs = shareawarevfs(repo)
        for line in vfs.read('cache/distance').splitlines():
            line = line.rsplit(' ', 2)
            try:
                d = (int(line[1]), int(line[2]))
                distances[line[0]] = d
            except ValueError:
                # corrupt entry, ignore line
                pass
    except (IOError, OSError):
        pass

    return distances

def invalidatedistancecache(repo):
    """Try to invalidate any existing distance caches"""
    error = False
    vfs = shareawarevfs(repo)
    try:
        if vfs.isdir('cache/distance'):
            shutil.rmtree(vfs.join('cache/distance'))
        else:
            vfs.unlink('cache/distance')
    except (OSError, IOError) as inst:
        if inst.errno != errno.ENOENT:
            error = True
    try:
        vfs.unlink('cache/distance.current')
    except (OSError, IOError) as inst:
        if inst.errno != errno.ENOENT:
            error = True

    if error:
        repo.ui.warn(_('Unable to invalidate tracking cache; ' +
                       'distance displayed may be incorrect\n'))

def precachedistance(repo):
    """
    Caclulate and cache the distance between bookmarks and what they
    track, plus the distance from the tipmost head on current topological
    branch. This can be an expensive operation especially in repositories
    with a high commit rate, so it can be turned off in your hgrc:

        [remotenames]
        precachedistance = False
        precachecurrent = False
    """
    # to avoid stale namespaces, let's reload
    repo._remotenames.clearnames()

    wlock = repo.wlock()
    try:
        invalidatedistancecache(repo)

        distances = {}
        if repo.ui.configbool('remotenames', 'precachedistance', True):
            distances = {}
            for bmark, tracked in _readtracking(repo).iteritems():
                distance = calculatenamedistance(repo, bmark, tracked)
                if distance != (None, None):
                    distances[bmark] = distance
            writedistancecache(repo, distances)

        if repo.ui.configbool('remotenames', 'precachecurrent', True):
            # are we on a 'branch' but not at the head?
            # i.e. is there a bookmark that we are heading towards?
            revs = list(repo.revs('limit(.:: and bookmark() - ., 1)'))
            if revs:
                # if we are here then we have one or more bookmarks
                # and we'll pick the first one for now
                bmark = repo[revs[0]].bookmarks()[0]
                distance = len(repo.revs('only(%d, .)' % revs[0]))
                vfs = shareawarevfs(repo)
                vfs.write('cache/distance.current',
                          '%s %d' % (bmark, distance))

    finally:
        wlock.release()

#########
# revsets
#########

def upstream_revs(filt, repo, subset, x):
    upstream_tips = set()
    for remotename in repo._remotenames.keys():
        rname = 'remote' + remotename
        try:
            ns = repo.names[rname]
        except KeyError:
            continue
        for name in ns.listnames(repo):
            if filt(splitremotename(name)[0]):
                upstream_tips.update(ns.nodes(repo, name))

    if not upstream_tips:
        return revset.baseset([])

    tipancestors = repo.revs('::%ln', upstream_tips)
    return revset.filteredset(subset, lambda n: n in tipancestors)

def upstream(repo, subset, x):
    '''``upstream()``
    Select changesets in an upstream repository according to remotenames.
    '''
    repo = repo.unfiltered()
    upstream_names = repo.ui.configlist('remotenames', 'upstream')
    # override default args from hgrc with args passed in on the command line
    if x:
        upstream_names = [revset.getstring(symbol,
                                           "remote path must be a string")
                          for symbol in revset.getlist(x)]

    default_path = dict(repo.ui.configitems('paths')).get('default')
    if not upstream_names and default_path:
        default_path = expandscheme(repo.ui, default_path)
        upstream_names = [activepath(repo.ui, default_path)]

    def filt(name):
        if upstream_names:
            return name in upstream_names
        return True

    return upstream_revs(filt, repo, subset, x)

def pushed(repo, subset, x):
    '''``pushed()``
    Select changesets in any remote repository according to remotenames.
    '''
    revset.getargs(x, 0, 0, "pushed takes no arguments")
    return upstream_revs(lambda x: True, repo, subset, x)

def remotenamesrevset(repo, subset, x):
    """``remotenames()``
    All remote branches heads.
    """
    revset.getargs(x, 0, 0, "remotenames takes no arguments")
    remoterevs = set()
    cl = repo.changelog
    for remotename in repo._remotenames.keys():
        rname = 'remote' + remotename
        try:
            ns = repo.names[rname]
        except KeyError:
            continue
        for name in ns.listnames(repo):
            remoterevs.update(ns.nodes(repo, name))

    results = (cl.rev(n) for n in remoterevs if n in repo)
    return subset & revset.baseset(sorted(results))

revset.symbols.update({'upstream': upstream,
                       'pushed': pushed,
                       'remotenames': remotenamesrevset})

###########
# templates
###########

def remotenameskw(**args):
    """:remotenames: List of strings. List of remote names associated with the
    changeset. If remotenames.suppressbranches is True then branch names will
    be hidden if there is a bookmark at the same changeset.

    """
    repo, ctx = args['repo'], args['ctx']

    remotenames = []
    if 'remotebookmarks' in repo.names:
        remotenames = repo.names['remotebookmarks'].names(repo, ctx.node())

    suppress = repo.ui.configbool('remotenames', 'suppressbranches', False)
    if (not remotenames or not suppress) and 'remotebranches' in repo.names:
        remotenames += repo.names['remotebranches'].names(repo, ctx.node())

    return templatekw.showlist('remotename', remotenames,
                               plural='remotenames', **args)

#############################
# bookmarks api compatibility
#############################
def bmactive(repo):
    try:
        return repo._activebookmark
    except AttributeError:
        return repo._bookmarkcurrent
