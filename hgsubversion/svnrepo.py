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

import errno

from mercurial import error
from mercurial import util as hgutil

try:
    from mercurial.peer import peerrepository
    from mercurial import httppeer
except ImportError:
    from mercurial.repo import repository as peerrepository
    from mercurial import httprepo as httppeer

try:
    from mercurial import phases
    phases.public # defeat demand import
except ImportError:
    phases = None

import re
import util
import wrappers
import svnwrap
import svnmeta

propertycache = hgutil.propertycache

class ctxctx(object):
    """Proxies a ctx object and ensures files is never empty."""
    def __init__(self, ctx):
        self._ctx = ctx

    def files(self):
        return self._ctx.files() or ['.svn']

    def filectx(self, path, filelog=None):
        if path == '.svn':
            raise IOError(errno.ENOENT, '.svn is a fake file')
        return self._ctx.filectx(path, filelog=filelog)

    def __getattr__(self, name):
        return getattr(self._ctx, name)

    def __getitem__(self, key):
        return self._ctx[key]

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
        def svn_commitctx(self, ctx):
            """Commits a ctx, but defeats manifest recycling introduced in hg 1.9."""
            hash = self.commitctx(ctxctx(ctx))
            if phases is not None and getattr(self, 'pushkey', False):
                # set phase to be public
                self.pushkey('phases', self[hash].hex(), str(phases.draft), str(phases.public))
            return hash

        # TODO use newbranch to allow branch creation in Subversion?
        @remotesvn
        def push(self, remote, force=False, revs=None, newbranch=None):
            return wrappers.push(self, remote, force, revs)

        @remotesvn
        def pull(self, remote, heads=[], force=False):
            return wrappers.pull(self, remote, heads, force)

        @remotesvn
        def findoutgoing(self, remote, base=None, heads=None, force=False):
            return wrappers.findoutgoing(repo, remote, heads, force)

        def svnmeta(self, uuid=None, subdir=None):
            return svnmeta.SVNMeta(self, uuid, subdir)

    repo.__class__ = svnlocalrepo

class svnremoterepo(peerrepository):
    """ the dumb wrapper for actual Subversion repositories """

    def __init__(self, ui, path=None):
        self.ui = ui
        if path is None:
            path = self.ui.config('paths', 'default-push')
        if path is None:
            path = self.ui.config('paths', 'default')
        if not path:
            raise hgutil.Abort('no Subversion URL specified')
        self.path = path
        self.capabilities = set(['lookup', 'subversion'])
        pws = self.ui.config('hgsubversion', 'password_stores', None)
        if pws is not None:
            # Split pws at comas and strip neighbouring whitespace (whitespace
            # at the beginning and end of pws has already been removed by the
            # config parser).
            self.password_stores = re.split(r'\s*,\s*', pws)
        else:
            self.password_stores = None

    def _capabilities(self):
        return self.capabilities

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
            return svnwrap.SubversionRepo(*self.svnauth, password_stores=self.password_stores)
        except svnwrap.SubversionConnectionException, e:
            self.ui.traceback()
            raise hgutil.Abort(e)

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
            return httppeer.instance(ui, url, create)
        except error.RepoError:
            ui.traceback()
            ui.note('(falling back to Subversion support)\n')

    if create:
        raise hgutil.Abort('cannot create new remote Subversion repository')

    return svnremoterepo(ui, url)
