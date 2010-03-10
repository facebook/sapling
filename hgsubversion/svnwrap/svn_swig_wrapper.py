import cStringIO
import getpass
import errno
import os
import shutil
import sys
import tempfile
import urlparse
import urllib
import collections

import warnings
warnings.filterwarnings('ignore',
                        module='svn.core',
                        category=DeprecationWarning)

from svn import client
from svn import core
from svn import delta
from svn import ra

from mercurial import util as hgutil

def version():
    return '%d.%d.%d' % (core.SVN_VER_MAJOR, core.SVN_VER_MINOR,
                         core.SVN_VER_MICRO)

if (core.SVN_VER_MAJOR, core.SVN_VER_MINOR,
    core.SVN_VER_MICRO) < (1, 5, 0): #pragma: no cover
    raise ImportError, ('You must have Subversion 1.5.0 or newer and '
                        'matching SWIG bindings. You appear to'
                        ' have %s' % version())

class SubversionRepoCanNotReplay(Exception):
    """Exception raised when the svn server is too old to have replay.
    """

class SubversionRepoCanNotDiff(Exception):
    """Exception raised when the svn API diff3() command cannot be used
    """

'''Default chunk size used in fetch_history_at_paths() and revisions().'''
_chunk_size = 1000

def optrev(revnum):
    optrev = core.svn_opt_revision_t()
    optrev.kind = core.svn_opt_revision_number
    optrev.value.number = revnum
    return optrev

svn_config = core.svn_config_get_config(None)
class RaCallbacks(ra.Callbacks):
    @staticmethod
    def open_tmp_file(pool): #pragma: no cover
        (fd, fn) = tempfile.mkstemp()
        os.close(fd)
        return fn

    @staticmethod
    def get_client_string(pool):
        return 'hgsubversion'

def user_pass_prompt(realm, default_username, ms, pool): #pragma: no cover
    # FIXME: should use getpass() and username() from mercurial.ui
    creds = core.svn_auth_cred_simple_t()
    creds.may_save = ms
    if default_username:
        sys.stderr.write('Auth realm: %s\n' % (realm,))
        creds.username = default_username
    else:
        sys.stderr.write('Auth realm: %s\n' % (realm,))
        sys.stderr.write('Username: ')
        sys.stderr.flush()
        creds.username = sys.stdin.readline().strip()
    creds.password = getpass.getpass('Password for %s: ' % creds.username)
    return creds

def _create_auth_baton(pool):
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
        for name in ('gnome_keyring', 'keychain', 'kwallet', 'windows'):
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
        client.get_simple_prompt_provider(user_pass_prompt, 2),
        ]

    return core.svn_auth_open(providers, pool)


def parse_url(url, user=None, passwd=None):
    """Parse a URL and return a tuple (username, password, url)
    """
    scheme, netloc, path, params, query, fragment = urlparse.urlparse(url)
    if '@' in netloc:
        userpass, netloc = netloc.split('@')
        if not user and not passwd:
            if ':' in userpass:
                user, passwd = userpass.split(':')
            else:
                user, passwd = userpass, ''
            user, passwd = urllib.unquote(user), urllib.unquote(passwd)
    if user and scheme == 'svn+ssh':
        netloc = '@'.join((user, netloc, ))
    url = urlparse.urlunparse((scheme, netloc, path, params, query, fragment))
    return (user or None, passwd or None, url)


class Revision(tuple):
    """Wrapper for a Subversion revision.

    Derives from tuple in an attempt to minimise the memory footprint.
    """
    def __new__(self, revnum, author, message, date, paths, strip_path=''):
        _paths = {}
        if paths:
            for p in paths:
                _paths[p[len(strip_path):]] = paths[p]
        return tuple.__new__(self,
                             (revnum, author, message, date, _paths))

    def get_revnum(self):
        return self[0]
    revnum = property(get_revnum)

    def get_author(self):
        return self[1]
    author = property(get_author)

    def get_message(self):
        return self[2]
    message = property(get_message)

    def get_date(self):
        return self[3]
    date = property(get_date)

    def get_paths(self):
        return self[4]
    paths = property(get_paths)

    def __str__(self):
        return 'r%d by %s' % (self.revnum, self.author)

_svntypes = {
    core.svn_node_dir: 'd',
    core.svn_node_file: 'f',
    }

class SubversionRepo(object):
    """Wrapper for a Subversion repository.

    This uses the SWIG Python bindings, and will only work on svn >= 1.4.
    It takes a required param, the URL.
    """
    def __init__(self, url='', username='', password='', head=None):
        parsed = parse_url(url, username, password)
        # --username and --password override URL credentials
        self.username = parsed[0]
        self.password = parsed[1]
        self.svn_url = parsed[2]
        self.auth_baton_pool = core.Pool()
        self.auth_baton = _create_auth_baton(self.auth_baton_pool)
        # self.init_ra_and_client() assumes that a pool already exists
        self.pool = core.Pool()

        self.init_ra_and_client()
        self.uuid = ra.get_uuid(self.ra, self.pool)
        self.root = urllib.unquote(ra.get_repos_root(self.ra, self.pool))
        # *will* have a leading '/', would not if we used get_repos_root2
        self.subdir = url[len(self.root):]
        if not self.subdir or self.subdir[-1] != '/':
            self.subdir += '/'
        self.hasdiff3 = True

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
            url = self.svn_url.encode('utf-8')
            scheme, netloc, path, params, query, fragment = urlparse.urlparse(url)
            path=urllib.quote(path)
            url = urlparse.urlunparse((scheme, netloc, path, params, query, fragment))
            self.ra = ra.open2(url, callbacks,
                               svn_config, self.pool)
        except core.SubversionException, e:
            raise hgutil.Abort(e.args[0])

    def HEAD(self):
        return ra.get_latest_revnum(self.ra, self.pool)
    HEAD = property(HEAD)

    def START(self):
        return 0
    START = property(START)

    def last_changed_rev(self):
        try:
            holder = []
            ra.get_log(self.ra, [''],
                       self.HEAD, 1,
                       1, #limit of how many log messages to load
                       True, # don't need to know changed paths
                       True, # stop on copies
                       lambda paths, revnum, author, date, message, pool:
                           holder.append(revnum),
                       self.pool)

            return holder[-1]
        except core.SubversionException, e:
            if e.apr_err not in [core.SVN_ERR_FS_NOT_FOUND]:
                raise
            else:
                return self.HEAD
    last_changed_rev = property(last_changed_rev)

    def branches(self):
        """Get the branches defined in this repo assuming a standard layout.

        This method should be eliminated; this class does not have
        sufficient knowledge to yield all known tags.
        """
        branches = self.list_dir('branches').keys()
        branch_info = {}
        head=self.HEAD
        for b in branches:
            b_path = 'branches/%s' %b
            hist_gen = self.revisions([b_path], stop=head)
            hist = hist_gen.next()
            source, source_rev = self._get_copy_source(b_path, cached_head=head)
            # This if statement guards against projects that have non-ancestral
            # branches by not listing them has branches
            # Note that they probably are really ancestrally related, but there
            # is just no way for us to know how.
            if source is not None and source_rev is not None:
                branch_info[b] = (source, source_rev, hist.revnum)
        return branch_info
    branches = property(branches)

    def tags(self):
        """Get the current tags in this repo assuming a standard layout.

        This returns a dictionary of tag: (source path, source rev)

        This method should be eliminated; this class does not have
        sufficient knowledge to yield all known tags.
        """
        return self.tags_at_rev(self.HEAD)
    tags = property(tags)

    def tags_at_rev(self, revision):
        """Get the tags in this repo at the given revision, assuming a
        standard layout.

        This returns a dictionary of tag: (source path, source rev)

        This method should be eliminated; this class does not have
        sufficient knowledge to yield all known tags.
        """
        try:
            tags = self.list_dir('tags', revision=revision).keys()
        except core.SubversionException, e:
            if e.apr_err == core.SVN_ERR_FS_NOT_FOUND:
                return {}
            raise
        tag_info = {}
        for t in tags:
            tag_info[t] = self._get_copy_source('tags/%s' % t,
                                                cached_head=revision)
        return tag_info

    def _get_copy_source(self, path, cached_head=None):
        """Get copy revision for the given path, assuming it was meant to be
        a copy of the entire tree.
        """
        if not cached_head:
            cached_head = self.HEAD
        hist_gen = self.revisions([path], stop=cached_head)
        hist = hist_gen.next()
        if hist.paths[path].copyfrom_path is None:
            return None, None
        source = hist.paths[path].copyfrom_path
        source_rev = 0
        for p in hist.paths:
            if not p.startswith(path):
                continue
            if hist.paths[p].copyfrom_rev:
                # We assume that the revision of the source tree as it was
                # copied was actually the revision of the highest revision
                # copied item. This could be wrong, but in practice it will
                # *probably* be correct
                if source_rev < hist.paths[p].copyfrom_rev:
                    source_rev = hist.paths[p].copyfrom_rev
        source = source[len(self.subdir):]
        return source, source_rev

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

    def revisions(self, paths=None, start=None, stop=None,
                  chunk_size=_chunk_size):
        """Load the history of this repo.

        This is LAZY. It returns a generator, and fetches a small number
        of revisions at a time.

        The reason this is lazy is so that you can use the same repo object
        to perform RA calls to get deltas.
        """
        if paths is None:
            paths = ['']
        if not start:
            start = self.START
        if not stop:
            stop = self.HEAD
        while stop > start:
            def callback(paths, revnum, author, date, message, pool):
                r = Revision(revnum, author, message, date, paths,
                             strip_path=self.subdir)
                revisions.append(r)
            # use a queue; we only access revisions in a FIFO manner
            revisions = collections.deque()

            try:
                # TODO: using min(start + chunk_size, stop) may be preferable;
                #       ra.get_log(), even with chunk_size set, takes a while
                #       when converting the 65k+ rev. in LLVM.
                ra.get_log(self.ra,
                           paths,
                           start+1,
                           stop,
                           chunk_size, #limit of how many log messages to load
                           True, # don't need to know changed paths
                           True, # stop on copies
                           callback,
                           self.pool)
            except core.SubversionException, e:
                if e.apr_err == core.SVN_ERR_FS_NOT_FOUND:
                    raise hgutil.Abort('%s not found at revision %d!'
                                       % (self.subdir.rstrip('/'), stop))
                elif e.apr_err == core.SVN_ERR_FS_NO_SUCH_REVISION:
                    raise hgutil.Abort(e.message)
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
        """
        self.init_ra_and_client()
        commit_info = []
        def commit_cb(_commit_info, pool):
            commit_info.append(_commit_info)
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

        delta.path_driver(editor, edit_baton, base_revision, paths, driver_cb,
                          self.pool)
        editor.close_edit(edit_baton, self.pool)

    def get_replay(self, revision, editor, oldest_rev_i_have=0):
        # this method has a tendency to chew through RAM if you don't re-init
        self.init_ra_and_client()
        e_ptr, e_baton = delta.make_editor(editor)
        try:
            ra.replay(self.ra, revision, oldest_rev_i_have, True, e_ptr,
                      e_baton, self.pool)
        except core.SubversionException, e: #pragma: no cover
            # can I depend on this number being constant?
            if (e.apr_err == core.SVN_ERR_RA_NOT_IMPLEMENTED or
                e.apr_err == core.SVN_ERR_UNSUPPORTED_FEATURE):
                raise SubversionRepoCanNotReplay, ('This Subversion server '
                   'is older than 1.4.0, and cannot satisfy replay requests.')
            else:
                raise

    def get_unified_diff(self, path, revision, other_path=None, other_rev=None,
                         deleted=True, ignore_type=False):
        """Gets a unidiff of path at revision against revision-1.
        """
        if not self.hasdiff3:
            raise SubversionRepoCanNotDiff()
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
            except core.SubversionException, e:
                # "Can't write to stream: The handle is invalid."
                # This error happens systematically under Windows, possibly
                # related to file handles being non-write shareable by default.
                if e.apr_err != 720006:
                    raise
                self.hasdiff3 = False
                raise SubversionRepoCanNotDiff()
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
            out = cStringIO.StringIO()
            info = ra.get_file(self.ra, path, revision, out)
            data = out.getvalue()
            out.close()
            if isinstance(info, list):
                info = info[-1]
            mode = ("svn:executable" in info) and 'x' or ''
            mode = ("svn:special" in info) and 'l' or mode
        except core.SubversionException, e:
            notfound = (core.SVN_ERR_FS_NOT_FOUND,
                        core.SVN_ERR_RA_DAV_PATH_NOT_FOUND)
            if e.args[1] in notfound: # File not found
                raise IOError(errno.ENOENT, e.args[0])
            raise
        if mode  == 'l':
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
        except core.SubversionException, e:
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
        except core.SubversionException, e:
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
        return '/'.join((self.svn_url,
                         urllib.quote(path).rstrip('/'),
                         ))
