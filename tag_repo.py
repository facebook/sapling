from mercurial import node
from mercurial import util as hgutil
import mercurial.repo

import hg_delta_editor
import util
import wrappers

def generate_repo_class(ui, repo):
    def localsvn(fn):
        '''
        Filter for instance methods which only apply to local Subversion
        repositories.
        '''
        if util.is_svn_repo(repo):
            return fn
        else:
            original = repo.__getattribute__(fn.__name__)
            return original

    def remotesvn(fn):
        '''
        Filter for instance methods which require the first argument
        to be a remote Subversion repository instance.
        '''
        original = repo.__getattribute__(fn.__name__)
        def wrapper(self, *args, **opts):
            print args
            if not isinstance(args[0], svnremoterepo):
                return original(*args, **opts)
            else:
                return fn(self, *args, **opts)
        wrapper.__name__ = fn.__name__ + '_wrapper'
        wrapper.__doc__ = fn.__doc__
        return wrapper

    class svnlocalrepo(repo.__class__):
        @remotesvn
        def pull(self, remote, heads=None, force=False):
            try:
                lock = self.wlock()
                wrappers.pull(None, self.ui, self, source=remote.path,
                              svn=True, rev=heads, force=force)
            except KeyboardInterrupt:
                pass
            finally:
                lock.release()

        @localsvn
        def tags(self):
            tags = super(svnlocalrepo, self).tags()
            hg_editor = hg_delta_editor.HgChangeReceiver(repo=self)
            for tag, source in hg_editor.tags.iteritems():
                target = hg_editor.get_parent_revision(source[1]+1, source[0])
                tags['tag/%s' % tag] = node.hex(target)
            # TODO: should we even generate these tags?
            if not hasattr(self, '_nofaketags'):
                for (revnum, branch), node_hash in hg_editor.revmap.iteritems():
                    tags['%s@r%d' % (branch or 'trunk', revnum)] = node_hash
            return tags

        @localsvn
        def tagslist(self):
            try:
                self._nofaketags = True
                return super(svnlocalrepo, self).tagslist()
            finally:
                del self._nofaketags

    repo.__class__ = svnlocalrepo

class svnremoterepo(mercurial.repo.repository):
    def __init__(self, ui, path):
        self.ui = ui
        self.path = path
        self.capabilities = set(['lookup'])

    def url(self):
        return self.path

    def lookup(self, key):
        return key

    def cancopy(self):
        return False

    def heads(self, *args, **opts):
        """
        Whenever this function is hit, we abort. The traceback is useful for
        figuring out where to intercept the functionality.
        """
        raise hgutil.Abort('command unavailable for Subversion repositories')

def instance(ui, url, create):
    if create:
        raise hgutil.Abort('cannot create new remote Subversion repository')

    if url.startswith('svn+') and not url.startswith('svn+ssh:'):
        url = url[4:]
    return svnremoterepo(ui, util.normalize_url(url))
