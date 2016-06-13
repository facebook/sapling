import compathacks
import errno
import re
import os
import urllib
import json
import gc

from mercurial import cmdutil
from mercurial import error
from mercurial import hg
from mercurial import node
from mercurial import repair
from mercurial import util as hgutil

try:
    from collections import deque
except:
    from mercurial.util import deque

try:
    from mercurial import revset
except ImportError:
    pass

import maps

ignoredfiles = set(['.hgtags', '.hgsvnexternals', '.hgsub', '.hgsubstate'])

b_re = re.compile(r'^\+\+\+ b\/([^\n]*)', re.MULTILINE)
a_re = re.compile(r'^--- a\/([^\n]*)', re.MULTILINE)
devnull_re = re.compile(r'^([-+]{3}) /dev/null', re.MULTILINE)
header_re = re.compile(r'^diff --git .* b\/(.*)', re.MULTILINE)
newfile_devnull_re = re.compile(r'^--- /dev/null\n\+\+\+ b/([^\n]*)',
                                re.MULTILINE)


def formatrev(rev):
    if rev == -1:
        return '\t(working copy)'
    return '\t(revision %d)' % rev

def configpath(ui, name):
    path = ui.config('hgsubversion', name)
    return path and hgutil.expandpath(path)

def fileproperty(fname, pathfunc, default=None,
                 serializer=str, deserializer=str):
    """define a property that is backed by a file"""
    def fget(self):
        if not hgutil.safehasattr(self, fname):
            path = pathfunc(self)
            if os.path.exists(path):
                with open(path, 'r') as f:
                    setattr(self, fname, deserializer(f.read()))
            else:
                setattr(self, fname, default)
        return getattr(self, fname)

    def fset(self, value):
        setattr(self, fname, value)
        path = pathfunc(self)
        with open(path, 'w') as f:
            f.write(serializer(value))

    return property(fget, fset)

def filterdiff(diff, oldrev, newrev):
    diff = newfile_devnull_re.sub(r'--- \1\t(revision 0)' '\n'
                                  r'+++ \1\t(working copy)',
                                  diff)
    oldrev = formatrev(oldrev)
    newrev = formatrev(newrev)
    diff = a_re.sub(r'--- \1' + oldrev, diff)
    diff = b_re.sub(r'+++ \1' + newrev, diff)
    diff = devnull_re.sub(r'\1 /dev/null\t(working copy)', diff)
    diff = header_re.sub(r'Index: \1' + '\n' + ('=' * 67), diff)
    return diff

def gcdisable(orig):
    """decorator to disable GC for a function or method"""
    def wrapper(*args, **kwargs):
        enabled = gc.isenabled()
        if enabled:
            gc.disable()
        try:
            orig(*args, **kwargs)
        finally:
            if enabled:
                gc.enable()
    return wrapper

def parentrev(ui, repo, meta, hashes):
    """Find the svn parent revision of the repo's dirstate.
    """
    workingctx = repo[None].parents()[0]
    outrev = outgoing_revisions(repo, hashes, workingctx.node())
    if outrev:
        workingctx = repo[outrev[-1]].parents()[0]
    return workingctx


def islocalrepo(url):
    path = str(url) # convert once up front
    if path.startswith('file:///'):
        prefixlen = len('file://')
    elif path.startswith('file:/'):
        prefixlen = len('file:')
    else:
        return False
    if '#' in path.split('/')[-1]: # strip off #anchor
        path = path[:path.rfind('#')]
    path = url[prefixlen:]
    path = urllib.url2pathname(path).replace(os.sep, '/')
    while '/' in path:
        if reduce(lambda x, y: x and y,
                  map(lambda p: os.path.exists(os.path.join(path, p)),
                      ('hooks', 'format', 'db',))):
            return True
        path = path.rsplit('/', 1)[0]
    return False

def strip(ui, repo, changesets, *args , **opts):
    try:
        repair.strip(ui, repo, changesets, *args, **opts)
    except TypeError:
        # only 2.1.2 and later allow strip to take a list of nodes
        for changeset in changesets:
            repair.strip(ui, repo, changeset, *args, **opts)


def version(ui):
    """Return version information if available."""
    try:
        import __version__
        return __version__.version
    except ImportError:
        try:
            dn = os.path.dirname
            repo = hg.repository(ui, dn(dn(__file__)))
            ver = repo.dirstate.parents()[0]
            return node.hex(ver)[:12]
        except:
            return 'unknown'


def normalize_url(url):
    if not url:
        return url
    if url.startswith('svn+http://') or url.startswith('svn+https://'):
        url = url[4:]
    url, revs, checkout = parseurl(url)
    url = url.rstrip('/')
    if checkout:
        url = '%s#%s' % (url, checkout)
    return url

def _scrub(data):
    if not data and not isinstance(data, list):
        return ''
    return data

def _descrub(data):
    if isinstance(data, list):
        return tuple(data)
    if data == '':
        return None
    return data

def _convert(input, visitor):
    if isinstance(input, dict):
        scrubbed = {}
        d = dict([(_convert(key, visitor), _convert(value, visitor))
                  for key, value in input.iteritems()])
        for key, val in d.iteritems():
            scrubbed[visitor(key)] = visitor(val)
        return scrubbed
    elif isinstance(input, list):
        return [_convert(element, visitor) for element in input]
    elif isinstance(input, unicode):
        return input.encode('utf-8')
    return input

def dump(data, file_path):
    """Serialize some data to a path atomically.

    This is present because I kept corrupting my revmap by managing to hit ^C
    during the serialization of that file.
    """
    f = hgutil.atomictempfile(file_path, 'w+b', 0644)
    json.dump(_convert(data, _scrub), f)
    f.close()

def load(file_path, default=None, resave=True):
    """Deserialize some data from a path.
    """
    data = default
    if not os.path.exists(file_path):
        return data

    f = open(file_path)
    try:
        data = _convert(json.load(f), _descrub)
        f.close()
    except ValueError:
        try:
            # Ok, JSON couldn't be loaded, so we'll try the old way of using pickle
            data = compathacks.pickle_load(f)
        except:
            # well, pickle didn't work either, so we reset the file pointer and
            # read the string
            f.seek(0)
            data = f.read()

        # convert the file to json immediately
        f.close()
        if resave:
            dump(data, file_path)
    return data

def parseurl(url, heads=[]):
    checkout = None
    svn_url, (_junk, heads) = hg.parseurl(url, heads)
    if heads:
        checkout = heads[0]
    return svn_url, heads, checkout


class PrefixMatch(object):
    def __init__(self, prefix):
        self.p = prefix

    def files(self):
        return []

    def __call__(self, fn):
        return fn.startswith(self.p)

    def bad(self, f, msg):
        pass

    def always(self):
        return False

    def isexact(self):
        return False

    def anypats(self):
        return True

    def prefix(self):
        return False

def outgoing_revisions(repo, reverse_map, sourcerev):
    """Given a repo and an hg_editor, determines outgoing revisions for the
    current working copy state.
    """
    outgoing_rev_hashes = []
    if sourcerev in reverse_map:
        return
    sourcerev = repo[sourcerev]
    while (not sourcerev.node() in reverse_map
           and sourcerev.node() != node.nullid):
        outgoing_rev_hashes.append(sourcerev.node())
        sourcerev = sourcerev.parents()
        if len(sourcerev) != 1:
            raise hgutil.Abort("Sorry, can't find svn parent of a merge revision.")
        sourcerev = sourcerev[0]
    if sourcerev.node() != node.nullid:
        return outgoing_rev_hashes

def outgoing_common_and_heads(repo, reverse_map, sourcerev):
    """Given a repo and an hg_editor, determines outgoing revisions for the
    current working copy state. Returns a tuple (common, heads) like
    discovery.findcommonoutgoing does.
    """
    if sourcerev in reverse_map:
        return ([sourcerev], [sourcerev]) # nothing outgoing
    sourcecx = repo[sourcerev]
    while (not sourcecx.node() in reverse_map
           and sourcecx.node() != node.nullid):
        ps = sourcecx.parents()
        if len(ps) != 1:
            raise hgutil.Abort("Sorry, can't find svn parent of a merge revision.")
        sourcecx = ps[0]
    if sourcecx.node() != node.nullid:
        return ([sourcecx.node()], [sourcerev])
    return ([sourcerev], [sourcerev]) # nothing outgoing

def describe_commit(ui, h, b):
    ui.note(' committed to "%s" as %s\n' % ((b or 'default'), node.short(h)))


def swap_out_encoding(new_encoding="UTF-8"):
    from mercurial import encoding
    old = encoding.encoding
    encoding.encoding = new_encoding
    return old

def isancestor(ctx, ancestorctx):
    """Return True if ancestorctx is equal or an ancestor of ctx."""
    if ctx == ancestorctx:
        return True
    for actx in ctx.ancestors():
        if actx == ancestorctx:
            return True
    return False

def issamefile(parentctx, childctx, f):
    """Return True if f exists and is the same in childctx and parentctx"""
    if f not in parentctx or f not in childctx:
        return False
    if parentctx == childctx:
        return True
    if parentctx.rev() > childctx.rev():
        parentctx, childctx = childctx, parentctx

    def selfandancestors(selfctx):
        yield selfctx
        for ctx in selfctx.ancestors():
            yield ctx

    for pctx in selfandancestors(childctx):
        if pctx.rev() <= parentctx.rev():
            return True
        if f in pctx.files():
            return False
    # parentctx is not an ancestor of childctx, files are unrelated
    return False

def getsvnrev(ctx, defval=None):
    '''Extract SVN revision from commit metadata'''
    return ctx.extra().get('convert_revision', defval)

def revset_fromsvn(repo, subset, x):
    '''``fromsvn()``
    Select changesets that originate from Subversion.
    '''
    args = revset.getargs(x, 0, 0, "fromsvn takes no arguments")

    rev = repo.changelog.rev
    bin = node.bin
    meta = repo.svnmeta(skiperrorcheck=True)
    if not meta.revmapexists:
        raise hgutil.Abort("svn metadata is missing - "
                           "run 'hg svn rebuildmeta' to reconstruct it")
    svnrevs = set(rev(h) for h in meta.revmap.hashes().keys())
    return filter(svnrevs.__contains__, subset)

def revset_svnrev(repo, subset, x):
    '''``svnrev(number)``
    Select changesets that originate in the given Subversion revision.
    '''
    args = revset.getargs(x, 1, 1, "svnrev takes one argument")

    rev = revset.getstring(args[0],
                           "the argument to svnrev() must be a number")
    try:
        revnum = int(rev)
    except ValueError:
        raise error.ParseError("the argument to svnrev() must be a number")

    meta = repo.svnmeta(skiperrorcheck=True)
    if not meta.revmapexists:
        raise hgutil.Abort("svn metadata is missing - "
                           "run 'hg svn rebuildmeta' to reconstruct it")
    revs = []
    for n in meta.revmap.revhashes(revnum):
        r = repo[n].rev()
        if r in subset:
            revs.append(r)
    return revs

revsets = {
    'fromsvn': revset_fromsvn,
    'svnrev': revset_svnrev,
}

def revset_stringset(orig, repo, subset, x):
    if x.startswith('r') and x[1:].isdigit():
        return revset_svnrev(repo, subset, ('string', x[1:]))
    return orig(repo, subset, x)

def getfilestoresize(ui):
    """Return the replay or stupid file memory store size in megabytes or -1"""
    size = ui.configint('hgsubversion', 'filestoresize', 200)
    if size >= 0:
        size = size*(2**20)
    else:
        size = -1
    return size

def parse_revnum(svnrepo, r):
    try:
        return int(r or 0)
    except ValueError:
        if isinstance(r, str) and r.lower() in ('head', 'tip'):
            return svnrepo.last_changed_rev
        else:
            raise error.RepoLookupError("unknown Subversion revision %r" % r)
