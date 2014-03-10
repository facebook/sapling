import os

from mercurial import config
from mercurial import context
from mercurial import hg
from mercurial import node
from mercurial import ui
from mercurial import url
from mercurial import util

try:
    from mercurial import revset
    # force demandimport to load revset
    revset.methods
except ImportError:
   revset = None

try:
    from mercurial import templatekw
    # force demandimport to load templatekw
    templatekw.keywords
except ImportError:
    templatekw = None

from hgext import schemes

def reposetup(ui, repo):
    if not repo.local():
        return

    opull = repo.pull
    opush = repo.push
    olookup = repo.lookup
    ofindtags = repo._findtags

    class remotebranchesrepo(repo.__class__):
        def _findtags(self):
            (tags, tagtypes) = ofindtags()
            tags.update(self._remotebranches)
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

        def pull(self, remote, *args, **kwargs):
            res = opull(remote, *args, **kwargs)
            lock = self.lock()
            try:
                try:
                    path = self._activepath(remote)
                    if path:
                        self.saveremotebranches(path, remote.branchmap())
                except Exception, e:
                    ui.debug('remote branches for path %s not saved: %s\n'
                             % (path, e))
            finally:
                lock.release()
                return res

        def push(self, remote, *args, **kwargs):
            res = opush(remote, *args, **kwargs)
            lock = self.lock()
            try:
                try:
                    path = self._activepath(remote)
                    if path:
                        self.saveremotebranches(path, remote.branchmap())
                except Exception, e:
                    ui.debug('remote branches for path %s not saved: %s\n'
                             % (path, e))
            finally:
                lock.release()
                return res

        def _activepath(self, remote):
            conf = config.config()
            rc = self.join('hgrc')
            if os.path.exists(rc):
                fp = open(rc)
                conf.parse('.hgrc', fp.read())
                fp.close()
            realpath = ''
            if 'paths' in conf:
                for path, uri in conf['paths'].items():
                    for s in schemes.schemes.iterkeys():
                        if uri.startswith('%s://' % s):
                            # TODO: refactor schemes so we don't
                            # duplicate this logic
                            ui.note('performing schemes expansion with '
                                    'scheme %s\n' % s)
                            scheme = hg.schemes[s]
                            parts = uri.split('://', 1)[1].split('/',
                                                                 scheme.parts)
                            if len(parts) > scheme.parts:
                                tail = parts[-1]
                                parts = parts[:-1]
                            else:
                                tail = ''
                            context = dict((str(i+1), v) for i, v in
                                           enumerate(parts))
                            uri = ''.join(scheme.templater.process(
                                scheme.url, context)) + tail
                    uri = self.ui.expandpath(uri)
                    if remote.local():
                        uri = os.path.realpath(uri)
                        rpath = getattr(remote, 'root', None)
                        if rpath is None:
                            # Maybe a localpeer? (hg@1ac628cd7113, 2.3)
                            rpath = getattr(getattr(remote, '_repo', None),
                                            'root', None)
                    else:
                        rpath = remote._url
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
                        if path != 'default':
                            break
            return realpath

        def saveremotebranches(self, remote, bm):
            real = {}
            bfile = self.join('remotebranches')
            olddata = []
            existed = os.path.exists(bfile)
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
                    alias_default = ui.configbool('remotebranches', 'alias.default')
                    if remote != 'default' and branch == 'default' and alias_default:
                        f.write('%s %s\n' % (node.hex(n), remote))
                real[branch] = [node.hex(x) for x in nodes]
            f.close()

    repo.__class__ = remotebranchesrepo

try:
    # Mercurial 3.0 adds laziness for revsets, which breaks returning lists.
    baseset = revset.baseset
except AttributeError:
    baseset = lambda x: x

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

if revset is not None:
    revset.symbols.update({'upstream': upstream,
                           'pushed': pushed,
                           'remotebranches': remotebranchesrevset})

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

if templatekw is not None:
    templatekw.keywords['remotebranches'] = remotebrancheskw
