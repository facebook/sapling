import re
import os
import urllib

from mercurial import cmdutil
from mercurial import error
from mercurial import hg
from mercurial import node
from mercurial import util as hgutil

try:
    from mercurial import revset
except ImportError:
    pass

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


def parentrev(ui, repo, meta, hashes):
    """Find the svn parent revision of the repo's dirstate.
    """
    workingctx = repo.parents()[0]
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


def load_string(file_path, default=None, limit=1024):
    if not os.path.exists(file_path):
        return default
    try:
        f = open(file_path, 'r')
        ret = f.read(limit)
        f.close()
    except:
        return default
    if ret == '':
        return default
    return ret


def save_string(file_path, string):
    if string is None:
        string = ""
    f = open(file_path, 'wb')
    f.write(str(string))
    f.close()


# TODO remove when we drop 1.3 support
def progress(ui, *args, **kwargs):
    if getattr(ui, 'progress', False):
        return ui.progress(*args, **kwargs)

# TODO remove when we drop 1.5 support
remoteui = getattr(cmdutil, 'remoteui', getattr(hg, 'remoteui', False))
if not remoteui:
    raise ImportError('Failed to import remoteui')

def parseurl(url, heads=[]):
    parsed = hg.parseurl(url, heads)
    if len(parsed) == 3:
        # old hg, remove when we can be 1.5-only
        svn_url, heads, checkout = parsed
    else:
        svn_url, heads = parsed
        if isinstance(heads, tuple) and len(heads) == 2:
            # hg 1.6 or later
            _junk, heads = heads
        if heads:
            checkout = heads[0]
        else:
            checkout = None
    return svn_url, heads, checkout


class PrefixMatch(object):
    def __init__(self, prefix):
        self.p = prefix

    def files(self):
        return []

    def __call__(self, fn):
        return fn.startswith(self.p)

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

def default_commit_msg(ui):
    return ui.config('hgsubversion', 'defaultmessage', '')

def describe_commit(ui, h, b):
    ui.note(' committed to "%s" as %s\n' % ((b or 'default'), node.short(h)))


def swap_out_encoding(new_encoding="UTF-8"):
    from mercurial import encoding
    old = encoding.encoding
    encoding.encoding = new_encoding
    return old


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

def _templatehelper(ctx, kw):
    '''
    Helper function for displaying information about converted changesets.
    '''
    convertinfo = ctx.extra().get('convert_revision', '')

    if not convertinfo or not convertinfo.startswith('svn:'):
        return ''

    if kw == 'svnuuid':
        return convertinfo[4:40]
    elif kw == 'svnpath':
        return convertinfo[40:].rsplit('@', 1)[0]
    elif kw == 'svnrev':
        return convertinfo[40:].rsplit('@', 1)[-1]
    else:
        raise hgutil.Abort('unrecognized hgsubversion keyword %s' % kw)

def svnrevkw(**args):
    """:svnrev: String. Converted subversion revision number."""
    return _templatehelper(args['ctx'], 'svnrev')

def svnpathkw(**args):
    """:svnpath: String. Converted subversion revision project path."""
    return _templatehelper(args['ctx'], 'svnpath')

def svnuuidkw(**args):
    """:svnuuid: String. Converted subversion revision repository identifier."""
    return _templatehelper(args['ctx'], 'svnuuid')

templatekeywords = {
    'svnrev': svnrevkw,
    'svnpath': svnpathkw,
    'svnuuid': svnuuidkw,
}

def revset_fromsvn(repo, subset, x):
    '''``fromsvn()``
    Select changesets that originate from Subversion.
    '''
    args = revset.getargs(x, 0, 0, "fromsvn takes no arguments")

    def matches(r):
        convertinfo = repo[r].extra().get('convert_revision', '')
        return convertinfo[:4] == 'svn:'

    return [r for r in subset if matches(r)]

def revset_svnrev(repo, subset, x):
    '''``svnrev(number)``
    Select changesets that originate in the given Subversion revision.
    '''
    args = revset.getargs(x, 1, 1, "svnrev takes one argument")

    rev = revset.getstring(args[0],
                           "the argument to svnrev() must be a number")
    try:
        rev = int(rev)
    except ValueError:
        raise error.ParseError("the argument to svnrev() must be a number")

    def matches(r):
        convertinfo = repo[r].extra().get('convert_revision', '')
        if convertinfo[:4] != 'svn:':
            return False
        return int(convertinfo[40:].rsplit('@', 1)[-1]) == rev

    return [r for r in subset if matches(r)]

revsets = {
    'fromsvn': revset_fromsvn,
    'svnrev': revset_svnrev,
}
