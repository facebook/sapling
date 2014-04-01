import os

from mercurial import config
from mercurial import context
from mercurial import extensions
from mercurial import hg
from mercurial import node
from mercurial import ui
from mercurial import url
from mercurial import util
from mercurial import revset
from mercurial import templatekw
from mercurial import templater
from mercurial import exchange
from mercurial import namespaces

from hgext import schemes

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

extensions.wrapfunction(exchange, 'push', expush)
extensions.wrapfunction(exchange, 'pull', expull)

def reposetup(ui, repo):
    if not repo.local():
        return

    loadremotenames(repo)

# arguably, this needs a better name
def _preferredremotenames(repo):
    """This property is a dictionary of values identical to _remotenames but
    returning only 'preferred' names per path, e.g. '@' instead of
    'default'. A table of behavior is given below:

    +---------+---------+---------+
    |  name1  |  name2  | output  |
    |         |         |         |
    +---------+---------+---------+
    |         | default |         |
    +---------+---------+---------+
    |         |    @    |    @    |
    +---------+---------+---------+
    | default |    @    |    @    |
    +---------+---------+---------+
    |   foo   |    @    |  foo @  |
    +---------+---------+---------+
    |   foo   |   bar   | foo bar |
    +---------+---------+---------+

    """
    ret = {}

    remotenames = repo.names.allnames(repo, 'remotenames')
    # iterate over all the paths so we don't clobber path1/@ with
    # path2/@
    for path, uri in repo.ui.configitems('paths'):

        inverse = {}
        for name in remotenames:
            if not name.startswith(path):
                continue
            node = repo.names.singlenode(repo, name)
            # nothing to check, so add and move on
            if node not in inverse.keys():
                inverse[node] = name
                continue

            # get the ref names, remote will always be the same
            remote, ref1 = splitremotename(inverse[node])
            remote, ref2 = splitremotename(name)

            # prefer anything over default
            if ref2 == 'default':
                continue
            if ref1 == 'default':
                inverse[node] = joinremotename(remote, ref2)
                continue

            # prefer non-empty name to alias (empty) name
            if not ref2:
                continue
            if not ref1:
                inverse[node] = joinremotename(remote, ref2)
                continue

            # if we got to this point then both names are non-default /
            # non-alias names and we should add ref2 to the return list
            # directly (ref1 will be added normally)
            if ref1 and ref2 and ref1 != ref2:
                ret[joinremotename(remote, ref2)] = node

        ret.update(dict([(name, node) for node, name in
                         inverse.iteritems()]))

    return ret

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
        context = dict((str(i+1), v) for i, v in
                       enumerate(parts))
        uri = ''.join(scheme.templater.process(
            scheme.url, context)) + tail
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

    _remotenames = {}
    f = open(rfile)
    for line in f:
        line = line.strip()
        if not line:
            continue
        hash, name = line.split(' ', 1)
        if hash not in repo:
            continue
        ctx = repo[hash]
        if not ctx.extra().get('close'):
            _remotenames[name] = ctx.node()
    f.close()

    ns = namespaces.namespace
    n = ns("remotenames", "remotename",
           lambda rp: _remotenames.keys(),
           lambda rp, name: namespaces.tolist(_remotenames.get(name)),
           lambda rp, node: [name for name, n in _remotenames.iteritems()
                             if n == node])
    repo.names.addnamespace(n)

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
            f.write('%s %s/%s\n' % (node.hex(n), remote, branch))
            if remote != 'default' and branch == 'default' and alias_default:
                f.write('%s %s\n' % (node.hex(n), remote))
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
    def cond(n):
        return n in tipancestors
    return revset.filteredset(subset, cond)

def upstream(repo, subset, x):
    '''``upstream()``
    Select changesets in an upstream repository according to remotenames.
    '''
    args = revset.getargs(x, 0, 0, "upstream takes no arguments")
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
    args = revset.getargs(x, 0, 0, "pushed takes no arguments")
    return upstream_revs(lambda x: True, repo, subset, x)

def remotenamesrevset(repo, subset, x):
    """``remotenames()``
    All remote branches heads.
    """
    args = revset.getargs(x, 0, 0, "remotenames takes no arguments")
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

def preferredremotenameskw(**args):
    """:preferredremotenames: List of strings. List of remote bookmarks and
    branches associated with the changeset where bookmarks are preferred over
    displaying branches.
    """
    repo, ctx = args['repo'], args['ctx']
    remotenames = sorted([name for name, node in
                          _preferredremotenames(repo).iteritems()
                          if node == ctx.node()])
    if remotenames:
        return templatekw.showlist('remotename', remotenames,
                                   plural='remotenames', **args)

def calculateremotedistance(repo, ctx, remote):
    """Helper function to calculate the distance."""
    # get the remote ref (branch or bookmark) here
    remote, ref = splitremotename(remote)

    # only expand default or default-push paths
    if 'default' in remote:
        rpath = dict(repo.ui.configitems('paths')).get(remote, '')
        rpath = activepath(repo.ui, expandscheme(repo.ui, rpath))
        if rpath and rpath != remote:
            remote = rpath

    # similar to the 'current' keyword for bookmarks in templates, we, too,
    # will have 'current' be a keyword for the current bookmark falling back to
    # the branch name if there is no bookmark active.
    if ref == 'current':
        ref = repo._bookmarkcurrent
        if not ref:
            ref = ctx.branch()

    remote = joinremotename(remote, ref)

    for name, node in repo.markers('remotename').iteritems():
        if name == remote:
            sign = 1
            ctx1 = ctx
            ctx2 = repo[node]
            if ctx1.rev() < ctx2.rev():
                sign = -1
                ctx1, ctx2 = ctx2, ctx1
            span = repo.revs('%d::%d - %d' % (ctx2.rev(), ctx1.rev(), ctx2.rev()))
            return sign*len(span)
    return 0

def remotedistancekw(**args):
    """:remotedistance: String of the form <remotepath>:<distance>. For the default
     path, calculate the distance from the changeset to the remotepath,
     e.g. default/default

    """
    repo, ctx = args['repo'], args['ctx']
    ref = ctx.branch()

    distances = ['%s:%d' % (name, calculateremotedistance(repo, ctx, name))
                 for name in _preferredremotenames(repo)]
    return templatekw.showlist('remotedistance', distances,
                               plural='remotedistances', **args)


def remotedistance(context, mapping, args):
    """:remotedistance: String of the form <remotepath>:<distance>. Given a remote
    branch calculate the distance from the changeset to the remotepath,
    e.g. smf/default

    """
    remote = templater.stringify(args[0][1])
    ctx = mapping['ctx']
    repo = ctx._repo.unfiltered()

    return calculateremotedistance(repo, ctx, remote)

templatekw.keywords['preferredremotenames'] = preferredremotenameskw
templatekw.keywords['remotedistance'] = remotedistancekw
templater.funcs['remotedistance'] = remotedistance
