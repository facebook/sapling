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
from mercurial import localrepo
from mercurial import util as hgutil

peerapi = 0
try:
    try:
        from mercurial.repository import peer as peerrepository
        peerapi = 1
    except ImportError:
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
            ncbackup = self.ui.backupconfig('phases', 'new-commit')
            try:
                self.ui.setconfig('phases', 'new-commit', 'public')
                hash = self.commitctx(ctxctx(ctx))
            finally:
                self.ui.restoreconfig(ncbackup)
            if phases is not None and getattr(self, 'pushkey', False):
                # set phase to be public
                self.pushkey('phases', self[hash].hex(), str(phases.draft), str(phases.public))
            return hash

        if hgutil.safehasattr(localrepo.localrepository, 'push'):
            # Mercurial < 3.2
            # TODO use newbranch to allow branch creation in Subversion?
            @remotesvn
            def push(self, remote, force=False, revs=None, newbranch=None):
                return wrappers.push(self, remote, force, revs)

        if hgutil.safehasattr(localrepo.localrepository, 'pull'):
            # Mercurial < 3.2
            @remotesvn
            def pull(self, remote, heads=[], force=False):
                return wrappers.pull(self, remote, heads, force)

        @remotesvn
        def findoutgoing(self, remote, base=None, heads=None, force=False):
            return wrappers.findoutgoing(repo, remote, heads, force)

        def svnmeta(self, uuid=None, subdir=None, skiperrorcheck=False):
            return svnmeta.SVNMeta(self, uuid, subdir, skiperrorcheck)

    repo.__class__ = svnlocalrepo

class svnremoterepo(peerrepository):
    """ the dumb wrapper for actual Subversion repositories """

    def __init__(self, ui, path=None):
        self._ui = ui
        if path is None:
            path = self.ui.config('paths', 'default-push')
        if path is None:
            path = self.ui.config('paths', 'default')
        if not path:
            raise hgutil.Abort('no Subversion URL specified. Expect '
                               '[path] default= or [path] default-push= '
                               'SVN URL entries in hgrc.')
        self.path = path
        if peerapi == 1:
            self._capabilities = set(['lookup', 'subversion'])
        elif peerapi == 0:
            self.capabilities = set(['lookup', 'subversion'])
        pws = self.ui.config('hgsubversion', 'password_stores', None)
        if pws is not None:
            # Split pws at comas and strip neighbouring whitespace (whitespace
            # at the beginning and end of pws has already been removed by the
            # config parser).
            self.password_stores = re.split(r'\s*,\s*', pws)
        else:
            self.password_stores = None

    if peerapi == 1:
        def capabilities(self):
            return self._capabilities
    elif peerapi == 0:
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
        return self.svnauth[0]

    @propertycache
    def svn(self):
        try:
            auth = self.svnauth
            return svnwrap.SubversionRepo(auth[0], auth[1], auth[2], password_stores=self.password_stores)
        except svnwrap.SubversionConnectionException, e:
            self.ui.traceback()
            raise hgutil.Abort(e)

    @property
    def ui(self):
        return self._ui

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

    if peerapi == 1:
        def canpush(self):
            return True

        def close(self):
            pass

        def iterbatch(self):
            raise NotImplementedError

        def known(self):
            raise NotImplementedError

        def getbundle(self):
            raise NotImplementedError

        def local(self):
            return None

        def peer(self):
            return self

        def stream_out(self):
            raise NotImplementedError

        def unbundle(self):
            raise NotImplementedError

        def branchmap(self):
            raise NotImplementedError

        def debugwireargs(self):
            raise NotImplementedError

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

    svnwrap.prompt_callback(SubversionPrompt(ui))
    return svnremoterepo(ui, url)

class SubversionPrompt(object):
    def __init__(self, ui):
        self.ui = ui

    def maybe_print_realm(self, realm):
        if realm:
            self.ui.write('Authentication realm: %s\n' % (realm,))
            self.ui.flush()

    def username(self, realm, may_save, pool=None):
        self.maybe_print_realm(realm)
        username = self.ui.prompt('Username: ', default='')
        return (username, bool(may_save))

    def simple(self, realm, default_username, may_save, pool=None):
        self.maybe_print_realm(realm)
        if default_username:
            username = default_username
        else:
            username = self.ui.prompt('Username: ', default='')
        password = self.ui.getpass("Password for '%s': " % (username,), default='')
        return (username, password, bool(may_save))

    def ssl_client_cert(self, realm, may_save, pool=None):
        self.maybe_print_realm(realm)
        cert_file = self.ui.prompt('Client certificate filename: ', default='')
        return (cert_file, bool(may_save))

    def ssl_client_cert_pw(self, realm, may_save, pool=None):
        password = self.ui.getpass("Passphrase for '%s': " % (realm,), default='')
        return (password, bool(may_save))

    def insecure(fn):
        def fun(self, *args, **kwargs):
            failures = args[1]
            cert_info = args[2]
            # cert_info[0] is hostname
            # cert_info[1] is fingerprint

            fingerprint = self.ui.config('hostfingerprints', cert_info[0])
            if fingerprint and fingerprint.lower() == cert_info[1].lower():
                # same as the acceptance temporarily
                return (failures, False)

            cacerts = self.ui.config('web', 'cacerts')
            if not cacerts:
                # same as the acceptance temporarily
                return (failures, False)

            return fn(self, *args, **kwargs)
        return fun

    @insecure
    def ssl_server_trust(self, realm, failures, cert_info, may_save, pool=None):
        msg = "Error validating server certificate for '%s':\n" % (realm,)
        if failures & svnwrap.SSL_UNKNOWNCA:
            msg += (
                    ' - The certificate is not issued by a trusted authority. Use the\n'
                    '   fingerprint to validate the certificate manually!\n'
                    )
        if failures & svnwrap.SSL_CNMISMATCH:
            msg += ' - The certificate hostname does not match.\n'
        if failures & svnwrap.SSL_NOTYETVALID:
            msg += ' - The certificate is not yet valid.\n'
        if failures & svnwrap.SSL_EXPIRED:
            msg += ' - The certificate has expired.\n'
        if failures & svnwrap.SSL_OTHER:
            msg += ' - The certificate has an unknown error.\n'
        msg += (
                'Certificate information:\n'
                '- Hostname: %s\n'
                '- Valid: from %s until %s\n'
                '- Issuer: %s\n'
                '- Fingerprint: %s\n'
                ) % (
                        cert_info[0], # hostname
                        cert_info[2], # valid_from
                        cert_info[3], # valid_until
                        cert_info[4], # issuer_dname
                        cert_info[1], # fingerprint
                        )
        if may_save:
            msg += '(R)eject, accept (t)emporarily or accept (p)ermanently? '
            choices = (('&Reject'), ('&Temporarily'), ('&Permanently'))
        else:
            msg += '(R)eject or accept (t)emporarily? '
            choices = (('&Reject'), ('&Temporarily'))
        try:
            choice = self.ui.promptchoice(msg, choices, default=0)
        except TypeError:
            # mercurial version >2.6 use a different syntax and method signature
            msg += '$$ &Reject $$ &Temporarily '
            if may_save:
                msg += '$$ &Permanently '
            choice = self.ui.promptchoice(msg, default=0)

        if choice == 1:
            creds = (failures, False)
        elif may_save and choice == 2:
            creds = (failures, True)
        else:
            creds = None
        return creds
