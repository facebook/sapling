# Infinite push
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import
import errno
import logging
import os
import resource
import tempfile

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
    util,
    revset,
    phases,
    wireproto,
)

from mercurial.extensions import wrapcommand, wrapfunction
from mercurial.hg import repository
from mercurial.node import bin, hex
from mercurial.i18n import _
from . import store, indexapi


cmdtable = {}
command = cmdutil.command(cmdtable)

scratchbranchparttype = 'b2x:infinitepush'

experimental = 'experimental'
configbookmark = 'server-bundlestore-bookmark'
configcreate = 'server-bundlestore-create'

def _buildexternalbundlestore(ui):
    put_args = ui.configlist('infinitepush', 'put_args', [])
    put_binary = ui.config('infinitepush', 'put_binary')
    if not put_binary:
        raise error.Abort('put binary is not specified')
    get_args = ui.configlist('infinitepush', 'get_args', [])
    get_binary = ui.config('infinitepush', 'get_binary')
    if not get_binary:
        raise error.Abort('get binary is not specified')
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
    return indexapi.sqlindexapi(
        reponame, host, port, db, user, password,
        logfile, _getloglevel(ui))

def _getloglevel(ui):
    loglevel = ui.config('infinitepush', 'loglevel', 'DEBUG')
    numeric_loglevel = getattr(logging, loglevel.upper(), None)
    if not isinstance(numeric_loglevel, int):
        raise error.Abort(_('invalid log level %s') % loglevel)
    return numeric_loglevel

class bundlestore(object):
    def __init__(self, repo):
        self._repo = repo
        storetype = self._repo.ui.config('infinitepush', 'storetype', 'disk')
        if storetype == 'disk':
            self.store = store.filebundlestore(self._repo.ui, self._repo)
        elif storetype == 'external':
            self.store = _buildexternalbundlestore(self._repo.ui)

        indextype = self._repo.ui.config('infinitepush', 'indextype', 'disk')
        if indextype == 'disk':
            self.index = indexapi.fileindexapi(self._repo)
        elif indextype == 'sql':
            self.index = _buildsqlindex(self._repo.ui)
        else:
            raise error.Abort(
                _('unknown infinitepush index type specified %s') % indextype)

def reposetup(ui, repo):
    if repo.local():
        repo.bundlestore = bundlestore(repo)

def uisetup(ui):
    # remotenames circumvents the default push implementation entirely, so make
    # sure we load after it so that we wrap it.
    order = extensions._order
    order.remove('infinitepush')
    order.append('infinitepush')
    extensions._order = order

def extsetup(ui):
    isserver = ui.configbool('infinitepush', 'server')
    if isserver:
        serverextsetup(ui)
    else:
        clientextsetup(ui)

def serverextsetup(ui):
    origpushkeyhandler = bundle2.parthandlermapping['pushkey']

    def newpushkeyhandler(*args, **kwargs):
        bundle2pushkey(origpushkeyhandler, *args, **kwargs)
    newpushkeyhandler.params = origpushkeyhandler.params
    bundle2.parthandlermapping['pushkey'] = newpushkeyhandler

    wireproto.commands['lookup'] = (
        _lookupwrap(wireproto.commands['lookup'][0]), 'key')
    wrapfunction(exchange, 'getbundle', getbundle)

def clientextsetup(ui):
    entry = wrapcommand(commands.table, 'push', _push)
    # Don't add the 'to' arg if it already exists
    if not any(a for a in entry[1] if a[1] == 'to'):
        entry[1].append(('', 'to', '', _('push revs to this bookmark')))

    if not any(a for a in entry[1] if a[1] == 'create'):
        entry[1].append(
            ('', 'create', None, _('create a new remote bookmark')))

    wrapcommand(commands.table, 'pull', _pull)

    partorder = exchange.b2partsgenorder
    partorder.insert(partorder.index('changeset'),
                     partorder.pop(partorder.index(scratchbranchparttype)))

def getbundle(orig, repo, source, heads=None, common=None, bundlecaps=None,
              **kwargs):
    # Check if heads exists, if not, check bundle store
    if len(heads) == 1:
        if heads[0] not in repo.changelog.nodemap:
            index = repo.bundlestore.index
            store = repo.bundlestore.store
            bundleid = index.getbundle(hex(heads[0]))
            bundleraw = store.read(bundleid)
            bundlefile = _makebundlefromraw(bundleraw)
            bundlepath = "bundle:%s+%s" % (repo.root, bundlefile)
            bundlerepo = repository(repo.ui, bundlepath)
            repo = bundlerepo

    return orig(repo, source, heads=heads, common=common,
                bundlecaps=bundlecaps, **kwargs)

def _lookupwrap(orig):
    def _lookup(repo, proto, key):
        scratchbranchpat = repo.ui.config('infinitepush', 'branchpattern')
        if not scratchbranchpat:
            return orig(repo, proto, key)
        kind, pat, matcher = util.stringmatcher(scratchbranchpat)
        localkey = encoding.tolocal(key)

        if isinstance(localkey, str) and matcher(localkey):
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

def validaterevset(repo, revset):
    """Abort if the revs to be pushed aren't valid for a scratch branch."""
    if not repo.revs(revset):
        raise error.Abort(_('nothing to push'))

    heads = repo.revs('heads(%r)', revset)
    if len(heads) > 1:
        raise error.Abort(
            _('cannot push more than one head to a scratch branch'))

def getscratchbranchpart(repo, peer, outgoing, bookmark, create):
    if not outgoing.missing:
        raise error.Abort(_('no commits to push'))

    if scratchbranchparttype not in bundle2.bundle2caps(peer):
        raise error.Abort(_('no server support for %r') % scratchbranchparttype)

    validaterevset(repo, revset.formatspec('%ln', outgoing.missing))

    cg = changegroup.getlocalchangegroupraw(repo, 'push', outgoing)

    # .upper() marks this as a mandatory part: server will abort if there's no
    #  handler
    return bundle2.bundlepart(
        scratchbranchparttype.upper(),
        mandatoryparams={
            'bookmark': bookmark,
            'create': "1" if create else "0",
        }.items(),
        data=cg)

def _pull(orig, ui, repo, source="default", **opts):
    # Copy paste from `pull` command
    source, branches = hg.parseurl(ui.expandpath(source), opts.get('branch'))
    other = hg.peer(repo, opts, source)

    hasscratchbookmarks = False
    scratchbranchpat = ui.config('infinitepush', 'branchpattern')
    if opts.get('bookmark') and scratchbranchpat:
        kind, pat, matcher = util.stringmatcher(scratchbranchpat)
        bookmarks = []
        revs = opts.get('rev') or []
        for bookmark in opts.get('bookmark'):
            if matcher(bookmark):
                try:
                    if hasscratchbookmarks:
                        raise error.Abort(
                            'not implemented: not possible to pull more than '
                            'one scratch branch')
                    scratchbookmarkrev = other.lookup(bookmark)
                    revs.append(hex(scratchbookmarkrev))
                    hasscratchbookmarks = True
                except error.RepoLookupError:
                    raise error.abort(
                        'remote bookmark %s not found!' % bookmark)
            else:
                bookmarks.append(bookmark)
        opts['bookmark'] = bookmarks
        opts['rev'] = revs
    if hasscratchbookmarks:
        if len(opts.get('bookmark')) > 0:
            raise error.Abort(
                'not implemented: not possible to pull scratch ' +
                'and non-scratch branches at the same time')
        # Set anyincoming to True
        oldfindcommonincoming = wrapfunction(discovery,
                                             'findcommonincoming',
                                             _findcommonincoming)
    try:
        result = orig(ui, repo, source, **opts)
        return result
    finally:
        if hasscratchbookmarks:
            discovery.findcommonincoming = oldfindcommonincoming

def _findcommonincoming(orig, *args, **kwargs):
    common, inc, remoteheads = orig(*args, **kwargs)
    return common, True, remoteheads

def _push(orig, ui, repo, *args, **opts):
    oldbookmark = ui.backupconfig(experimental, configbookmark)
    oldcreate = ui.backupconfig(experimental, configcreate)
    oldphasemove = None

    try:
        bookmark = opts.get('to')
        create = opts.get('create') or False

        scratchbranchpat = ui.config('infinitepush', 'branchpattern', '')
        kind, pat, matcher = util.stringmatcher(scratchbranchpat)
        if matcher(bookmark):
            ui.setconfig(experimental, configbookmark, bookmark, '--to')
            ui.setconfig(experimental, configcreate, create, '--create')
            if ui.config(experimental, configbookmark):
                oldphasemove = wrapfunction(exchange,
                                            '_localphasemove',
                                            _phasemove)
        result = orig(ui, repo, *args, **opts)
    finally:
        ui.restoreconfig(oldbookmark)
        ui.restoreconfig(oldcreate)
        if oldphasemove:
            exchange._localphasemove = oldphasemove
    return result

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
    if 'changesets' in pushop.stepsdone or not bookmark:
        return

    if scratchbranchparttype not in bundle2.bundle2caps(pushop.remote):
        return

    pushop.stepsdone.add('changesets')
    if not pushop.outgoing.missing:
        pushop.ui.status(_('no changes found\n'))
        pushop.cgresult = 0
        return

    rebasepart = getscratchbranchpart(pushop.repo,
                                   pushop.remote,
                                   pushop.outgoing,
                                   bookmark,
                                   create)

    bundler.addpart(rebasepart)

    def handlereply(op):
        # server either succeeds or aborts; no code to read
        pushop.cgresult = 1

    return handlereply

bundle2.capabilities[scratchbranchparttype] = ()

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

def _getrevs(bundle, oldnode):
    'extracts and validates the revs to be imported'
    validaterevset(bundle, 'bundle()')
    revs = [bundle[r] for r in bundle.revs('sort(bundle())')]

    # new bookmark
    if oldnode is None:
        return revs

    # Fast forward update
    if oldnode in bundle and list(bundle.set('bundle() & %s::', oldnode)):
        return revs

    raise error.Abort(
        _('non-forward pushes are not allowed for scratch branches'))

@bundle2.parthandler(scratchbranchparttype, ('bookmark', 'create',))
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
        create = params.get('create')

        oldnode = index.getnode(bookmark)

        if not oldnode and create != "1":
            raise error.Abort("unknown bookmark %s" % bookmark,
                              hint="use --create if you want to create one")
        revs = _getrevs(bundle, oldnode)

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

        newnodes = filter(lambda node: not index.getbundle(node), nodes)
        with open(bundlefile, 'r') as f:
            key = store.write(f.read())
        try:
            index.addbookmarkandbundle(key, newnodes, bookmark, newnodes[-1])
        except NotImplementedError:
            index.addbookmark(bookmark, newnodes[-1])
            index.addbundle(key, newnodes)
    finally:
        try:
            if bundlefile:
                os.unlink(bundlefile)
        except OSError as e:
            if e.errno != errno.ENOENT:
                raise

    return 1

def bundle2pushkey(orig, op, part):
    if op.records[scratchbranchparttype + '_skippushkey']:
        if op.reply is not None:
            rpart = op.reply.newpart('reply:pushkey')
            rpart.addparam('in-reply-to', str(part.id), mandatory=False)
            rpart.addparam('return', '1', mandatory=False)
        return 1

    return orig(op, part)
