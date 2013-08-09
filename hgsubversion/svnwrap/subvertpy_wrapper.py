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

subvertpy_required = (0, 7, 4)
subversion_required = (1, 5, 0)

try:
    from subvertpy import client
    from subvertpy import delta
    from subvertpy import properties
    from subvertpy import ra
    from subvertpy import repos
    import subvertpy
except ImportError:
    raise ImportError('Subvertpy %d.%d.%d or later required, but not found'
                      % subvertpy_required)

def _versionstr(v):
    return '.'.join(str(d) for d in v)

if subvertpy.__version__ < subvertpy_required: # pragma: no cover
    raise ImportError('Subvertpy %s or later required, '
                      'but %s found'
                      % (_versionstr(subvertpy_required),
                         _versionstr(subvertpy.__version__)))

subversion_version = subvertpy.wc.api_version()

if subversion_version[:3] < subversion_required:
    raise ImportError('Subversion %s or later required, '
                      'but Subvertpy is using %s'
                      % (_versionstr(subversion_required),
                         _versionstr(subversion_version[:3])))


def version():
    svnvers = _versionstr(subversion_version[:3])
    if subversion_version[3]:
        svnvers += '-' + subversion_version[3]
    return (svnvers, 'Subvertpy ' + _versionstr(subvertpy.__version__))

def create_and_load(repopath, dumpfd):
    ''' create a new repository at repopath and load the given dump into it '''
    repo = repos.create(repopath)

    nullfd = open(os.devnull, 'w')

    try:
        repo.load_fs(dumpfd, nullfd, repos.LOAD_UUID_FORCE)
    finally:
        dumpfd.close()
        nullfd.close()

# exported values
ERR_FS_ALREADY_EXISTS = subvertpy.ERR_FS_ALREADY_EXISTS
ERR_FS_CONFLICT = subvertpy.ERR_FS_CONFLICT
ERR_FS_NOT_FOUND = subvertpy.ERR_FS_NOT_FOUND
ERR_FS_TXN_OUT_OF_DATE = subvertpy.ERR_FS_TXN_OUT_OF_DATE
ERR_INCOMPLETE_DATA = subvertpy.ERR_INCOMPLETE_DATA
ERR_RA_DAV_PATH_NOT_FOUND = subvertpy.ERR_RA_DAV_PATH_NOT_FOUND
ERR_RA_DAV_REQUEST_FAILED = subvertpy.ERR_RA_DAV_REQUEST_FAILED
ERR_REPOS_HOOK_FAILURE = subvertpy.ERR_REPOS_HOOK_FAILURE
SSL_CNMISMATCH = subvertpy.SSL_CNMISMATCH
SSL_EXPIRED = subvertpy.SSL_EXPIRED
SSL_NOTYETVALID = subvertpy.SSL_NOTYETVALID
SSL_OTHER = subvertpy.SSL_OTHER
SSL_UNKNOWNCA = subvertpy.SSL_UNKNOWNCA
SubversionException = subvertpy.SubversionException
apply_txdelta = delta.apply_txdelta_handler
# superclass for editor.HgEditor
Editor = object

def ieditor(fn):
    """No-op decorator to identify methods used by the SVN editor interface.

    This decorator is not needed for Subvertpy, but is retained for
    compatibility with the SWIG bindings.
    """

    return fn

_prompt = None
def prompt_callback(callback):
    global _prompt
    _prompt = callback

_svntypes = {
    subvertpy.NODE_DIR: 'd',
    subvertpy.NODE_FILE: 'f',
}

class PathAdapter(object):
    __slots__ = ('action', 'copyfrom_path', 'copyfrom_rev')

    def __init__(self, action, copyfrom_path, copyfrom_rev):
        self.action = action
        self.copyfrom_path = copyfrom_path
        self.copyfrom_rev = copyfrom_rev

        if self.copyfrom_path:
            self.copyfrom_path = intern(self.copyfrom_path)

    def __repr__(self):
        return '%s(%r, %r, %r)' % (type(self).__name__, self.action,
                                     self.copyfrom_path, self.copyfrom_rev)


class BaseEditor(object):
    __slots__ = ('editor', 'baton')

    def __init__(self, editor, baton=None):
        self.editor = editor
        self.baton = baton

    def set_target_revision(self, rev):
        pass

    def open_root(self, base_revnum):
        baton = self.editor.open_root(None, base_revnum)
        return DirectoryEditor(self.editor, baton)

    def abort(self):
        # TODO: should we do something special here?
        self.close()

    def close(self):
        del self.editor

class FileEditor(BaseEditor):
    __slots__ = ()

    def __init__(self, editor, baton):
        super(FileEditor, self).__init__(editor, baton)

    def change_prop(self, name, value):
        self.editor.change_file_prop(self.baton, name, value, pool=None)

    def apply_textdelta(self, base_checksum):
        return self.editor.apply_textdelta(self.baton, base_checksum)

    def close(self, checksum=None):
        self.editor.close_file(self.baton, checksum)
        super(FileEditor, self).close()

class DirectoryEditor(BaseEditor):
    __slots__ = ()

    def __init__(self, editor, baton):
        super(DirectoryEditor, self).__init__(editor, baton)

    def delete_entry(self, path, revnum):
        self.editor.delete_entry(path, revnum, self.baton)

    def open_directory(self, path, base_revnum):
        baton = self.editor.open_directory(path, self.baton, base_revnum)
        return DirectoryEditor(self.editor, baton)

    def add_directory(self, path, copyfrom_path=None, copyfrom_rev=-1):
        baton = self.editor.add_directory(
            path, self.baton, copyfrom_path, copyfrom_rev)
        return DirectoryEditor(self.editor, baton)

    def open_file(self, path, base_revnum):
        baton = self.editor.open_file(path, self.baton, base_revnum)
        return FileEditor(self.editor, baton)

    def add_file(self, path, copyfrom_path=None, copyfrom_rev=-1):
        baton = self.editor.add_file(
            path, self.baton, copyfrom_path, copyfrom_rev)
        return FileEditor(self.editor, baton)

    def change_prop(self, name, value):
        self.editor.change_dir_prop(self.baton, name, value, pool=None)

    def close(self):
        self.editor.close_directory(self.baton)
        super(DirectoryEditor, self).close()

class SubversionRepo(object):
    """Wrapper for a Subversion repository.

    This wrapper uses Subvertpy, an alternate set of bindings for Subversion
    that's more pythonic and sucks less. See earlier in this file for version
    requirements.

    Note that password stores do not work, the parameter is only here
    to ensure that the API is the same as for the SWIG wrapper.
    """
    def __init__(self, url='', username='', password='', head=None,
                 password_stores=None):
        parsed = common.parse_url(url, username, password)
        # --username and --password override URL credentials
        self.username = parsed[0]
        self.password = parsed[1]
        self.svn_url = parsed[2]

        self.init_ra_and_client()

        self.svn_url = self.remote.get_url()
        self.uuid = self.remote.get_uuid()
        self.root = self.remote.get_repos_root()
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
        """
        Initializes the RA and client layers.

        With the SWIG bindings, getting unified diffs runs the remote server
        sometimes runs out of open files. It is not known whether the Subvertpy
        is affected by this.
        """
        def getclientstring():
            return 'hgsubversion'

        def simple(realm, username, may_save):
            return _prompt.simple(realm, username, may_save)

        def username(realm, may_save):
            return _prompt.username(realm, may_save)

        def ssl_client_cert(realm, may_save):
            return _prompt.ssl_client_cert(realm, may_save)

        def ssl_client_cert_pw(realm, may_save):
            return _prompt.ssl_client_cert_pw(realm, may_save)

        def ssl_server_trust(realm, failures, cert_info, may_save):
            creds = _prompt.ssl_server_trust(realm, failures, cert_info, may_save)
            if creds is None:
                # We need to reject the certificate, but subvertpy doesn't
                # handle None as a return value here, and requires
                # we instead return a tuple of (int, bool). Because of that,
                # we return (0, False) instead.
                creds = (0, False)
            return creds

        providers = ra.get_platform_specific_client_providers()
        providers += [
            ra.get_simple_provider(),
            ra.get_username_provider(),
            ra.get_ssl_client_cert_file_provider(),
            ra.get_ssl_client_cert_pw_file_provider(),
            ra.get_ssl_server_trust_file_provider(),
        ]
        if _prompt:
            providers += [
                ra.get_simple_prompt_provider(simple, 2),
                ra.get_username_prompt_provider(username, 2),
                ra.get_ssl_client_cert_prompt_provider(ssl_client_cert, 2),
                ra.get_ssl_client_cert_pw_prompt_provider(ssl_client_cert_pw, 2),
                ra.get_ssl_server_trust_prompt_provider(ssl_server_trust),
            ]

        auth = ra.Auth(providers)
        if self.username:
            auth.set_parameter(subvertpy.AUTH_PARAM_DEFAULT_USERNAME, self.username)
        if self.password:
            auth.set_parameter(subvertpy.AUTH_PARAM_DEFAULT_PASSWORD, self.password)

        try:
            self.remote = ra.RemoteAccess(url=self.svn_url,
                                          client_string_func=getclientstring,
                                          auth=auth)
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

        self.client = client.Client()
        self.client.auth = auth

    @property
    def HEAD(self):
        return self.remote.get_latest_revnum()

    @property
    def last_changed_rev(self):
        try:
            holder = []
            def callback(paths, revnum, props, haschildren):
                holder.append(revnum)

            self.remote.get_log(paths=[''],
                                start=self.HEAD, end=1, limit=1,
                                discover_changed_paths=False,
                                callback=callback)

            return holder[-1]
        except SubversionException, e:
            if e.args[0] == ERR_FS_NOT_FOUND:
                raise
            else:
                return self.HEAD

    def list_dir(self, path, revision=None):
        """List the contents of a server-side directory.

        Returns a dict-like object with one dict key per directory entry.

        Args:
          dir: the directory to list, no leading slash
          rev: the revision at which to list the directory, defaults to HEAD
        """
        # TODO: reject leading slashes like the docstring says
        if path:
            path = path.rstrip('/') + '/'

        r = self.remote.get_dir(path, revision or self.HEAD, ra.DIRENT_ALL)
        dirents, fetched_rev, properties = r
        return dirents

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
            def callback(paths, revnum, props, haschildren):
                if paths is None:
                    return
                r = common.Revision(revnum,
                             props.get(properties.PROP_REVISION_AUTHOR),
                             props.get(properties.PROP_REVISION_LOG),
                             props.get(properties.PROP_REVISION_DATE),
                             dict([(k, PathAdapter(*v))
                                   for k, v in paths.iteritems()]),
                             strip_path=self.subdir)
                revisions.append(r)
            # we only access revisions in a FIFO manner
            revisions = collections.deque()

            revprops = [properties.PROP_REVISION_AUTHOR,
                        properties.PROP_REVISION_DATE,
                        properties.PROP_REVISION_LOG]
            try:
                # TODO: using min(start + chunk_size, stop) may be preferable;
                #       ra.get_log(), even with chunk_size set, takes a while
                #       when converting the 65k+ rev. in LLVM.
                self.remote.get_log(paths=paths, revprops=revprops,
                                    start=start + 1, end=stop, limit=chunk_size,
                                    discover_changed_paths=True,
                                    callback=callback)
            except SubversionException, e:
                if e.args[1] == ERR_FS_NOT_FOUND:
                    msg = ('%s not found at revision %d!'
                           % (self.subdir.rstrip('/'), stop))
                    raise common.SubversionConnectionException(msg)
                elif e.args[1] == subvertpy.ERR_FS_NO_SUCH_REVISION:
                    raise common.SubversionConnectionException(e.args[0])
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

    def commit(self, paths, message, file_data, base_revision, addeddirs,
               deleteddirs, props, copies):
        """Commits the appropriate targets from revision in editor's store.

        Return the committed revision as a common.Revision instance.
        """
        def commitcb(rev, date, author):
            r = common.Revision(rev, author, message, date)
            committedrev.append(r)

        committedrev = []
        revprops = { properties.PROP_REVISION_LOG: message }
        # revprops.update(props)
        commiteditor = self.remote.get_commit_editor(revprops, commitcb)

        paths = set(paths)
        paths.update(addeddirs)
        paths.update(deleteddirs)

        # ensure that all parents are visited too; this may be slow
        for path in paths.copy():
            for i in xrange(path.count('/'), -1, -1):
                p = path.rsplit('/', i)[0]
                if p in paths:
                    continue
                paths.add(p)
        paths = sorted(paths)

        def visitdir(editor, directory, paths, pathidx):
            while pathidx < len(paths):
                path = paths[pathidx]
                if directory and not path.startswith(directory + '/'):
                    return pathidx

                pathidx += 1

                if path in file_data:
                    # visiting a file
                    base_text, new_text, action = file_data[path]
                    if action == 'modify':
                        fileeditor = editor.open_file(path, base_revision)
                    elif action == 'add':
                        frompath, fromrev = copies.get(path, (None, -1))
                        if frompath:
                            frompath = self.path2url(frompath)
                        fileeditor = editor.add_file(path, frompath, fromrev)
                    elif action == 'delete':
                        editor.delete_entry(path, base_revision)
                        continue
                    else:
                        assert False, "invalid action '%s'" % action

                    if path in props:
                        if props[path].get('svn:special', None):
                            new_text = 'link %s' % new_text
                        for p, v in props[path].iteritems():
                            fileeditor.change_prop(p, v)


                    handler = fileeditor.apply_textdelta()
                    delta.send_stream(cStringIO.StringIO(new_text), handler)
                    fileeditor.close()

                else:
                    # visiting a directory
                    if path in addeddirs:
                        frompath, fromrev = copies.get(path, (None, -1))
                        if frompath:
                            frompath = self.path2url(frompath)
                        direditor = editor.add_directory(path, frompath, fromrev)

                    elif path in deleteddirs:
                        direditor = editor.delete_entry(path, base_revision)
                        continue
                    else:
                        direditor = editor.open_directory(path)

                    if path in props:
                        for p, v in props[path].iteritems():
                            direditor.change_prop(p, v)

                    pathidx = visitdir(direditor, path, paths, pathidx)
                    direditor.close()

            return pathidx

        try:
            rooteditor = commiteditor.open_root()
            visitdir(rooteditor, '', paths, 0)
            rooteditor.close()
        except:
            commiteditor.abort()
            raise
        commiteditor.close()

        return committedrev.pop()

    def get_replay(self, revision, editor, oldestrev=0):

        try:
            self.remote.replay(revision, oldestrev, BaseEditor(editor))
        except (SubversionException, NotImplementedError), e: # pragma: no cover
            # can I depend on this number being constant?
            if (isinstance(e, NotImplementedError) or
                e.args[1] == subvertpy.ERR_RA_NOT_IMPLEMENTED or
                e.args[1] == subvertpy.ERR_UNSUPPORTED_FEATURE):
                msg = ('This Subversion server is older than 1.4.0, and '
                       'cannot satisfy replay requests.')
                raise common.SubversionRepoCanNotReplay(msg)
            else:
                raise

    def get_revision(self, revision, editor):
        ''' feed the contents of the given revision to the given editor '''
        reporter = self.remote.do_update(revision, '', True,
                                         BaseEditor(editor))
        reporter.set_path('', revision, True)
        reporter.finish()

    def get_unified_diff(self, path, revision, other_path=None, other_rev=None,
                         deleted=True, ignore_type=False):
        """Gets a unidiff of path at revision against revision-1.
        """

        url = self.path2url(path)
        url2 = (other_path and self.path2url(other_path) or url)

        if other_rev is None:
            other_rev = revision - 1

        outfile, errfile = self.client.diff(other_rev, revision, url2, url,
                                            no_diff_deleted=deleted,
                                            ignore_content_type=ignore_type)
        error = errfile.read()
        assert not error, error

        return outfile.read()

    def get_file(self, path, revision):
        """Return content and mode of file at given path and revision.

        "link " prefix is dropped from symlink content. Mode is 'x' if
        file is executable, 'l' if a symlink, the empty string
        otherwise. If the file does not exist at this revision, raise
        IOError.
        """
        mode = ''
        try:
            out = common.SimpleStringIO()
            rev, info = self.remote.get_file(path, out, revision)
            data = out.getvalue()
            out.close()
            if isinstance(info, list):
                info = info[-1]
            mode = (properties.PROP_EXECUTABLE in info) and 'x' or ''
            mode = (properties.PROP_SPECIAL in info) and 'l' or mode
        except SubversionException, e:
            if e.args[1] in (ERR_FS_NOT_FOUND, ERR_RA_DAV_PATH_NOT_FOUND):
                # File not found
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
        try:
            pl = self.client.proplist(self.path2url(path), revision,
                                      client.depth_empty)
        except SubversionException, e:
            # Specified path does not exist at this revision
            if e.args[1] == subvertpy.ERR_NODE_UNKNOWN_KIND:
                raise IOError(errno.ENOENT, e.args[0])
            raise
        return pl and pl[0][1] or {}

    def list_files(self, dirpath, revision):
        """List the content of a directory at a given revision, recursively.

        Yield tuples (path, kind) where 'path' is the entry path relatively to
        'dirpath' and 'kind' is 'f' if the entry is a file, 'd' if it is a
        directory. Raise IOError if the directory cannot be found at given
        revision.
        """
        try:
            entries = self.client.list(self.path2url(dirpath), revision,
                                       client.depth_infinity, ra.DIRENT_KIND)
        except SubversionException, e:
            if e.args[1] == subvertpy.ERR_FS_NOT_FOUND:
                raise IOError(errno.ENOENT,
                              '%s cannot be found at r%d' % (dirpath, revision))
            raise
        for path, e in entries.iteritems():
            if not path: continue
            kind = _svntypes.get(e['kind'])
            yield path, kind

    def checkpath(self, path, revision):
        """Return the entry type at the given revision, 'f', 'd' or None
        if the entry does not exist.
        """
        kind = self.remote.check_path(path, revision)
        return _svntypes.get(kind)

    def path2url(self, path):
        """Build svn URL for path, URL-escaping path.
        """
        if not path or path == '.':
            return self.svn_url
        assert path[0] != '/', path
        return '/'.join((self.svn_url, urllib.quote(path).rstrip('/'),))
