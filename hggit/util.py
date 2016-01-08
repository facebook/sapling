"""Compatibility functions for old Mercurial versions and other utility
functions."""
import re

try:
    from collections import OrderedDict
except ImportError:
    from ordereddict import OrderedDict

from dulwich import errors
from mercurial import (
    util as hgutil,
)

gitschemes = ('git', 'git+ssh', 'git+http', 'git+https')

def parse_hgsub(lines):
    """Fills OrderedDict with hgsub file content passed as list of lines"""
    rv = OrderedDict()
    for l in lines:
        ls = l.strip()
        if not ls or ls[0] == '#':
            continue
        name, value = l.split('=', 1)
        rv[name.strip()] = value.strip()
    return rv

def serialize_hgsub(data):
    """Produces a string from OrderedDict hgsub content"""
    return ''.join(['%s = %s\n' % (n, v) for n, v in data.iteritems()])

def parse_hgsubstate(lines):
    """Fills OrderedDict with hgsubtate file content passed as list of lines"""
    rv = OrderedDict()
    for l in lines:
        ls = l.strip()
        if not ls or ls[0] == '#':
            continue
        value, name = l.split(' ', 1)
        rv[name.strip()] = value.strip()
    return rv

def serialize_hgsubstate(data):
    """Produces a string from OrderedDict hgsubstate content"""
    return ''.join(['%s %s\n' % (data[n], n) for n in sorted(data)])

def transform_notgit(f):
    '''use as a decorator around functions that call into dulwich'''
    def inner(*args, **kwargs):
        try:
            return f(*args, **kwargs)
        except errors.NotGitRepository:
            raise hgutil.Abort('not a git repository')
    return inner

def isgitsshuri(uri):
    """Method that returns True if a uri looks like git-style uri

    Tests:

    >>> print isgitsshuri('http://fqdn.com/hg')
    False
    >>> print isgitsshuri('http://fqdn.com/test.git')
    False
    >>> print isgitsshuri('git@github.com:user/repo.git')
    True
    >>> print isgitsshuri('github-123.com:user/repo.git')
    True
    >>> print isgitsshuri('git@127.0.0.1:repo.git')
    True
    >>> print isgitsshuri('git@[2001:db8::1]:repository.git')
    True
    """
    for scheme in gitschemes:
        if uri.startswith('%s://' % scheme):
            return False

    if uri.startswith('http:') or uri.startswith('https:'):
        return False

    m = re.match(r'(?:.+@)*([\[]?[\w\d\.\:\-]+[\]]?):(.*)', uri)
    if m:
        # here we're being fairly conservative about what we consider to be git
        # urls
        giturl, repopath = m.groups()
        # definitely a git repo
        if repopath.endswith('.git'):
            return True
        # use a simple regex to check if it is a fqdn regex
        fqdn_re = (r'(?=^.{4,253}$)(^((?!-)[a-zA-Z0-9-]{1,63}'
                   r'(?<!-)\.)+[a-zA-Z]{2,63}$)')
        if re.match(fqdn_re, giturl):
            return True
    return False
