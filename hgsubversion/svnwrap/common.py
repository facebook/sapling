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
import fnmatch
import ConfigParser
import sys

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
        netloc = '@'.join((user, netloc,))
    url = urlparse.urlunparse((scheme, netloc, path, params, query, fragment))
    return (user or None, passwd or None, url)


class Revision(tuple):
    """Wrapper for a Subversion revision.

    Derives from tuple in an attempt to minimise the memory footprint.
    """
    def __new__(self, revnum, author, message, date, paths=None, strip_path=''):
        _paths = {}
        if paths:
            for p in paths:
                if p.startswith(strip_path):
                    _paths[p[len(strip_path):]] = paths[p]
        return tuple.__new__(self,
                             (revnum, author, message, date, _paths))

    @property
    def revnum(self):
        return self[0]

    @property
    def author(self):
        return self[1]

    @property
    def message(self):
        return self[2]

    @property
    def date(self):
        return self[3]

    @property
    def paths(self):
        return self[4]

    def __str__(self):
        return 'r%d by %s' % (self.revnum, self.author)


_svn_config_dir = None


class AutoPropsConfig(object):
    """Provides the subversion auto-props functionality
       when pushing new files.
    """
    def __init__(self, config_dir=None):
        config_file = config_file_path(config_dir)
        self.config = ConfigParser.RawConfigParser()
        self.config.read([config_file])

    def properties(self, file):
        """Returns a dictionary of the auto-props applicable for file.
           Takes enable-auto-props into account.
        """
        properties = {}
        if self.autoprops_enabled():
            for pattern,prop_list in self.config.items('auto-props'):
                if fnmatch.fnmatchcase(os.path.basename(file), pattern):
                    properties.update(parse_autoprops(prop_list))
        return properties

    def autoprops_enabled(self):
        return (self.config.has_option('miscellany', 'enable-auto-props')
        and self.config.getboolean( 'miscellany', 'enable-auto-props')
        and self.config.has_section('auto-props'))


def config_file_path(config_dir):
    if config_dir == None:
        global _svn_config_dir
        config_dir = _svn_config_dir
    if config_dir == None:
        if sys.platform == 'win32':
            config_dir = os.path.join(os.environ['APPDATA'], 'Subversion')
        else:
            config_dir = os.path.join(os.environ['HOME'], '.subversion')
    return os.path.join(config_dir, 'config')


def parse_autoprops(prop_list):
    """Parses a string of autoprops and returns a dictionary of
       the results.
       Emulates the parsing of core.auto_props_enumerator.
    """
    def unquote(s):
        if len(s)>1 and s[0] in ['"', "'"] and s[0]==s[-1]:
            return s[1:-1]
        return s

    properties = {}
    for prop in prop_list.split(';'):
        if '=' in prop:
            prop, value = prop.split('=',1)
            value = unquote(value.strip())
        else:
            value = ''
        properties[prop.strip()] = value
    return properties

class SimpleStringIO(object):
    """SimpleStringIO can replace a StringIO in write mode.

    cStringIO reallocates and doubles the size of its internal buffer
    when it needs to append new data which requires two large blocks for
    large inputs. SimpleStringIO stores each individual blocks and joins
    them once done. This might cause more memory fragmentation but
    requires only one large block. In practice, ra.get_file() seems to
    write in 16kB blocks (svn 1.7.5) which should be friendly to memory
    allocators.
    """
    def __init__(self, closing=True):
        self._blocks = []
        self._closing = closing

    def write(self, s):
        self._blocks.append(s)

    def getvalue(self):
        return ''.join(self._blocks)

    def close(self):
        if self._closing:
            del self._blocks
