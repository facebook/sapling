# Infinite push
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
    [infinitepush]
    # Server-side and client-side option. Pattern of the infinitepush bookmark
    branchpattern = PATTERN

    # Server or client
    server = False

    # Server-side option. Possible values: 'disk' or 'sql'. Fails if not set
    indextype = disk

    # Server-side option. Used only if indextype=sql.
    # Format: 'IP:PORT:DB_NAME:USER:PASSWORD'
    sqlhost = IP:PORT:DB_NAME:USER:PASSWORD

    # Server-side option. Used only if indextype=disk.
    # Filesystem path to the index store
    indexpath = PATH

    # Server-side option. Possible values: 'disk' or 'external'
    # Fails if not set
    storetype = disk

    # Server-side option.
    # Path to the binary that will save bundle to the bundlestore
    # Formatted cmd line will be passed to it (see `put_args`)
    put_binary = put

    # Serser-side option. Used only if storetype=external.
    # Format cmd-line string for put binary. Placeholder: {filename}
    put_args = {filename}

    # Server-side option.
    # Path to the binary that get bundle from the bundlestore.
    # Formatted cmd line will be passed to it (see `get_args`)
    get_binary = get

    # Serser-side option. Used only if storetype=external.
    # Format cmd-line string for get binary. Placeholders: {filename} {handle}
    get_args = {filename} {handle}

    # Server-side option
    logfile = FIlE

    # Server-side option
    loglevel = DEBUG

    # Client-side option
    debugbackuplog = FILE
"""

from __future__ import absolute_import
import errno
import hashlib
import json
import logging
import os
import resource
import socket
import struct
import tempfile

from collections import defaultdict
from hgext3rd.extutil import runshellcommand
from mercurial import (
    bundle2,
    changegroup,
    cmdutil,
    commands,
    discovery,
    encoding,
    error,
    exchange,
    extensions,
    hg,
    localrepo,
    phases,
    pushkey,
    revset,
    util,
    wireproto,
)

from mercurial.extensions import wrapcommand, wrapfunction, unwrapfunction
from mercurial.hg import repository
from mercurial.node import bin, hex
from mercurial.i18n import _
from mercurial.peer import batchable, future
from mercurial.wireproto import encodelist, decodelist


cmdtable = {}
command = cmdutil.command(cmdtable)

pushrebaseparttype = 'b2x:rebase'
scratchbranchparttype = 'b2x:infinitepush'
scratchbookmarksparttype = 'b2x:infinitepushscratchbookmarks'

experimental = 'experimental'
configmanybookmarks = 'server-bundlestore-manybookmarks'
configbookmark = 'server-bundlestore-bookmark'
configcreate = 'server-bundlestore-create'
configscratchpush = 'infinitepush-scratchpush'

_scratchbranchmatcher = lambda x: False

def _buildexternalbundlestore(ui):
    put_args = ui.configlist('infinitepush', 'put_args', [])
    put_binary = ui.config('infinitepush', 'put_binary')
    if not put_binary:
        raise error.Abort('put binary is not specified')
    get_args = ui.configlist('infinitepush', 'get_args', [])
    get_binary = ui.config('infinitepush', 'get_binary')
    if not get_binary:
        raise error.Abort('get binary is not specified')
    from . import store
    return store.externalbundlestore(put_binary, put_args, get_binary, get_args)

def _buildsqlindex(ui):
    sqlhost = ui.config('infinitepush', 'sqlhost')
    if not sqlhost:
        raise error.Abort(_('please set infinitepush.sqlhost'))
    host, port, db, user, password = sqlhost.split(':')
    reponame = ui.config('infinitepush', 'reponame')
    if not reponame:
        raise error.Abort(_('please set infinitepush.reponame'))

    logfile = ui.config('infinitepush', 'logfile', '')
    waittimeout = ui.configint('infinitepush', 'waittimeout', 300)
    from . import sqlindexapi
    return sqlindexapi.sqlindexapi(
        reponame, host, port, db, user, password,
        logfile, _getloglevel(ui), waittimeout=waittimeout)

def _getloglevel(ui):
    loglevel = ui.config('infinitepush', 'loglevel', 'DEBUG')
    numeric_loglevel = getattr(logging, loglevel.upper(), None)
    if not isinstance(numeric_loglevel, int):
        raise error.Abort(_('invalid log level %s') % loglevel)
    return numeric_loglevel

class bundlestore(object):
    def __init__(self, repo):
        self._repo = repo
        storetype = self._repo.ui.config('infinitepush', 'storetype', '')
        if storetype == 'disk':
            from . import store
            self.store = store.filebundlestore(self._repo.ui, self._repo)
        elif storetype == 'external':
            self.store = _buildexternalbundlestore(self._repo.ui)
        else:
            raise error.Abort(
                _('unknown infinitepush store type specified %s') % storetype)

        indextype = self._repo.ui.config('infinitepush', 'indextype', '')
        if indextype == 'disk':
            from . import fileindexapi
            self.index = fileindexapi.fileindexapi(self._repo)
        elif indextype == 'sql':
            self.index = _buildsqlindex(self._repo.ui)
        else:
            raise error.Abort(
                _('unknown infinitepush index type specified %s') % indextype)

def _isserver(ui):
    return ui.configbool('infinitepush', 'server')

def reposetup(ui, repo):
    if _isserver(ui) and repo.local():
        repo.bundlestore = bundlestore(repo)

def uisetup(ui):
    # remotenames circumvents the default push implementation entirely, so make
    # sure we load after it so that we wrap it.
    order = extensions._order
    order.remove('infinitepush')
    order.append('infinitepush')
    extensions._order = order

def extsetup(ui):
    commonsetup(ui)
    if _isserver(ui):
        serverextsetup(ui)
    else:
        clientextsetup(ui)

def commonsetup(ui):
    wireproto.commands['listkeyspatterns'] = (
        wireprotolistkeyspatterns, 'namespace patterns')
    scratchbranchpat = ui.config('infinitepush', 'branchpattern')
    if scratchbranchpat:
        global _scratchbranchmatcher
        kind, pat, _scratchbranchmatcher = util.stringmatcher(scratchbranchpat)

def serverextsetup(ui):
    origpushkeyhandler = bundle2.parthandlermapping['pushkey']

    def newpushkeyhandler(*args, **kwargs):
        bundle2pushkey(origpushkeyhandler, *args, **kwargs)
    newpushkeyhandler.params = origpushkeyhandler.params
    bundle2.parthandlermapping['pushkey'] = newpushkeyhandler

    wrapfunction(localrepo.localrepository, 'listkeys', localrepolistkeys)
    wireproto.commands['lookup'] = (
        _lookupwrap(wireproto.commands['lookup'][0]), 'key')
    wrapfunction(exchange, 'getbundlechunks', getbundlechunks)

def clientextsetup(ui):
    entry = wrapcommand(commands.table, 'push', _push)
    # Don't add the 'to' arg if it already exists
    if not any(a for a in entry[1] if a[1] == 'to'):
        entry[1].append(('', 'to', '', _('push revs to this bookmark')))

    if not any(a for a in entry[1] if a[1] == 'create'):
        entry[1].append(
            ('', 'create', None, _('create a new remote bookmark')))

    entry[1].append(
        ('', 'bundle-store', None,
         _('force push to go to bundle store (EXPERIMENTAL)')))

    bookcmd = extensions.wrapcommand(commands.table, 'bookmarks', exbookmarks)
    bookcmd[1].append(
        ('', 'list-remote', '',
         'list remote bookmarks. '
         'Use \'*\' to find all bookmarks with the same prefix',
         'PATTERN'))
    bookcmd[1].append(
        ('', 'remote-path', '',
         'name of the remote path to list the bookmarks'))

    wrapcommand(commands.table, 'pull', _pull)

    wrapfunction(discovery, 'checkheads', _checkheads)

    wireproto.wirepeer.listkeyspatterns = listkeyspatterns

    # Move infinitepush part before pushrebase part
    # to avoid generation of both parts.
    partorder = exchange.b2partsgenorder
    index = partorder.index('changeset')
    if pushrebaseparttype in partorder:
        index = min(index, partorder.index(pushrebaseparttype))
    partorder.insert(
        index, partorder.pop(partorder.index(scratchbranchparttype)))

def _showbookmarks(ui, bookmarks, **opts):
    # Copy-paste from commands.py
    fm = ui.formatter('bookmarks', opts)
    for bmark, n in sorted(bookmarks.iteritems()):
        fm.startitem()
        if not ui.quiet:
            fm.plain('   ')
        fm.write('bookmark', '%s', bmark)
        pad = ' ' * (25 - encoding.colwidth(bmark))
        fm.condwrite(not ui.quiet, 'node', pad + ' %s', n)
        fm.plain('\n')
    fm.end()

def exbookmarks(orig, ui, repo, *names, **opts):
    pattern = opts.get('list_remote')
    if pattern:
        remotepath = opts.get('remote_path')
        path = ui.paths.getpath(remotepath or None, default=('default'))
        destpath = path.pushloc or path.loc
        other = hg.peer(repo, opts, destpath)
        fetchedbookmarks = other.listkeyspatterns('bookmarks',
                                                  patterns=[pattern])
        _showbookmarks(ui, fetchedbookmarks, **opts)
        return
    return orig(ui, repo, *names, **opts)

def _checkheads(orig, pushop):
    if pushop.ui.configbool(experimental, configscratchpush, False):
        return
    return orig(pushop)

def wireprotolistkeyspatterns(repo, proto, namespace, patterns):
    patterns = decodelist(patterns)
    d = repo.listkeys(encoding.tolocal(namespace), patterns).iteritems()
    return pushkey.encodekeys(d)

def localrepolistkeys(orig, self, namespace, patterns=None):
    if namespace == 'bookmarks' and patterns:
        index = self.bundlestore.index
        results = {}
        bookmarks = orig(self, namespace)
        for pattern in patterns:
            results.update(index.getbookmarks(pattern))
            if pattern.endswith('*'):
                pattern = 're:^' + pattern[:-1] + '.*'
            kind, pat, matcher = util.stringmatcher(pattern)
            for bookmark, node in bookmarks.iteritems():
                if matcher(bookmark):
                    results[bookmark] = node
        return results
    else:
        return orig(self, namespace)

@batchable
def listkeyspatterns(self, namespace, patterns):
    if not self.capable('pushkey'):
        yield {}, None
    f = future()
    self.ui.debug('preparing listkeys for "%s" with pattern "%s"\n' %
                  (namespace, patterns))
    yield {
        'namespace': encoding.fromlocal(namespace),
        'patterns': encodelist(patterns)
    }, f
    d = f.value
    self.ui.debug('received listkey for "%s": %i bytes\n'
                  % (namespace, len(d)))
    yield pushkey.decodekeys(d)

def _readbundlerevs(bundlerepo):
    return list(bundlerepo.revs('bundle()'))

def _includefilelogstobundle(bundlecaps, bundlerepo, bundlerevs, ui):
    '''Tells remotefilelog to include all changed files to the changegroup

    By default remotefilelog doesn't include file content to the changegroup.
    But we need to include it if we are fetching from bundlestore.
    '''
    changedfiles = set()
    cl = bundlerepo.changelog
    for r in bundlerevs:
        # [3] means changed files
        changedfiles.update(cl.read(r)[3])
    if not changedfiles:
        return bundlecaps

    changedfiles = '\0'.join(changedfiles)
    newcaps = []
    appended = False
    for cap in (bundlecaps or []):
        if cap.startswith('excludepattern='):
            newcaps.append('\0'.join((cap, changedfiles)))
            appended = True
        else:
            newcaps.append(cap)
    if not appended:
        # Not found excludepattern cap. Just append it
        newcaps.append('excludepattern=' + changedfiles)

    return newcaps

def getbundlerepo(repo, unknownhead):
    index = repo.bundlestore.index
    store = repo.bundlestore.store
    bundleid = index.getbundle(hex(unknownhead))
    if bundleid is None:
        raise error.Abort('%s head is not known' % hex(unknownhead))
    bundleraw = store.read(bundleid)
    bundlefile = _makebundlefromraw(bundleraw)
    bundlepath = "bundle:%s+%s" % (repo.root, bundlefile)
    return repository(repo.ui, bundlepath)

def _getoutputbundleraw(bundlerepo, bundleroots, unknownhead):
    '''
    Bundle may include more revision then user requested. For example,
    if user asks for revision but bundle also consists its descendants.
    This function will filter out all revision that user is not requested.
    '''
    outgoing = discovery.outgoing(bundlerepo, commonheads=bundleroots,
                                  missingheads=[unknownhead])
    outputbundleraw = changegroup.getlocalchangegroupraw(bundlerepo, 'pull',
                                                         outgoing)
    return outputbundleraw

def _getbundleroots(oldrepo, bundlerepo, bundlerevs):
    cl = bundlerepo.changelog
    bundleroots = []
    for rev in bundlerevs:
        node = cl.node(rev)
        parents = cl.parents(node)
        for parent in parents:
            # include all revs that exist in the main repo
            # to make sure that bundle may apply client-side
            if parent in oldrepo:
                bundleroots.append(parent)
    return bundleroots

def getbundlechunks(orig, repo, source, heads=None, bundlecaps=None, **kwargs):
    heads = heads or []
    # newheads are parents of roots of scratch bundles that were requested
    newphases = {}
    scratchbundles = []
    newheads = []
    scratchheads = []
    for head in heads:
        if head not in repo.changelog.nodemap:
            bundlerepo = getbundlerepo(repo, head)
            bundlerevs = set(_readbundlerevs(bundlerepo))
            bundlecaps = _includefilelogstobundle(bundlecaps, bundlerepo,
                                                  bundlerevs, repo.ui)
            cl = bundlerepo.changelog
            for rev in bundlerevs:
                node = cl.node(rev)
                newphases[hex(node)] = str(phases.draft)

            bundleroots = _getbundleroots(repo, bundlerepo, bundlerevs)
            outputbundleraw = _getoutputbundleraw(bundlerepo, bundleroots, head)
            scratchbundles.append(outputbundleraw)
            newheads.extend(bundleroots)
            scratchheads.append(head)

    pullfrombundlestore = bool(scratchbundles)
    wrappedchangegrouppart = False
    wrappedlistkeys = False
    oldchangegrouppart = exchange.getbundle2partsmapping['changegroup']
    try:
        def _changegrouppart(bundler, *args, **kwargs):
            # Order is important here. First add non-scratch part
            # and only then add parts with scratch bundles because
            # non-scratch part contains parents of roots of scratch bundles.
            result = oldchangegrouppart(bundler, *args, **kwargs)
            for bundle in scratchbundles:
                bundler.newpart('changegroup', data=bundle)
            return result

        exchange.getbundle2partsmapping['changegroup'] = _changegrouppart
        wrappedchangegrouppart = True

        def _listkeys(orig, self, namespace):
            origvalues = orig(self, namespace)
            if namespace == 'phases' and pullfrombundlestore:
                if origvalues.get('publishing') == 'True':
                    # Make repo non-publishing to preserve draft phase
                    del origvalues['publishing']
                origvalues.update(newphases)
            return origvalues

        wrapfunction(localrepo.localrepository, 'listkeys', _listkeys)
        wrappedlistkeys = True
        heads = list((set(newheads) | set(heads)) - set(scratchheads))
        result = orig(repo, source, heads=heads,
                      bundlecaps=bundlecaps, **kwargs)
    finally:
        if wrappedchangegrouppart:
            exchange.getbundle2partsmapping['changegroup'] = oldchangegrouppart
        if wrappedlistkeys:
            unwrapfunction(localrepo.localrepository, 'listkeys', _listkeys)
    return result

def _lookupwrap(orig):
    def _lookup(repo, proto, key):
        localkey = encoding.tolocal(key)

        if isinstance(localkey, str) and _scratchbranchmatcher(localkey):
            scratchnode = repo.bundlestore.index.getnode(localkey)
            if scratchnode:
                return "%s %s\n" % (1, scratchnode)
            else:
                return "%s %s\n" % (0, 'scratch branch %s not found' % localkey)
        else:
            try:
                c = repo[localkey]
                r = c.hex()
                return "%s %s\n" % (1, r)
            except Exception as inst:
                if repo.bundlestore.index.getbundle(localkey):
                    return "%s %s\n" % (1, localkey)
                else:
                    r = str(inst)
                    return "%s %s\n" % (0, r)
    return _lookup

def validaterevset(repo, revset, bookmark):
    """Abort if the revs to be pushed aren't valid for a scratch branch."""
    if not repo.revs(revset):
        raise error.Abort(_('nothing to push'))
    if bookmark:
        # Allow bundle with many heads only if no bookmark is specified
        heads = repo.revs('heads(%r)', revset)
        if len(heads) > 1:
            raise error.Abort(
                _('cannot push more than one head to a scratch branch'))

def getscratchbranchpart(repo, peer, outgoing, force, ui, bookmark, create):
    if not outgoing.missing:
        raise error.Abort(_('no commits to push'))

    if scratchbranchparttype not in bundle2.bundle2caps(peer):
        raise error.Abort(_('no server support for %r') % scratchbranchparttype)

    validaterevset(repo, revset.formatspec('%ln', outgoing.missing), bookmark)

    cg = changegroup.getlocalchangegroupraw(repo, 'push', outgoing)

    params = {}
    if bookmark:
        params['bookmark'] = bookmark
        # 'prevbooknode' is necessary for pushkey reply part
        params['bookprevnode'] = ''
        if bookmark in repo:
            params['bookprevnode'] = repo[bookmark].hex()
        if create:
            params['create'] = '1'
    if force:
        params['force'] = '1'

    # Do not send pushback bundle2 part with bookmarks if remotenames extension
    # is enabled. It will be handled manually in `_push()`
    if not _isremotebooksenabled(ui):
        params['pushbackbookmarks'] = '1'

    # .upper() marks this as a mandatory part: server will abort if there's no
    #  handler
    return bundle2.bundlepart(
        scratchbranchparttype.upper(),
        advisoryparams=params.iteritems(),
        data=cg)

def _encodebookmarks(bookmarks):
    encoded = {}
    for bookmark, node in bookmarks.iteritems():
        encoded[bookmark] = node
    dumped = json.dumps(encoded)
    result = struct.pack('>i', len(dumped)) + dumped
    return result

def _decodebookmarks(stream):
    sizeofjsonsize = struct.calcsize('>i')
    size = struct.unpack('>i', stream.read(sizeofjsonsize))[0]
    unicodedict = json.loads(stream.read(size))
    # python json module always returns unicode strings. We need to convert
    # it back to bytes string
    result = {}
    for bookmark, node in unicodedict.iteritems():
        bookmark = bookmark.encode('ascii')
        node = node.encode('ascii')
        result[bookmark] = node
    return result

def getscratchbookmarkspart(peer, bookmarks):
    if scratchbookmarksparttype not in bundle2.bundle2caps(peer):
        raise error.Abort(
            _('no server support for %r') % scratchbookmarksparttype)

    return bundle2.bundlepart(
        scratchbookmarksparttype.upper(),
        data=_encodebookmarks(bookmarks))

def _pull(orig, ui, repo, source="default", **opts):
    # Copy paste from `pull` command
    source, branches = hg.parseurl(ui.expandpath(source), opts.get('branch'))

    hasscratchbookmarks = False
    scratchbookmarks = {}
    if opts.get('bookmark'):
        bookmarks = []
        revs = opts.get('rev') or []
        for bookmark in opts.get('bookmark'):
            if _scratchbranchmatcher(bookmark):
                other = hg.peer(repo, opts, source)
                fetchedbookmarks = other.listkeyspatterns('bookmarks',
                                                          patterns=[bookmark])
                if bookmark not in fetchedbookmarks:
                    raise error.Abort('remote bookmark %s not found!' %
                                      bookmark)
                scratchbookmarks[bookmark] = fetchedbookmarks[bookmark]
                revs.append(fetchedbookmarks[bookmark])
                hasscratchbookmarks = True
            else:
                bookmarks.append(bookmark)
        opts['bookmark'] = bookmarks
        opts['rev'] = revs

    try:
        inhibitmod = extensions.find('inhibit')
    except KeyError:
        # Ignore if inhibit is not enabled
        pass
    else:
        # Pulling revisions that were filtered results in a error.
        # Let's inhibit them
        unfi = repo.unfiltered()
        for rev in opts.get('rev', []):
            try:
                repo[rev]
            except error.FilteredRepoLookupError:
                node = unfi[rev].node()
                inhibitmod._inhibitmarkers(repo.unfiltered(), [node])
            except error.RepoLookupError:
                pass

    if hasscratchbookmarks:
        # Set anyincoming to True
        oldfindcommonincoming = wrapfunction(discovery,
                                             'findcommonincoming',
                                             _findcommonincoming)
    try:
        # Remote scratch bookmarks will be deleted because remotenames doesn't
        # know about them. Let's save it before pull and restore after
        remotescratchbookmarks = _readscratchremotebookmarks(ui, repo, source)
        result = orig(ui, repo, source, **opts)
        # TODO(stash): race condition is possible
        # if scratch bookmarks was updated right after orig.
        # But that's unlikely and shouldn't be harmful.
        if _isremotebooksenabled(ui):
            remotescratchbookmarks.update(scratchbookmarks)
            _saveremotebookmarks(repo, remotescratchbookmarks, source)
        else:
            _savelocalbookmarks(repo, scratchbookmarks)
        return result
    finally:
        if hasscratchbookmarks:
            discovery.findcommonincoming = oldfindcommonincoming

def _isremotebooksenabled(ui):
    return ('remotenames' in extensions._extensions and
            ui.configbool('remotenames', 'bookmarks', True))

def _readscratchremotebookmarks(ui, repo, other):
    if _isremotebooksenabled(ui):
        remotenamesext = extensions.find('remotenames')
        remotepath = remotenamesext.activepath(repo.ui, other)
        result = {}
        # Let's refresh remotenames to make sure we have it up to date
        # Seems that `repo.names['remotebookmarks']` may return stale bookmarks
        # and it results in deleting scratch bookmarks. Our best guess how to
        # fix it is to use `clearnames()`
        repo._remotenames.clearnames()
        for remotebookmark in repo.names['remotebookmarks'].listnames(repo):
            path, bookname = remotenamesext.splitremotename(remotebookmark)
            if path == remotepath and _scratchbranchmatcher(bookname):
                nodes = repo.names['remotebookmarks'].nodes(repo,
                                                            remotebookmark)
                result[bookname] = hex(nodes[0])
        return result
    else:
        return {}

def _saveremotebookmarks(repo, newbookmarks, remote):
    remotenamesext = extensions.find('remotenames')
    remotepath = remotenamesext.activepath(repo.ui, remote)
    branches = defaultdict(list)
    bookmarks = {}
    remotenames = remotenamesext.readremotenames(repo)
    for hexnode, nametype, remote, rname in remotenames:
        if remote != remotepath:
            continue
        if nametype == 'bookmarks':
            if rname in newbookmarks:
                # It's possible if we have a normal bookmark that matches
                # scratch branch pattern. In this case just use the current
                # bookmark node
                del newbookmarks[rname]
            bookmarks[rname] = hexnode
        elif nametype == 'branches':
            # saveremotenames expects 20 byte binary nodes for branches
            branches[rname].append(bin(hexnode))

    for bookmark, hexnode in newbookmarks.iteritems():
        bookmarks[bookmark] = hexnode
    remotenamesext.saveremotenames(repo, remotepath, branches, bookmarks)

def _savelocalbookmarks(repo, bookmarks):
    if not bookmarks:
        return
    with repo.wlock():
        with repo.lock():
            with repo.transaction('bookmark') as tr:
                for scratchbook, node in bookmarks.iteritems():
                    changectx = repo[node]
                    repo._bookmarks[scratchbook] = changectx.node()
                repo._bookmarks.recordchange(tr)

def _findcommonincoming(orig, *args, **kwargs):
    common, inc, remoteheads = orig(*args, **kwargs)
    return common, True, remoteheads

def _push(orig, ui, repo, dest=None, *args, **opts):
    oldbookmark = ui.backupconfig(experimental, configbookmark)
    oldcreate = ui.backupconfig(experimental, configcreate)
    oldphasemove = None

    try:
        bookmark = opts.get('to') or ''
        create = opts.get('create') or False

        scratchpush = opts.get('bundle_store')
        if _scratchbranchmatcher(bookmark):
            # Hack to fix interaction with remotenames. Remotenames push
            # '--to' bookmark to the server but we don't want to push scratch
            # bookmark to the server. Let's delete '--to' and '--create' and
            # also set allow_anon to True (because if --to is not set
            # remotenames will think that we are pushing anonymoush head)
            if 'to' in opts:
                del opts['to']
            if 'create' in opts:
                del opts['create']
            opts['allow_anon'] = True
            ui.setconfig(experimental, configbookmark, bookmark, '--to')
            ui.setconfig(experimental, configcreate, create, '--create')
            scratchpush = True
            # bundle2 can be sent back after push (for example, bundle2
            # containing `pushkey` part to update bookmarks)
            ui.setconfig(experimental, 'bundle2.pushback', True)

        if scratchpush:
            ui.setconfig(experimental, configscratchpush, True)
            oldphasemove = wrapfunction(exchange,
                                        '_localphasemove',
                                        _phasemove)
        # Copy-paste from `push` command
        path = ui.paths.getpath(dest, default=('default-push', 'default'))
        if not path:
            raise error.Abort(_('default repository not configured!'),
                             hint=_("see 'hg help config.paths'"))
        destpath = path.pushloc or path.loc
        if destpath.startswith('svn+') and scratchpush:
            raise error.Abort('infinite push does not work with svn repo',
                              hint='did you forget to `hg push default`?')
        # Remote scratch bookmarks will be deleted because remotenames doesn't
        # know about them. Let's save it before push and restore after
        remotescratchbookmarks = _readscratchremotebookmarks(ui, repo, destpath)
        result = orig(ui, repo, dest, *args, **opts)
        if _isremotebooksenabled(ui):
            if bookmark and scratchpush:
                other = hg.peer(repo, opts, destpath)
                fetchedbookmarks = other.listkeyspatterns('bookmarks',
                                                          patterns=[bookmark])
                remotescratchbookmarks.update(fetchedbookmarks)
            _saveremotebookmarks(repo, remotescratchbookmarks, destpath)
    finally:
        ui.restoreconfig(oldbookmark)
        ui.restoreconfig(oldcreate)
        if oldphasemove:
            exchange._localphasemove = oldphasemove
    return result

def _getcommonprefix(ui, repo):
    username = ui.shortuser(ui.username())
    hostname = socket.gethostname()

    result = '/'.join(('infinitepush',
                     'backups',
                     username,
                     hostname))
    if not repo.origroot.startswith('/'):
        result += '/'
    result += repo.origroot
    if result.endswith('/'):
        result = result[:-1]
    return result

def _getbackupbookmarkprefix(ui, repo):
    return '/'.join((_getcommonprefix(ui, repo),
                     'bookmarks'))

def _escapebookmark(bookmark):
    '''
    If `bookmark` contains "/bookmarks/" as a substring then replace it with
    "/bookmarksbookmarks/". This will make parsing remote bookmark name
    unambigious.
    '''

    return bookmark.replace('/bookmarks/', '/bookmarksbookmarks/')

def _getbackupbookmarkname(ui, bookmark, repo):
    bookmark = encoding.fromlocal(bookmark)
    bookmark = _escapebookmark(bookmark)
    return '/'.join((_getbackupbookmarkprefix(ui, repo), bookmark))

def _getbackupheadprefix(ui, repo):
    return '/'.join((_getcommonprefix(ui, repo),
                     'heads'))

def _getbackupheadname(ui, hexhead, repo):
    return '/'.join((_getbackupheadprefix(ui, repo), hexhead))

def _getremote(repo, ui, dest, **opts):
    path = ui.paths.getpath(dest, default=('default-push', 'default'))
    if not path:
        raise error.Abort(_('default repository not configured!'),
                         hint=_("see 'hg help config.paths'"))
    dest = path.pushloc or path.loc
    return hg.peer(repo, opts, dest)

@command('debugbackup', [('', 'background', None, 'run backup in background')])
def backup(ui, repo, dest=None, **opts):
    """
    Pushes commits, bookmarks and heads to infinitepush.
    New non-extinct commits are saved since the last `hg debugbackup` or since 0
    revision if this backup is the first.
    Local bookmarks are saved remotely as:
        infinitepush/backups/USERNAME/HOST/REPOROOT/bookmarks/LOCAL_BOOKMARK
    Local heads are saved remotely as:
        infinitepush/backups/USERNAME/HOST/REPOROOT/heads/HEAD_HASH
    """

    if opts.get('background'):
        background_cmd = 'hg debugbackup'
        logfile = ui.config('infinitepush', 'debugbackuplog')
        if logfile:
            background_cmd = background_cmd + ' &> ' + logfile
        runshellcommand(background_cmd, os.environ)
        return 0
    backupedstatefile = 'infinitepushlastbackupedstate'
    backuptipbookmarkshash = repo.svfs.tryread(backupedstatefile).split(' ')
    backuptip = 0
    # hash of empty string is the default. This is to prevent backuping of
    # empty repo
    bookmarkshash = hashlib.sha1().hexdigest()
    if len(backuptipbookmarkshash) == 2:
        try:
            backuptip = int(backuptipbookmarkshash[0]) + 1
        except ValueError:
            pass
        if len(backuptipbookmarkshash[1]) == 40:
            bookmarkshash = backuptipbookmarkshash[1]

    bookmarkstobackup = {}
    for bookmark, node in repo._bookmarks.iteritems():
        bookmark = _getbackupbookmarkname(ui, bookmark, repo)
        hexnode = hex(node)
        bookmarkstobackup[bookmark] = hexnode

    for headrev in repo.revs('head() & not public()'):
        hexhead = repo[headrev].hex()
        headbookmarksname = _getbackupheadname(ui, hexhead, repo)
        bookmarkstobackup[headbookmarksname] = hexhead

    currentbookmarkshash = hashlib.sha1()
    for book, node in sorted(bookmarkstobackup.iteritems()):
        currentbookmarkshash.update(book)
        currentbookmarkshash.update(node)
    currentbookmarkshash = currentbookmarkshash.hexdigest()

    # Use unfiltered repo because backuptip may now point to obsolete changeset
    repo = repo.unfiltered()

    # To avoid race conditions save current tip of the repo and backup
    # everything up to this revision.
    currenttiprev = len(repo) - 1
    revs = []
    if backuptip <= currenttiprev:
        revset = 'head() & draft() & %d:' % backuptip
        revs = list(repo.revs(revset))

    if currentbookmarkshash == bookmarkshash and not revs:
        ui.status(_('nothing to backup\n'))
        return 0

    # Adding patterns to delete previous heads and bookmarks
    bookmarkstobackup[_getbackupheadprefix(ui, repo) + '/*'] = ''
    bookmarkstobackup[_getbackupbookmarkprefix(ui, repo) + '/*'] = ''

    other = _getremote(repo, ui, dest, **opts)
    bundler = bundle2.bundle20(ui, bundle2.bundle2caps(other))
    # Disallow pushback because we want to avoid taking repo locks.
    # And we don't need pushback anyway
    capsblob = bundle2.encodecaps(bundle2.getrepocaps(repo,
                                                      allowpushback=False))
    bundler.newpart('replycaps', data=capsblob)
    if revs:
        nodes = map(repo.changelog.node, revs)
        outgoing = discovery.findcommonoutgoing(repo, other, onlyheads=nodes)
        bundler.addpart(getscratchbranchpart(repo, other, outgoing,
                                             force=False, ui=ui,
                                             bookmark=None, create=False))

    if bookmarkstobackup:
        bundler.addpart(getscratchbookmarkspart(other, bookmarkstobackup))
    stream = util.chunkbuffer(bundler.getchunks())

    try:
        other.unbundle(stream, ['force'], other.url())
    except error.BundleValueError as exc:
        raise error.Abort(_('missing support for %s') % exc)

    with repo.svfs(backupedstatefile, mode="w", atomictemp=True) as f:
        f.write(str(currenttiprev) + ' ' + currentbookmarkshash)
    return 0

def _phasemove(orig, pushop, nodes, phase=phases.public):
    """prevent commits from being marked public

    Since these are going to a scratch branch, they aren't really being
    published."""

    if phase != phases.public:
        orig(pushop, nodes, phase)

@exchange.b2partsgenerator(scratchbranchparttype)
def partgen(pushop, bundler):
    bookmark = pushop.ui.config(experimental, configbookmark)
    create = pushop.ui.configbool(experimental, configcreate)
    scratchpush = pushop.ui.configbool(experimental, configscratchpush)
    if 'changesets' in pushop.stepsdone or not scratchpush:
        return

    if scratchbranchparttype not in bundle2.bundle2caps(pushop.remote):
        return

    pushop.stepsdone.add('changesets')
    if not pushop.outgoing.missing:
        pushop.ui.status(_('no changes found\n'))
        pushop.cgresult = 0
        return

    scratchpart = getscratchbranchpart(pushop.repo,
                                       pushop.remote,
                                       pushop.outgoing,
                                       pushop.force,
                                       pushop.ui,
                                       bookmark,
                                       create)

    bundler.addpart(scratchpart)

    def handlereply(op):
        # server either succeeds or aborts; no code to read
        pushop.cgresult = 1

    return handlereply

@exchange.b2partsgenerator(scratchbookmarksparttype)
def partgen(pushop, bundler):
    manybookmarks = pushop.ui.config(experimental, configmanybookmarks)
    if manybookmarks:
        bundler.addpart(getscratchbookmarkspart(pushop.remote, manybookmarks))

bundle2.capabilities[scratchbranchparttype] = ()
bundle2.capabilities[scratchbookmarksparttype] = ()

def _makebundlefile(part):
    """constructs a temporary bundle file

    part.data should be an uncompressed v1 changegroup"""

    fp = None
    fd, bundlefile = tempfile.mkstemp()
    try:  # guards bundlefile
        try:  # guards fp
            fp = os.fdopen(fd, 'wb')
            magic = 'HG10UN'
            fp.write(magic)
            data = part.read(resource.getpagesize() - len(magic))
            while data:
                fp.write(data)
                data = part.read(resource.getpagesize())
        finally:
            fp.close()
    except Exception:
        try:
            os.unlink(bundlefile)
        except Exception:
            # we would rather see the original exception
            pass
        raise

    return bundlefile

def _makebundlefromraw(data):
    fp = None
    fd, bundlefile = tempfile.mkstemp()
    try:  # guards bundlefile
        try:  # guards fp
            fp = os.fdopen(fd, 'wb')
            fp.write(data)
        finally:
            fp.close()
    except Exception:
        try:
            os.unlink(bundlefile)
        except Exception:
            # we would rather see the original exception
            pass
        raise

    return bundlefile

def _getrevs(bundle, oldnode, force, bookmark):
    'extracts and validates the revs to be imported'
    revs = [bundle[r] for r in bundle.revs('sort(bundle())')]

    # new bookmark
    if oldnode is None:
        return revs

    # Fast forward update
    if oldnode in bundle and list(bundle.set('bundle() & %s::', oldnode)):
        return revs

    # Forced non-fast forward update
    if force:
        return revs
    else:
        raise error.Abort(_('non-forward push'),
                          hint=_('use --force to override'))

@bundle2.parthandler(scratchbranchparttype,
                     ('bookmark', 'bookprevnode' 'create', 'force',
                      'pushbackbookmarks'))
def bundle2scratchbranch(op, part):
    '''unbundle a bundle2 part containing a changegroup to store'''

    params = part.params
    index = op.repo.bundlestore.index
    store = op.repo.bundlestore.store
    op.records.add(scratchbranchparttype + '_skippushkey', True)

    bundlefile = None

    try:  # guards bundlefile
        bundlefile = _makebundlefile(part)
        bundlepath = "bundle:%s+%s" % (op.repo.root, bundlefile)
        bundle = repository(op.repo.ui, bundlepath)

        bookmark = params.get('bookmark')
        bookprevnode = params.get('bookprevnode', '')
        create = params.get('create')
        force = params.get('force')

        if bookmark:
            oldnode = index.getnode(bookmark)

            if not oldnode and not create:
                raise error.Abort("unknown bookmark %s" % bookmark,
                                  hint="use --create if you want to create one")
        else:
            oldnode = None
        bundleheads = bundle.revs('heads(bundle())')
        if bookmark and len(bundleheads) > 1:
            raise error.Abort(
                _('cannot push more than one head to a scratch branch'))

        revs = _getrevs(bundle, oldnode, force, bookmark)

        # Notify the user of what is being pushed
        plural = 's' if len(revs) > 1 else ''
        op.repo.ui.warn(_("pushing %s commit%s:\n") % (len(revs), plural))
        maxoutput = 10
        for i in range(0, min(len(revs), maxoutput)):
            firstline = bundle[revs[i]].description().split('\n')[0][:50]
            op.repo.ui.warn(("    %s  %s\n") % (revs[i], firstline))

        if len(revs) > maxoutput + 1:
            op.repo.ui.warn(("    ...\n"))
            firstline = bundle[revs[-1]].description().split('\n')[0][:50]
            op.repo.ui.warn(("    %s  %s\n") % (revs[-1], firstline))

        nodes = [hex(rev.node()) for rev in revs]
        inindex = lambda rev: bool(index.getbundle(bundle[rev].hex()))
        hasnewheads = any(not inindex(rev) for rev in bundleheads)
        # If there's a bookmark specified, there should be only one head,
        # so we choose the last node, which will be that head.
        # If a bug or malicious client allows there to be a bookmark
        # with multiple heads, we will place the bookmark on the last head.
        bookmarknode = nodes[-1] if nodes else None
        with index:
            if hasnewheads:
                with open(bundlefile, 'r') as f:
                    key = store.write(f.read())
                index.addbundle(key, nodes)
            if bookmark:
                index.addbookmark(bookmark, bookmarknode)
                _maybeaddpushbackpart(op, bookmark, bookmarknode,
                                      bookprevnode, params)
    finally:
        try:
            if bundlefile:
                os.unlink(bundlefile)
        except OSError as e:
            if e.errno != errno.ENOENT:
                raise

    return 1

@bundle2.parthandler(scratchbookmarksparttype, ('prefixestodelete',))
def bundle2scratchbookmarks(op, part):
    '''Handler deletes bookmarks first then adds new bookmarks.
    '''
    index = op.repo.bundlestore.index
    decodedbookmarks = _decodebookmarks(part)
    toinsert = {}
    todelete = []
    for bookmark, node in decodedbookmarks.iteritems():
        if node:
            toinsert[bookmark] = node
        else:
            todelete.append(bookmark)
    with index:
        if todelete:
            index.deletebookmarks(todelete)
        if toinsert:
            index.addmanybookmarks(toinsert)

def _maybeaddpushbackpart(op, bookmark, newnode, oldnode, params):
    if params.get('pushbackbookmarks'):
        if op.reply and 'pushback' in op.reply.capabilities:
            params = {
                'namespace': 'bookmarks',
                'key': bookmark,
                'new': newnode,
                'old': oldnode,
            }
            op.reply.newpart('pushkey', mandatoryparams=params.iteritems())

def bundle2pushkey(orig, op, part):
    if op.records[scratchbranchparttype + '_skippushkey']:
        if op.reply is not None:
            rpart = op.reply.newpart('reply:pushkey')
            rpart.addparam('in-reply-to', str(part.id), mandatory=False)
            rpart.addparam('return', '1', mandatory=False)
        return 1

    return orig(op, part)
