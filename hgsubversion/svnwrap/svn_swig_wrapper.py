import cStringIO
import errno
import os
import shutil
import sys
import tempfile
import urllib
import collections

import common

import warnings
warnings.filterwarnings('ignore',
                        module='svn.core',
                        category=DeprecationWarning)

required_bindings = (1, 5, 0)

try:
    from svn import client
    from svn import core
    from svn import delta
    from svn import ra

    subversion_version = (core.SVN_VER_MAJOR, core.SVN_VER_MINOR,
                          core.SVN_VER_MICRO)
except ImportError:
    raise ImportError('Subversion %d.%d.%d or later required, '
                      'but no bindings were found' % required_bindings)

if subversion_version < required_bindings: # pragma: no cover
    raise ImportError('Subversion %d.%d.%d or later required, '
                      'but bindings for %d.%d.%d found' %
                      (required_bindings + subversion_version))

def version():
    return '%d.%d.%d' % subversion_version, 'SWIG'

# exported values
ERR_FS_ALREADY_EXISTS = core.SVN_ERR_FS_ALREADY_EXISTS
ERR_FS_CONFLICT = core.SVN_ERR_FS_CONFLICT
ERR_FS_NOT_FOUND = core.SVN_ERR_FS_NOT_FOUND
ERR_FS_TXN_OUT_OF_DATE = core.SVN_ERR_FS_TXN_OUT_OF_DATE
ERR_INCOMPLETE_DATA = core.SVN_ERR_INCOMPLETE_DATA
ERR_RA_DAV_REQUEST_FAILED = core.SVN_ERR_RA_DAV_REQUEST_FAILED
ERR_REPOS_HOOK_FAILURE = core.SVN_ERR_REPOS_HOOK_FAILURE
SSL_CNMISMATCH = core.SVN_AUTH_SSL_CNMISMATCH
SSL_EXPIRED = core.SVN_AUTH_SSL_EXPIRED
SSL_NOTYETVALID = core.SVN_AUTH_SSL_NOTYETVALID
SSL_OTHER = core.SVN_AUTH_SSL_OTHER
SSL_UNKNOWNCA = core.SVN_AUTH_SSL_UNKNOWNCA
SubversionException = core.SubversionException
Editor = delta.Editor

def apply_txdelta(base, target):
    handler, baton = delta.svn_txdelta_apply(cStringIO.StringIO(base),
                                             target, None)
    return (lambda window: handler(window, baton))

def optrev(revnum):
    optrev = core.svn_opt_revision_t()
    optrev.kind = core.svn_opt_revision_number
    optrev.value.number = revnum
    return optrev

core.svn_config_ensure(None)
svn_config = core.svn_config_get_config(None)
class RaCallbacks(ra.Callbacks):
    @staticmethod
    def open_tmp_file(pool): # pragma: no cover
        (fd, fn) = tempfile.mkstemp()
        os.close(fd)
        return fn

    @staticmethod
    def get_client_string(pool):
        return 'hgsubversion'

def ieditor(fn):
    """Helps identify methods used by the SVN editor interface.

    Stash any exception raised in the method on self.

    This is required because the SWIG bindings just mutate any exception into
    a generic Subversion exception with no way of telling what the original was.
    This allows the editor object to notice when you try and commit and really
    got an exception in the replay process.
    """
    def fun(self, *args, **kwargs):
        try:
            return fn(self, *args, **kwargs)
        except: # pragma: no cover
            if self.current.exception is not None:
                self.current.exception = sys.exc_info()
            raise
    return fun

_prompt = None
def prompt_callback(callback):
    global _prompt
    _prompt = callback

def _simple(realm, default_username, ms, pool):
    ret = _prompt.simple(realm, default_username, ms, pool)
    creds = core.svn_auth_cred_simple_t()
    (creds.username, creds.password, creds.may_save) = ret
    return creds

def _username(realm, ms, pool):
    ret = _prompt.username(realm, ms, pool)
    creds = core.svn_auth_cred_username_t()
    (creds.username, creds.may_save) = ret
    return creds

def _ssl_client_cert(realm, may_save, pool):
    ret = _prompt.ssl_client_cert(realm, may_save, pool)
    creds = core.svn_auth_cred_ssl_client_cert_t()
    (creds.cert_file, creds.may_save) = ret
    return creds

def _ssl_client_cert_pw(realm, may_save, pool):
    ret = _prompt.ssl_client_cert_pw(realm, may_save, pool)
    creds = core.svn_auth_cred_ssl_client_cert_pw_t()
    (creds.password, creds.may_save) = ret
    return creds

def _ssl_server_trust(realm, failures, cert_info, may_save, pool):
    cert = [
            cert_info.hostname,
            cert_info.fingerprint,
            cert_info.valid_from,
            cert_info.valid_until,
            cert_info.issuer_dname,
            ]
    ret = _prompt.ssl_server_trust(realm, failures, cert, may_save, pool)
    if ret:
        creds = core.svn_auth_cred_ssl_server_trust_t()
        (creds.accepted_failures, creds.may_save) = ret
    else:
        creds = None
    return creds

def _create_auth_baton(pool, password_stores):
    """Create a Subversion authentication baton. """
    # Give the client context baton a suite of authentication
    # providers.h
    platform_specific = ['svn_auth_get_gnome_keyring_simple_provider',
                         'svn_auth_get_gnome_keyring_ssl_client_cert_pw_provider',
                         'svn_auth_get_keychain_simple_provider',
                         'svn_auth_get_keychain_ssl_client_cert_pw_provider',
                         'svn_auth_get_kwallet_simple_provider',
                         'svn_auth_get_kwallet_ssl_client_cert_pw_provider',
                         'svn_auth_get_ssl_client_cert_file_provider',
                         'svn_auth_get_windows_simple_provider',
                         'svn_auth_get_windows_ssl_server_trust_provider',
                         ]

    providers = []
    # Platform-dependant authentication methods
    getprovider = getattr(core, 'svn_auth_get_platform_specific_provider',
                          None)
    if getprovider:
        # Available in svn >= 1.6
        if password_stores is None:
            password_stores = ('gnome_keyring', 'keychain', 'kwallet', 'windows')
        for name in password_stores:
            for type in ('simple', 'ssl_client_cert_pw', 'ssl_server_trust'):
                p = getprovider(name, type, pool)
                if p:
                    providers.append(p)
    else:
        for p in platform_specific:
            if hasattr(core, p):
                try:
                    providers.append(getattr(core, p)())
                except RuntimeError:
                    pass

    providers += [
        client.get_simple_provider(),
        client.get_username_provider(),
        client.get_ssl_client_cert_file_provider(),
        client.get_ssl_client_cert_pw_file_provider(),
        client.get_ssl_server_trust_file_provider(),
        ]

    if _prompt:
        providers += [
            client.get_simple_prompt_provider(_simple, 2),
            client.get_username_prompt_provider(_username, 2),
            client.get_ssl_client_cert_prompt_provider(_ssl_client_cert, 2),
            client.get_ssl_client_cert_pw_prompt_provider(_ssl_client_cert_pw, 2),
            client.get_ssl_server_trust_prompt_provider(_ssl_server_trust),
            ]

    return core.svn_auth_open(providers, pool)

_svntypes = {
    core.svn_node_dir: 'd',
    core.svn_node_file: 'f',
    }

class SubversionRepo(object):
    """Wrapper for a Subversion repository.

    It uses the SWIG Python bindings, see above for requirements.
    """
    def __init__(self, url='', username='', password='', head=None, password_stores=None):
        parsed = common.parse_url(url, username, password)
        # --username and --password override URL credentials
        self.username = parsed[0]
        self.password = parsed[1]
        self.svn_url = core.svn_path_canonicalize(parsed[2])
        self.auth_baton_pool = core.Pool()
        self.auth_baton = _create_auth_baton(self.auth_baton_pool, password_stores)
        # self.init_ra_and_client() assumes that a pool already exists
        self.pool = core.Pool()

        self.init_ra_and_client()
        self.uuid = ra.get_uuid(self.ra, self.pool)
        self.svn_url = ra.get_session_url(self.ra, self.pool)
        self.root = ra.get_repos_root(self.ra, self.pool)
        assert self.svn_url.startswith(self.root)
        # *will* have a leading '/', would not if we used get_repos_root2
        self.subdir = self.svn_url[len(self.root):]
        if not self.subdir or self.subdir[-1] != '/':
            self.subdir += '/'
        # the RA interface always yields quoted paths, but the editor interface
        # expects unquoted paths
        self.subdir = urllib.unquote(self.subdir)
        self.hasdiff3 = True
        self.autoprops_config = common.AutoPropsConfig()

    def init_ra_and_client(self):
        """Initializes the RA and client layers, because sometimes getting
        unified diffs runs the remote server out of open files.
        """
        # while we're in here we'll recreate our pool
        self.pool = core.Pool()
        if self.username:
            core.svn_auth_set_parameter(self.auth_baton,
                                        core.SVN_AUTH_PARAM_DEFAULT_USERNAME,
                                        self.username)
        if self.password:
            core.svn_auth_set_parameter(self.auth_baton,
                                        core.SVN_AUTH_PARAM_DEFAULT_PASSWORD,
                                        self.password)
        self.client_context = client.create_context()

        self.client_context.auth_baton = self.auth_baton
        self.client_context.config = svn_config
        callbacks = RaCallbacks()
        callbacks.auth_baton = self.auth_baton
        self.callbacks = callbacks
        try:
            self.ra = ra.open2(self.svn_url, callbacks,
                               svn_config, self.pool)
        except SubversionException, e:
            # e.child contains a detailed error messages
            msglist = []
            svn_exc = e
            while svn_exc:
                if svn_exc.args[0]:
                    msglist.append(svn_exc.args[0])
                svn_exc = svn_exc.child
            msg = '\n'.join(msglist)
            raise common.SubversionConnectionException(msg)

    @property
    def HEAD(self):
        return ra.get_latest_revnum(self.ra, self.pool)

    @property
    def last_changed_rev(self):
        try:
            holder = []
            ra.get_log(self.ra, [''],
                       self.HEAD, 1,
                       1, # limit of how many log messages to load
                       True, # don't need to know changed paths
                       True, # stop on copies
                       lambda paths, revnum, author, date, message, pool:
                           holder.append(revnum),
                       self.pool)

            return holder[-1]
        except SubversionException, e:
            if e.apr_err not in [core.SVN_ERR_FS_NOT_FOUND]:
                raise
            else:
                return self.HEAD

    def list_dir(self, dir, revision=None):
        """List the contents of a server-side directory.

        Returns a dict-like object with one dict key per directory entry.

        Args:
          dir: the directory to list, no leading slash
          rev: the revision at which to list the directory, defaults to HEAD
        """
        # TODO this should just not accept leading slashes like the docstring says
        if dir and dir[-1] == '/':
            dir = dir[:-1]
        if revision is None:
            revision = self.HEAD
        r = ra.get_dir2(self.ra, dir, revision, core.SVN_DIRENT_KIND, self.pool)
        folders, props, junk = r
        return folders

    def revisions(self, paths=None, start=0, stop=0,
                  chunk_size=common.chunk_size):
        """Load the history of this repo.

        This is LAZY. It returns a generator, and fetches a small number
        of revisions at a time.

        The reason this is lazy is so that you can use the same repo object
        to perform RA calls to get deltas.
        """
        if paths is None:
            paths = ['']
        if not stop:
            stop = self.HEAD
        while stop > start:
            def callback(paths, revnum, author, date, message, pool):
                r = common.Revision(revnum, author, message, date, paths,
                                    strip_path=self.subdir)
                revisions.append(r)
            # we only access revisions in a FIFO manner
            revisions = collections.deque()

            try:
                # TODO: using min(start + chunk_size, stop) may be preferable;
                #       ra.get_log(), even with chunk_size set, takes a while
                #       when converting the 65k+ rev. in LLVM.
                ra.get_log(self.ra,
                           paths,
                           start + 1,
                           stop,
                           chunk_size, # limit of how many log messages to load
                           True, # don't need to know changed paths
                           True, # stop on copies
                           callback,
                           self.pool)
            except core.SubversionException, e:
                if e.apr_err == core.SVN_ERR_FS_NOT_FOUND:
                    msg = ('%s not found at revision %d!'
                           % (self.subdir.rstrip('/'), stop))
                    raise common.SubversionConnectionException(msg)
                elif e.apr_err == core.SVN_ERR_FS_NO_SUCH_REVISION:
                    raise common.SubversionConnectionException(e.message)
                else:
                    raise

            while len(revisions) > 1:
                yield revisions.popleft()

            if len(revisions) == 0:
                # exit the loop; there is no history for the path.
                break
            else:
                r = revisions.popleft()
                start = r.revnum
                yield r
            self.init_ra_and_client()

    def commit(self, paths, message, file_data, base_revision, addeddirs,
               deleteddirs, properties, copies):
        """Commits the appropriate targets from revision in editor's store.

        Return the committed revision as a common.Revision instance.
        """
        self.init_ra_and_client()

        def commit_cb(commit_info, pool):
            # disregard commit_info.post_commit_err for now
            r = common.Revision(commit_info.revision, commit_info.author,
                                message, commit_info.date)

            committedrev.append(r)

        committedrev = []

        editor, edit_baton = ra.get_commit_editor2(self.ra,
                                                   message,
                                                   commit_cb,
                                                   None,
                                                   False,
                                                   self.pool)
        checksum = []
        # internal dir batons can fall out of scope and get GCed before svn is
        # done with them. This prevents that (credit to gvn for the idea).
        batons = [edit_baton, ]
        def driver_cb(parent, path, pool):
            if not parent:
                bat = editor.open_root(edit_baton, base_revision, self.pool)
                batons.append(bat)
                return bat
            if path in deleteddirs:
                bat = editor.delete_entry(path, base_revision, parent, pool)
                batons.append(bat)
                return bat
            if path not in file_data:
                if path in addeddirs:
                    bat = editor.add_directory(path, parent, None, -1, pool)
                else:
                    bat = editor.open_directory(path, parent, base_revision, pool)
                batons.append(bat)
                props = properties.get(path, {})
                if 'svn:externals' in props:
                    value = props['svn:externals']
                    editor.change_dir_prop(bat, 'svn:externals', value, pool)
                return bat
            base_text, new_text, action = file_data[path]
            compute_delta = True
            if action == 'modify':
                baton = editor.open_file(path, parent, base_revision, pool)
            elif action == 'add':
                frompath, fromrev = copies.get(path, (None, -1))
                if frompath:
                    frompath = self.path2url(frompath)
                baton = editor.add_file(path, parent, frompath, fromrev, pool)
            elif action == 'delete':
                baton = editor.delete_entry(path, base_revision, parent, pool)
                compute_delta = False

            if path in properties:
                if properties[path].get('svn:special', None):
                    new_text = 'link %s' % new_text
                for p, v in properties[path].iteritems():
                    editor.change_file_prop(baton, p, v)

            if compute_delta:
                handler, wh_baton = editor.apply_textdelta(baton, None,
                                                           self.pool)

                txdelta_stream = delta.svn_txdelta(
                    cStringIO.StringIO(base_text), cStringIO.StringIO(new_text),
                    self.pool)
                delta.svn_txdelta_send_txstream(txdelta_stream, handler,
                                                wh_baton, pool)

                # TODO pass md5(new_text) instead of None
                editor.close_file(baton, None, pool)

        try:
            delta.path_driver(editor, edit_baton, base_revision, paths, driver_cb,
                              self.pool)
        except:
            # If anything went wrong on the preceding lines, we should
            # abort the in-progress transaction.
            editor.abort_edit(edit_baton, self.pool)
            raise

        editor.close_edit(edit_baton, self.pool)

        return committedrev.pop()

    def get_replay(self, revision, editor, oldest_rev_i_have=0):
        # this method has a tendency to chew through RAM if you don't re-init
        self.init_ra_and_client()
        e_ptr, e_baton = delta.make_editor(editor)
        try:
            ra.replay(self.ra, revision, oldest_rev_i_have, True, e_ptr,
                      e_baton, self.pool)
        except SubversionException, e: # pragma: no cover
            # can I depend on this number being constant?
            if (e.apr_err == core.SVN_ERR_RA_NOT_IMPLEMENTED or
                e.apr_err == core.SVN_ERR_UNSUPPORTED_FEATURE):
                msg = ('This Subversion server is older than 1.4.0, and '
                       'cannot satisfy replay requests.')
                raise common.SubversionRepoCanNotReplay(msg)
            else:
                raise

        # if we're not pulling the whole repo, svn fails to report
        # file properties for files merged from subtrees outside ours
        if self.svn_url != self.root:
            links, execs = editor.current.symlinks, editor.current.execfiles
            l = len(self.subdir) - 1
            for f in editor.current.added:
                sf = f[l:]
                if links[f] or execs[f]:
                    continue
                # The list_props API creates a new connection and then
                # calls get_file for the remote file case.  It also
                # creates a new connection to the subversion server
                # every time it's called.  As a result, it's actually
                # *cheaper* to call get_file than list_props here
                data, mode = self.get_file(sf, revision)
                links[f] = mode == 'l'
                execs[f] = mode == 'x'

    def get_revision(self, revision, editor):
        ''' feed the contents of the given revision to the given editor '''

        e_ptr, e_baton = delta.make_editor(editor)

        reporter, reporter_baton = ra.do_update(self.ra, revision, "", True,
                                                e_ptr, e_baton)

        reporter.set_path(reporter_baton, "", revision, True, None)
        reporter.finish_report(reporter_baton)

    def get_unified_diff(self, path, revision, other_path=None, other_rev=None,
                         deleted=True, ignore_type=False):
        """Gets a unidiff of path at revision against revision-1.
        """
        if not self.hasdiff3:
            raise common.SubversionRepoCanNotDiff()
        # works around an svn server keeping too many open files (observed
        # in an svnserve from the 1.2 era)
        self.init_ra_and_client()

        url = self.path2url(path)
        url2 = url
        url2 = (other_path and self.path2url(other_path) or url)
        if other_rev is None:
            other_rev = revision - 1
        old_cwd = os.getcwd()
        tmpdir = tempfile.mkdtemp('svnwrap_temp')
        out, err = None, None
        try:
            # hot tip: the swig bridge doesn't like StringIO for these bad boys
            out_path = os.path.join(tmpdir, 'diffout')
            error_path = os.path.join(tmpdir, 'differr')
            out = open(out_path, 'w')
            err = open(error_path, 'w')
            try:
                client.diff3([], url2, optrev(other_rev), url, optrev(revision),
                             True, True, deleted, ignore_type, 'UTF-8', out, err,
                             self.client_context, self.pool)
            except SubversionException, e:
                # "Can't write to stream: The handle is invalid."
                # This error happens systematically under Windows, possibly
                # related to file handles being non-write shareable by default.
                if e.apr_err != 720006:
                    raise
                self.hasdiff3 = False
                raise common.SubversionRepoCanNotDiff()
            out.close()
            err.close()
            out, err = None, None
            assert len(open(error_path).read()) == 0
            diff = open(out_path).read()
            return diff
        finally:
            if out: out.close()
            if err: err.close()
            shutil.rmtree(tmpdir)
            os.chdir(old_cwd)

    def get_file(self, path, revision):
        """Return content and mode of file at given path and revision.

        "link " prefix is dropped from symlink content. Mode is 'x' if
        file is executable, 'l' if a symlink, the empty string
        otherwise. If the file does not exist at this revision, raise
        IOError.
        """
        assert not path.startswith('/')
        mode = ''
        try:
            out = common.SimpleStringIO()
            info = ra.get_file(self.ra, path, revision, out)
            data = out.getvalue()
            out.close()
            if isinstance(info, list):
                info = info[-1]
            mode = ("svn:executable" in info) and 'x' or ''
            mode = ("svn:special" in info) and 'l' or mode
        except SubversionException, e:
            notfound = (core.SVN_ERR_FS_NOT_FOUND,
                        core.SVN_ERR_RA_DAV_PATH_NOT_FOUND)
            if e.args[1] in notfound: # File not found
                raise IOError(errno.ENOENT, e.args[0])
            raise
        if mode == 'l':
            linkprefix = "link "
            if data.startswith(linkprefix):
                data = data[len(linkprefix):]
        return data, mode

    def list_props(self, path, revision):
        """Return a mapping of property names to values, raise IOError if
        specified path does not exist.
        """
        self.init_ra_and_client()
        rev = optrev(revision)
        rpath = self.path2url(path)
        try:
            pl = client.proplist2(rpath, rev, rev, False,
                                  self.client_context, self.pool)
        except SubversionException, e:
            # Specified path does not exist at this revision
            if e.apr_err == core.SVN_ERR_NODE_UNKNOWN_KIND:
                raise IOError(errno.ENOENT, e.args[0])
            raise
        if not pl:
            return {}
        return pl[0][1]

    def list_files(self, dirpath, revision):
        """List the content of a directory at a given revision, recursively.

        Yield tuples (path, kind) where 'path' is the entry path relatively to
        'dirpath' and 'kind' is 'f' if the entry is a file, 'd' if it is a
        directory. Raise IOError if the directory cannot be found at given
        revision.
        """
        rpath = self.path2url(dirpath)
        pool = core.Pool()
        rev = optrev(revision)
        try:
            entries = client.ls(rpath, rev, True, self.client_context, pool)
        except SubversionException, e:
            if e.apr_err == core.SVN_ERR_FS_NOT_FOUND:
                raise IOError(errno.ENOENT,
                              '%s cannot be found at r%d' % (dirpath, revision))
            raise
        for path, e in entries.iteritems():
            kind = _svntypes.get(e.kind)
            yield path, kind

    def checkpath(self, path, revision):
        """Return the entry type at the given revision, 'f', 'd' or None
        if the entry does not exist.
        """
        kind = ra.check_path(self.ra, path.strip('/'), revision)
        return _svntypes.get(kind)

    def path2url(self, path):
        """Build svn URL for path, URL-escaping path.
        """
        if not path or path == '.':
            return self.svn_url
        assert path[0] != '/', path
        path = path.rstrip('/')
        try:
            # new in svn 1.7
            return core.svn_uri_canonicalize(self.svn_url + '/' + path)
        except AttributeError:
            return self.svn_url + '/' + urllib.quote(path)
