import os

from mercurial import config
from mercurial import hg
from mercurial import node
from mercurial import ui
from mercurial import url
from mercurial import util


def reposetup(ui, repo):
    if repo.local():
        opull = repo.pull
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
                            remotebranches[name] = olookup(hash)
                return remotebranches

            def lookup(self, key):
                if key in self._remotebranches:
                    key = self._remotebranches[key]
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
                        uri = self.ui.expandpath(uri)
                        if remote.local():
                            uri = os.path.realpath(uri)
                            rpath = remote.root
                        else:
                            rpath = remote._url
                            if uri.startswith('http'):
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
                    real[branch] = [node.hex(x) for x in nodes]
                f.close()

        repo.__class__ = remotebranchesrepo
