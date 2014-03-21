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

from hgext import schemes

try:
    # Mercurial 3.0 adds laziness for revsets, which breaks returning lists.
    baseset = revset.baseset
except AttributeError:
    baseset = lambda x: x

hasexchange = False
try:
    from mercurial import exchange
    hasexchange = bool(getattr(exchange, 'push', False))
except ImportError:
    pass

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
                # to issue a pull to refresh .hg/remotebranches
                bmap = {}
                repo = repo.unfiltered()
                for branch, nodes in remote.branchmap().iteritems():
                    bmap[branch] = [n for n in nodes if not repo[n].obsolete()]
                saveremotebranches(repo, path, bmap)
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
                saveremotebranches(repo, path, remote.branchmap())
        except Exception, e:
            ui.debug('remote branches for path %s not saved: %s\n'
                     % (path, e))
    finally:
        lock.release()
        return res

if hasexchange:
    extensions.wrapfunction(exchange, 'push', expush)
    extensions.wrapfunction(exchange, 'pull', expull)

def reposetup(ui, repo):
    if not repo.local():
        return

    opull = getattr(repo.__class__, 'pull', False)
    opush = getattr(repo.__class__, 'push', False)
    olookup = repo.lookup
    ofindtags = repo._findtags

    if opull or opush:
        # Mercurial 3.1 and earlier use push/pull methods on the
        # localrepo object instead of in the exchange module. Avoid
        # reintroducing these methods into newer hg versions so we can
        # continue to detect breakage.
        class rbexchangerepo(repo.__class__):
            def pull(self, remote, *args, **kwargs):
                return expull(opull, self, remote, *args, **kwargs)

            def push(self, remote, *args, **kwargs):
                return expush(opush, self, remote, *args, **kwargs)
        repo.__class__ = rbexchangerepo

    class remotebranchesrepo(repo.__class__):
        def _findtags(self):
            (tags, tagtypes) = ofindtags()
            for tag, n in self._remotebranches.iteritems():
                tags[tag] = n
                tagtypes[tag] = 'remote'
            return (tags, tagtypes)

        @util.propertycache
        def _remotebranches(self):
            remotebranches = {}
            bfile = self.join('remotebranches')
            if os.path.exists(bfile):
                f = open(bfile)
                for line in f:
                    line = line.strip()
                    if line:
                        hash, name = line.split(' ', 1)
                        # look up the hash in the changelog directly
                        # to avoid infinite recursion if the hash is bogus
                        n = self.changelog._match(hash)
                        if n:
                            # we need rev since node will recurse lookup
                            ctx = context.changectx(self,
                                                    self.changelog.rev(n))
                            if not ctx.extra().get('close'):
                                remotebranches[name] = n
            return remotebranches

        def lookup(self, key):
            try:
                if key in self._remotebranches:
                    key = self._remotebranches[key]
            except TypeError: # unhashable type
                pass
            return olookup(key)

    repo.__class__ = remotebranchesrepo

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
        rpath = remote._url

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


def saveremotebranches(repo, remote, bm):
    bfile = repo.join('remotebranches')
    olddata = []
    existed = os.path.exists(bfile)
    alias_default = repo.ui.configbool('remotebranches', 'alias.default')
    if existed:
        f = open(bfile)
        olddata = [l for l in f
                   if not l.split(' ', 1)[1].startswith(remote)]
    f = open(bfile, 'w')
    if existed:
        f.write(''.join(olddata))
    for branch, nodes in bm.iteritems():
        for n in nodes:
            f.write('%s %s/%s\n' % (node.hex(n), remote, branch))
            if remote != 'default' and branch == 'default' and alias_default:
                f.write('%s %s\n' % (node.hex(n), remote))
    f.close()

#########
# revsets
#########

def upstream_revs(filt, repo, subset, x):
    upstream_tips = [node.hex(n) for name, n in
             repo._remotebranches.iteritems() if filt(name)]
    if not upstream_tips: []

    ls = getattr(revset, 'lazyset', False)
    if ls:
        # If revset.lazyset exists (hg 3.0), use lazysets instead for
        # speed.
        tipancestors = repo.revs('::%ln', map(node.bin, upstream_tips))
        def cond(n):
            return n in tipancestors
        return ls(subset, cond)
    # 2.9 and earlier codepath
    upstream = reduce(lambda x, y: x.update(y) or x,
                      map(lambda x: set(revset.ancestors(repo, subset, x)),
                          [('string', n) for n in upstream_tips]),
                      set())
    return [r for r in subset if r in upstream]

def upstream(repo, subset, x):
    '''``upstream()``
    Select changesets in an upstream repository according to remotebranches.
    '''
    args = revset.getargs(x, 0, 0, "upstream takes no arguments")
    upstream_names = [s + '/' for s in
                      repo.ui.configlist('remotebranches', 'upstream')]
    if not upstream_names:
        filt = lambda x: True
    else:
        filt = lambda name: any(map(name.startswith, upstream_names))
    return upstream_revs(filt, repo, subset, x)

def pushed(repo, subset, x):
    '''``pushed()``
    Select changesets in any remote repository according to remotebranches.
    '''
    args = revset.getargs(x, 0, 0, "pushed takes no arguments")
    return upstream_revs(lambda x: True, repo, subset, x)

def remotebranchesrevset(repo, subset, x):
    """``remotebranches()``
    All remote branches heads.
    """
    args = revset.getargs(x, 0, 0, "remotebranches takes no arguments")
    remoterevs = set(repo[n].rev() for n in repo._remotebranches.itervalues())
    return baseset([r for r in subset if r in remoterevs])

revset.symbols.update({'upstream': upstream,
                       'pushed': pushed,
                       'remotebranches': remotebranchesrevset})

###########
# templates
###########

def remotebrancheskw(**args):
    """:remotebranches: List of strings. Any remote branch associated
    with the changeset.
    """
    repo, ctx = args['repo'], args['ctx']
    remotenodes = {}
    for name, node in repo._remotebranches.iteritems():
        remotenodes.setdefault(node, []).append(name)
    if ctx.node() in remotenodes:
        names = sorted(remotenodes[ctx.node()])
        return templatekw.showlist('remotebranch', names,
                                   plural='remotebranches', **args)

templatekw.keywords['remotebranches'] = remotebrancheskw
