import os

from mercurial import extensions
from mercurial import hg
from mercurial import ui
from mercurial import url
from mercurial import util
from mercurial import repoview
from mercurial import revset
from mercurial import templatekw
from mercurial import exchange
from mercurial import error
from mercurial import namespaces
from mercurial.node import hex
from hgext import schemes

_remotenames = {}
_remotetypes = {}

def expush(orig, repo, remote, *args, **kwargs):
    res = orig(repo, remote, *args, **kwargs)
    lock = repo.lock()
    try:
        try:
            path = activepath(repo.ui, remote)
            if path:
                # on a push, we don't want to keep obsolete heads since
                # they won't show up as heads on the next pull, so we
                # remove them here otherwise we would require the user
                # to issue a pull to refresh .hg/remotenames
                bmap = {}
                repo = repo.unfiltered()
                for branch, nodes in remote.branchmap().iteritems():
                    bmap[branch] = [n for n in nodes if not repo[n].obsolete()]
                saveremotenames(repo, path, bmap, remote.listkeys('bookmarks'))
        except Exception, e:
            ui.debug('remote branches for path %s not saved: %s\n'
                     % (path, e))
    finally:
        lock.release()
        return res

def expull(orig, repo, remote, *args, **kwargs):
    res = orig(repo, remote, *args, **kwargs)
    lock = repo.lock()
    try:
        try:
            path = activepath(repo.ui, remote)
            if path:
                saveremotenames(repo, path, remote.branchmap(),
                                remote.listkeys('bookmarks'))
        except Exception, e:
            ui.debug('remote branches for path %s not saved: %s\n'
                     % (path, e))
    finally:
        lock.release()
        return res

def blockerhook(orig, repo, *args, **kwargs):
    blockers = orig(repo)

    # add remotenames to blockers
    cl = repo.changelog
    ns = repo.names["remotenames"]
    for name in ns.listnames(repo):
        blockers.update(cl.rev(node) for node in
                        ns.nodes(repo, name))

    return blockers

extensions.wrapfunction(exchange, 'push', expush)
extensions.wrapfunction(exchange, 'pull', expull)
extensions.wrapfunction(repoview, '_getdynamicblockers', blockerhook)

def reposetup(ui, repo):
    if not repo.local():
        return

    loadremotenames(repo)

    ns = namespaces.namespace
    n = ns("remotenames", "remotename",
           lambda rp: _remotenames.keys(),
           lambda rp, name: namespaces.tolist(_remotenames.get(name)),
           lambda rp, node: [name for name, n in _remotenames.iteritems()
                             if n == node])
    repo.names.addnamespace(n)

def activepath(ui, remote):
    realpath = ''
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
        except:
            rpath = remote.url

    for path, uri in ui.configitems('paths'):
        uri = ui.expandpath(expandscheme(ui, uri))
        if local:
            uri = os.path.realpath(uri)
        else:
            if uri.startswith('http'):
                try:
                    uri = url.url(uri).authinfo()[0]
                except AttributeError:
                    try:
                        uri = util.url(uri).authinfo()[0]
                    except AttributeError:
                        uri = url.getauthinfo(uri)[0]
        uri = uri.rstrip('/')
        rpath = rpath.rstrip('/')
        if uri == rpath:
            realpath = path
            # prefer a non-default name to default
            if path != 'default' and path != 'default-push':
                break
    return realpath

def expandscheme(ui, uri):
    '''For a given uri, expand the scheme for it'''
    urischemes = [s for s in schemes.schemes.iterkeys()
                  if uri.startswith('%s://' % s)]
    for s in urischemes:
        # TODO: refactor schemes so we don't
        # duplicate this logic
        ui.note('performing schemes expansion with '
                'scheme %s\n' % s)
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

def loadremotenames(repo):
    rfile = repo.join('remotenames')
    # exit early if there is nothing to do
    if not os.path.exists(rfile):
        return

    branches = repo.names['branches'].listnames(repo)
    bookmarks = repo.names['bookmarks'].listnames(repo)

    f = open(rfile)
    for line in f:
        line = line.strip()
        if not line:
            continue
        node, name = line.split(' ', 1)
        try:
            ctx = repo[node]
        except error.RepoLookupError:
            continue

        if not ctx.extra().get('close'):
            _remotenames[name] = ctx.node()

        # cache the type of the remote name
        remote, rname = splitremotename(name)
        if rname in branches:
            _remotetypes[name] = 'branches'
        elif rname in bookmarks:
            _remotetypes[name] = 'bookmarks'
    f.close()

def saveremotenames(repo, remote, branches, bookmarks):
    bfile = repo.join('remotenames')
    olddata = []
    existed = os.path.exists(bfile)
    alias_default = repo.ui.configbool('remotenames', 'alias.default')
    if existed:
        f = open(bfile)
        olddata = [l for l in f
                   if not l.split(' ', 1)[1].startswith(remote)]
    f = open(bfile, 'w')
    if existed:
        f.write(''.join(olddata))
    for branch, nodes in branches.iteritems():
        for n in nodes:
            f.write('%s %s/%s\n' % (hex(n), remote, branch))
            if remote != 'default' and branch == 'default' and alias_default:
                f.write('%s %s\n' % (hex(n), remote))
    for bookmark, n in bookmarks.iteritems():
        f.write('%s %s/%s\n' % (n, remote, bookmark))
    f.close()

#########
# revsets
#########

def upstream_revs(filt, repo, subset, x):
    upstream_tips = set()
    ns = repo.names["remotenames"]
    for name in ns.listnames(repo):
        if filt(name):
            upstream_tips.update(ns.nodes(repo, name))

    if not upstream_tips:
        return revset.baseset([])

    tipancestors = repo.revs('::%ln', upstream_tips)
    return revset.filteredset(subset, lambda n: n in tipancestors)

def upstream(repo, subset, x):
    '''``upstream()``
    Select changesets in an upstream repository according to remotenames.
    '''
    revset.getargs(x, 0, 0, "upstream takes no arguments")
    upstream_names = [s + '/' for s in
                      repo.ui.configlist('remotenames', 'upstream')]
    if not upstream_names:
        filt = lambda x: True
    else:
        filt = lambda name: any(map(name.startswith, upstream_names))
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
    ns = repo.names["remotenames"]
    for name in ns.listnames(repo):
        remoterevs.update(ns.nodes(repo, name))
    return revset.baseset(sorted(cl.rev(n) for n in remoterevs))

revset.symbols.update({'upstream': upstream,
                       'pushed': pushed,
                       'remotenames': remotenamesrevset})

###########
# templates
###########

def remotebookmarkskw(**args):
    """:remotebookmarks: List of strings. List of remote bookmarks associated with
    the changeset.

    """
    repo, ctx = args['repo'], args['ctx']

    remotebooks = [name for name in
                   repo.names['remotenames'].names(repo, ctx.node())
                   if _remotetypes[name] == 'bookmarks']

    return templatekw.showlist('remotebookmark', remotebooks,
                               plural='remotebookmarks', **args)

def remotebrancheskw(**args):
    """:remotebranches: List of strings. List of remote branches associated with
    the changeset.

    """
    repo, ctx = args['repo'], args['ctx']

    remotebranches = [name for name in
                      repo.names['remotenames'].names(repo, ctx.node())
                      if _remotetypes[name] == 'branches']

    return templatekw.showlist('remotebranch', remotebranches,
                               plural='remotebranches', **args)

def remotenameskw(**args):
    """:remotenames: List of strings. List of remote names associated with the
    changeset. If remotenames.suppressbranches is True then branch names will
    be hidden if there is a bookmark at the same changeset.

    """
    repo, ctx = args['repo'], args['ctx']

    remotenames = [name for name in
                   repo.names['remotenames'].names(repo, ctx.node())
                   if _remotetypes[name] == 'bookmarks']

    suppress = repo.ui.configbool('remotenames', 'suppressbranches', False)
    if not remotenames or not suppress:
        remotenames += [name for name in
                        repo.names['remotenames'].names(repo, ctx.node())
                        if _remotetypes[name] == 'branches']

    return templatekw.showlist('remotename', remotenames,
                               plural='remotenames', **args)

templatekw.keywords['remotebookmarks'] = remotebookmarkskw
templatekw.keywords['remotebranches'] = remotebrancheskw
templatekw.keywords['remotenames'] = remotenameskw
