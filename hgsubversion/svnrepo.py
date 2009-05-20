"""
repository class-based interface for hgsubversion

  Copyright (C) 2009, Dan Villiom Podlaski Christiansen <danchr@gmail.com>
  See parent package for licensing.

Internally, Mercurial assumes that every single repository is a localrepository
subclass: pull() is called on the instance pull *to*, but not the one pulled
*from*. To work around this, we create two classes:

- svnremoterepo for Subversion repositories, but it doesn't really do anything.
- svnlocalrepo for local repositories which handles both operations on itself --
  the local, hgsubversion-enabled clone -- and the remote repository. Decorators
  are used to distinguish and filter these operations from others.
"""

from mercurial import error
from mercurial import node
from mercurial import util as hgutil
from mercurial import httprepo
import mercurial.repo

import hg_delta_editor
import util
import wrappers

def generate_repo_class(ui, repo):
    """ This function generates the local repository wrapper. """

    def localsvn(fn):
        """
        Filter for instance methods which only apply to local Subversion
        repositories.
        """
        if util.is_svn_repo(repo):
            return fn
        else:
            return getattr(repo, fn.__name__)

    def remotesvn(fn):
        """
        Filter for instance methods which require the first argument
        to be a remote Subversion repository instance.
        """
        original = getattr(repo.__class__, fn.__name__)
        def wrapper(self, *args, **opts):
            capable = getattr(args[0], 'capable', lambda x: False)
            if capable('subversion'):
                return fn(self, *args, **opts)
            else:
                return original(self, *args, **opts)
        wrapper.__name__ = fn.__name__ + '_wrapper'
        wrapper.__doc__ = fn.__doc__
        return wrapper

    class svnlocalrepo(repo.__class__):
        @remotesvn
        def push(self, remote, force=False, revs=None):
            wrappers.push(self, dest=remote.svnurl, force=force, revs=None)

        @remotesvn
        def pull(self, remote, heads=None, force=False):
            lock = self.wlock()
            try:
                wrappers.pull(self, source=remote.svnurl,
                              heads=heads, force=force)
            finally:
                lock.release()

        @localsvn
        def tags(self):
            tags = super(svnlocalrepo, self).tags()
            hg_editor = hg_delta_editor.HgChangeReceiver(repo=self)
            for tag, source in hg_editor.tags.iteritems():
                target = hg_editor.get_parent_revision(source[1]+1, source[0])
                tags['tag/%s' % tag] = target
            return tags

    repo.__class__ = svnlocalrepo

class svnremoterepo(mercurial.repo.repository):
    """ the dumb wrapper for actual Subversion repositories """

    def __init__(self, ui, path):
        self.ui = ui
        self.path = path
        self.capabilities = set(['lookup', 'subversion'])

    @property
    def svnurl(self):
        return util.normalize_url(self.path)

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
    if url.startswith('http://') or url.startswith('https://'):
        try:
            # may yield a bogus 'real URL...' message
            return httprepo.instance(ui, url, create)
        except error.RepoError:
            pass

    if create:
        raise hgutil.Abort('cannot create new remote Subversion repository')

    return svnremoterepo(ui, url)
