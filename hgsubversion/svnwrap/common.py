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

class SubversionRepoCanNotReplay(Exception):
    """Exception raised when the svn server is too old to have replay.
    """

class SubversionRepoCanNotDiff(Exception):
    """Exception raised when the svn API diff3() command cannot be used
    """

class SubversionConnectionException(Exception):
    """Exception raised when a generic error occurs when connecting to a
       repository.
    """

# Default chunk size used in fetch_history_at_paths() and revisions().
chunk_size = 1000

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
