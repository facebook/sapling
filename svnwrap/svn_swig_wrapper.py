import cStringIO
import getpass
import os
import shutil
import sys
import tempfile

from svn import client
from svn import core
from svn import delta
from svn import ra

svn_config = core.svn_config_get_config(None)


def user_pass_prompt(realm, default_username, ms, pool):
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
    providers = [
        client.get_simple_provider(),
        client.get_username_provider(),
        client.get_ssl_client_cert_file_provider(),
        client.get_ssl_client_cert_pw_file_provider(),
        client.get_ssl_server_trust_file_provider(),
        ]
    # Platform-dependant authentication methods
    if hasattr(client, 'get_windows_simple_provider'):
        try:
            providers.append(client.get_windows_simple_provider())
        except:
            pass
    if hasattr(client, 'get_keychain_simple_provider'):
        try:
            providers.append(client.get_keychain_simple_provider())
        except:
            pass
    providers.extend([client.get_simple_prompt_provider(user_pass_prompt, 2),
                     ])
    return core.svn_auth_open(providers, pool)


class Revision(object):
    """Wrapper for a Subversion revision.
    """
    def __init__(self, revnum, author, message, date, paths, strip_path=''):
        self.revnum, self.author, self.message = revnum, author, message
        # TODO parse this into a datetime
        self.date = date
        self.paths = {}
        if paths:
            for p in paths:
                self.paths[p[len(strip_path):]] = paths[p]

    def __str__(self):
        return 'r%d by %s' % (self.revnum, self.author)

class SubversionRepo(object):
    """Wrapper for a Subversion repository.

    This uses the SWIG Python bindings, and will only work on svn >= 1.4.
    It takes a required param, the URL.
    """
    def __init__(self, url='', username=''):
        self.svn_url = url
        self.uname = username
        self.auth_baton_pool = core.Pool()
        self.auth_baton = _create_auth_baton(self.auth_baton_pool)

        self.init_ra_and_client()
        self.uuid = ra.get_uuid(self.ra, self.pool)
        repo_root = ra.get_repos_root(self.ra, self.pool)
        # *will* have a leading '/', would not if we used get_repos_root2
        self.subdir = url[len(repo_root):]
        if not self.subdir or self.subdir[-1] != '/':
            self.subdir += '/'

    def init_ra_and_client(self):
        """Initializes the RA and client layers, because sometimes getting
        unified diffs runs the remote server out of open files.
        """
        # while we're in here we'll recreate our pool
        self.pool = core.Pool()
        self.client_context = client.create_context()
        core.svn_auth_set_parameter(self.auth_baton,
                                    core.SVN_AUTH_PARAM_DEFAULT_USERNAME,
                                    self.uname)

        self.client_context.auth_baton = self.auth_baton
        self.client_context.config = svn_config
        self.ra = client.open_ra_session(self.svn_url.encode('utf8'),
                                         self.client_context)


    @property
    def HEAD(self):
        return ra.get_latest_revnum(self.ra, self.pool)

    @property
    def START(self):
        return 0

    @property
    def branches(self):
        """Get the branches defined in this repo assuming a standard layout.
        """
        branches = self.list_dir('branches').keys()
        branch_info = {}
        head=self.HEAD
        for b in branches:
            b_path = 'branches/%s' %b
            hist_gen = self.fetch_history_at_paths([b_path], stop=head)
            hist = hist_gen.next()
            source, source_rev = self._get_copy_source(b_path, cached_head=head)
            # This if statement guards against projects that have non-ancestral
            # branches by not listing them has branches
            # Note that they probably are really ancestrally related, but there
            # is just no way for us to know how.
            if source is not None and source_rev is not None:
                branch_info[b] = (source, source_rev, hist.revnum)
        return branch_info

    @property
    def tags(self):
        """Get the current tags in this repo assuming a standard layout.

        This returns a dictionary of tag: (source path, source rev)
        """
        tags = self.list_dir('tags').keys()
        tag_info = {}
        head = self.HEAD
        for t in tags:
            tag_info[t] = self._get_copy_source('tags/%s' % t,
                                                cached_head=head)
        return tag_info

    def _get_copy_source(self, path, cached_head=None):
        """Get copy revision for the given path, assuming it was meant to be
        a copy of the entire tree.
        """
        if not cached_head:
            cached_head = self.HEAD
        hist_gen = self.fetch_history_at_paths([path], stop=cached_head)
        hist = hist_gen.next()
        if hist.paths[path].copyfrom_path is None:
            return None, None
        source = hist.paths[path].copyfrom_path
        source_rev = 0
        for p in hist.paths:
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
        if dir[-1] == '/':
            dir = dir[:-1]
        if revision is None:
            revision = self.HEAD
        r = ra.get_dir2(self.ra, dir, revision, core.SVN_DIRENT_KIND, self.pool)
        folders, props, junk = r
        return folders

    def revisions(self, start=None, chunk_size=1000):
        """Load the history of this repo.

        This is LAZY. It returns a generator, and fetches a small number
        of revisions at a time.

        The reason this is lazy is so that you can use the same repo object
        to perform RA calls to get deltas.
        """
        # NB: you'd think this would work, but you'd be wrong. I'm pretty
        # convinced there must be some kind of svn bug here.
        #return self.fetch_history_at_paths(['tags', 'trunk', 'branches'],
        #                                   start=start)
        # this does the same thing, but at the repo root + filtering. It's
        # kind of tough cookies, sadly.
        for r in self.fetch_history_at_paths([''], start=start,
                                             chunk_size=chunk_size):
            should_yield = False
            i = 0
            paths = list(r.paths.keys())
            while i < len(paths) and not should_yield:
                p = paths[i]
                if (p.startswith('trunk') or p.startswith('tags')
                    or p.startswith('branches')):
                    should_yield = True
                i += 1
            if should_yield:
                yield r


    def fetch_history_at_paths(self, paths, start=None, stop=None,
                               chunk_size=1000):
        revisions = []
        def callback(paths, revnum, author, date, message, pool):
            r = Revision(revnum, author, message, date, paths,
                         strip_path=self.subdir)
            revisions.append(r)
        if not start:
            start = self.START
        if not stop:
            stop = self.HEAD
        while stop > start:
            ra.get_log(self.ra, paths,
                        start+1,
                        stop,
                        chunk_size, #limit of how many log messages to load
                        True, # don't need to know changed paths
                        True, # stop on copies
                        callback,
                        self.pool)
            if len(revisions) < chunk_size:
                # this means there was no history for the path, so force the
                # loop to exit
                start = stop
            else:
                start = revisions[-1].revnum
            while len(revisions) > 0:
                yield revisions[0]
                revisions.pop(0)

    def commit(self, paths, message, file_data, base_revision, dirs,
               properties):
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
        def driver_cb(parent, path, pool):
            if path in dirs:
                return editor.add_directory(path, parent, None, -1, pool)
            base_text, new_text, action = file_data[path]
            compute_delta = True
            if action == 'modify':
                baton = editor.open_file(path, parent, base_revision, pool)
            elif action == 'add':
                try:
                    baton = editor.add_file(path, parent, None, -1, pool)
                except (core.SubversionException, TypeError), e:
                    print e.message
                    raise
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
        except core.SubversionException, e:
            # can I depend on this number being constant?
            if (e.message == "Server doesn't support the replay command"
                or e.apr_err == 170003):
                raise SubversionRepoCanNotReplay, ('This Subversion server '
                   'is older than 1.4.0, and cannot satisfy replay requests.')
            else:
                raise

    def get_unified_diff(self, path, revision, other_path=None, other_rev=None,
                         deleted=True, ignore_type=False):
        """Gets a unidiff of path at revision against revision-1.
        """
        # works around an svn server keeping too many open files (observed
        # in an svnserve from the 1.2 era)
        self.init_ra_and_client()

        old_cwd = os.getcwd()
        assert path[0] != '/'
        url = self.svn_url + '/' + path
        url2 = url
        if other_path is not None:
            url2 = self.svn_url + '/' + other_path
        if other_rev is None:
            other_rev = revision - 1
        tmpdir = tempfile.mkdtemp('svnwrap_temp')
        # hot tip: the swig bridge doesn't like StringIO for these bad boys
        out_path = os.path.join(tmpdir, 'diffout')
        error_path = os.path.join(tmpdir, 'differr')
        out = open(out_path, 'w')
        err = open(error_path, 'w')
        rev_old = core.svn_opt_revision_t()
        rev_old.kind = core.svn_opt_revision_number
        rev_old.value.number = other_rev
        rev_new = core.svn_opt_revision_t()
        rev_new.kind = core.svn_opt_revision_number
        rev_new.value.number = revision
        client.diff3([], url2, rev_old, url, rev_new, True, True,
                     deleted, ignore_type, 'UTF-8', out, err,
                     self.client_context, self.pool)
        out.close()
        err.close()
        assert len(open(error_path).read()) == 0
        diff = open(out_path).read()
        os.chdir(old_cwd)
        shutil.rmtree(tmpdir)
        return diff

    def get_file(self, path, revision):
        out = cStringIO.StringIO()
        tmpdir = tempfile.mkdtemp('svnwrap_temp')
        # hot tip: the swig bridge doesn't like StringIO for these bad boys
        out_path = os.path.join(tmpdir, 'diffout')
        out = open(out_path, 'w')
        ra.get_file(self.ra, path,revision, out , None)
        out.close()
        x = open(out_path).read()
        shutil.rmtree(tmpdir)
        return x

    def proplist(self, path, revision, recurse=False):
        rev = core.svn_opt_revision_t()
        rev.kind = core.svn_opt_revision_number
        rev.value.number = revision
        if path[-1] == '/':
            path = path[:-1]
        if path[0] == '/':
            path = path[1:]
        pl = dict(client.proplist2(self.svn_url+'/'+path, rev, rev, True,
                                   self.client_context, self.pool))
        pl2 = {}
        for key, value in pl.iteritems():
            pl2[key[len(self.svn_url)+1:]] = value
        return pl2

    def fetch_all_files_to_dir(self, path, revision, checkout_path):
        rev = core.svn_opt_revision_t()
        rev.kind = core.svn_opt_revision_number
        rev.value.number = revision
        client.export3(self.svn_url+'/'+path, checkout_path, rev,
                       rev, True, True, True, 'LF', # should be 'CRLF' on win32
                       self.client_context, self.pool)

class SubversionRepoCanNotReplay(Exception):
    """Exception raised when the svn server is too old to have replay.
    """
