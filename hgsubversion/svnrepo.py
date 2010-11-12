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
from mercurial import util as hgutil
from mercurial import httprepo
import mercurial.repo

import util
import wrappers
import svnwrap
import svnmeta

propertycache = hgutil.propertycache

def generate_repo_class(ui, repo):
    """ This function generates the local repository wrapper. """

    superclass = repo.__class__

    def remotesvn(fn):
        """
        Filter for instance methods which require the first argument
        to be a remote Subversion repository instance.
        """
        original = getattr(repo, fn.__name__, None)

        # remove when dropping support for hg < 1.6.
        if original is None and fn.__name__ == 'findoutgoing':
            return

        def wrapper(self, *args, **opts):
            capable = getattr(args[0], 'capable', lambda x: False)
            if capable('subversion'):
                return fn(self, *args, **opts)
            else:
                return original(*args, **opts)
        wrapper.__name__ = fn.__name__ + '_wrapper'
        wrapper.__doc__ = fn.__doc__
        return wrapper

    class svnlocalrepo(superclass):
        # TODO use newbranch to allow branch creation in Subversion?
        @remotesvn
        def push(self, remote, force=False, revs=None, newbranch=None):
            return wrappers.push(self, remote, force, revs)

        @remotesvn
        def pull(self, remote, heads=[], force=False):
            return wrappers.pull(self, remote, heads, force)

        @remotesvn
        def findoutgoing(self, remote, base=None, heads=None, force=False):
            return wrappers.outgoing(repo, remote, heads, force)

        def svnmeta(self, uuid=None, subdir=None):
            return svnmeta.SVNMeta(self, uuid, subdir)

    repo.__class__ = svnlocalrepo

class svnremoterepo(mercurial.repo.repository):
    """ the dumb wrapper for actual Subversion repositories """

    def __init__(self, ui, path=None):
        self.ui = ui
        if path is None:
            path = self.ui.config('paths', 'default')
        self.path = path
        self.capabilities = set(['lookup', 'subversion'])

    @propertycache
    def svnauth(self):
        # DO NOT default the user to hg's getuser(). If you provide
        # *any* default username to Subversion, it won't use any remembered
        # username for the desired realm, breaking OS X Keychain support,
        # GNOME keyring support, and all similar tools.
        user = self.ui.config('hgsubversion', 'username')
        passwd = self.ui.config('hgsubversion', 'password')
        url = util.normalize_url(self.path)
        user, passwd, url = svnwrap.parse_url(url, user, passwd)
        return url, user, passwd

    @property
    def svnurl(self):
        return self.svn.svn_url

    @propertycache
    def svn(self):
        try:
            return svnwrap.SubversionRepo(*self.svnauth)
        except svnwrap.SubversionConnectionException, e:
            self.ui.traceback()
            raise hgutil.Abort(e)

    @property
    def svnuuid(self):
        return self.svn.uuid

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

    def pushkey(self, namespace, key, old, new):
        return False

    def listkeys(self, namespace):
        return {}

def instance(ui, url, create):
    if url.startswith('http://') or url.startswith('https://'):
        try:
            # may yield a bogus 'real URL...' message
            return httprepo.instance(ui, url, create)
        except error.RepoError:
            ui.traceback()
            ui.note('(falling back to Subversion support)\n')

    if create:
        raise hgutil.Abort('cannot create new remote Subversion repository')

    return svnremoterepo(ui, url)
