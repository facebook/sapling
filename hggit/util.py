"""Compatibility functions for old Mercurial versions and other utility
functions."""
from dulwich import errors
from mercurial import util as hgutil
try:
    from collections import OrderedDict
except ImportError:
    from ordereddict import OrderedDict

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
