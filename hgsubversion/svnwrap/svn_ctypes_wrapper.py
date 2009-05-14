"""Right now this is a dummy module, but it should wrap the ctypes API and
allow running this more easily without the SWIG bindings.
"""
from csvn import repos

class Revision(object):
    """Wrapper for a Subversion revision.
    """
    def __init__(self, revnum, author, message, date, paths, strip_path=''):
        self.revnum, self.author, self.message = revnum, author, message
        # TODO parse this into a datetime
        self.date = date
        self.paths = {}
        for p in paths:
            self.paths[p[len(strip_path):]] = paths[p]

    def __str__(self):
        return 'r%d by %s' % (self.revnum, self.author)


class SubversionRepo(object):
    """Wrapper for a Subversion repository.

    This uses the SWIG Python bindings, and will only work on svn >= 1.4.
    It takes a required param, the URL.
    """
    def __init__(self, url=''):
        self.svn_url = url

        self.init_ra_and_client()
        self.uuid = ra.get_uuid(self.ra, self.pool)
        repo_root = ra.get_repos_root(self.ra, self.pool)
        # *will* have a leading '/', would not if we used get_repos_root2
        self.subdir = url[len(repo_root):]
        if not self.subdir or self.subdir[-1] != '/':
            self.subdir += '/'

    def init_ra_and_client(self):
        # TODO(augie) need to figure out a way to do auth
        self.repo = repos.RemoteRepository(self.svn_url)

    def HEAD(self):
        raise NotImplementedError
    HEAD = property(HEAD)

    def START(self):
        return 0
    START = property(START)

    def branches(self):
        """Get the branches defined in this repo assuming a standard layout.
        """
        raise NotImplementedError
    branches = property(branches)

    def tags(self):
        """Get the current tags in this repo assuming a standard layout.

        This returns a dictionary of tag: (source path, source rev)
        """
        raise NotImplementedError
    tags = property(tags)

    def _get_copy_source(self, path, cached_head=None):
        """Get copy revision for the given path, assuming it was meant to be
        a copy of the entire tree.
        """
        raise NotImplementedError

    def list_dir(self, dir, revision=None):
        """List the contents of a server-side directory.

        Returns a dict-like object with one dict key per directory entry.

        Args:
          dir: the directory to list, no leading slash
          rev: the revision at which to list the directory, defaults to HEAD
        """
        raise NotImplementedError

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
        raise NotImplementedError


    def fetch_history_at_paths(self, paths, start=None, stop=None,
                               chunk_size=1000):
        raise NotImplementedError

    def get_replay(self, revision, editor, oldest_rev_i_have=0):
        raise NotImplementedError

    def get_unified_diff(self, path, revision, deleted=True, ignore_type=False):
        raise NotImplementedError

    def get_file(self, path, revision):
        raise NotImplementedError

    def proplist(self, path, revision, recurse=False):
        raise NotImplementedError

    def fetch_all_files_to_dir(self, path, revision, checkout_path):
        raise NotImplementedError

class SubversionRepoCanNotReplay(Exception):
    """Exception raised when the svn server is too old to have replay.
    """
