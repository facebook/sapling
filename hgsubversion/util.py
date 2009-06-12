import os

from mercurial import hg
from mercurial import node
from mercurial import util as hgutil


def getuserpass(opts):
    # DO NOT default the user to hg's getuser(). If you provide
    # *any* default username to Subversion, it won't use any remembered
    # username for the desired realm, breaking OS X Keychain support,
    # GNOME keyring support, and all similar tools.
    return opts.get('username', None), opts.get('password', '')


def version(ui):
    """Guess the version of hgsubversion.
    """
    # TODO make this say something other than "unknown" for installed hgsubversion
    dn = os.path.dirname
    repo = hg.repository(ui, dn(dn(__file__)))
    ver = repo.dirstate.parents()[0]
    return node.hex(ver)[:12]


def normalize_url(svnurl):
    url, revs, checkout = hg.parseurl(svnurl)
    url = url.rstrip('/')
    if checkout:
        url = '%s#%s' % (url, checkout)
    return url


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

def is_svn_repo(repo):
    return os.path.exists(os.path.join(repo.path, 'svn', 'uuid'))

default_commit_msg = '*** empty log message ***'

def describe_revision(ui, r):
    try:
        msg = [s for s in map(str.strip, r.message.splitlines()) if s][0]
    except:
        msg = default_commit_msg

    ui.status(('[r%d] %s: %s' % (r.revnum, r.author, msg))[:80] + '\n')

def describe_commit(ui, h, b):
    ui.note(' committed to "%s" as %s\n' % ((b or 'default'), node.short(h)))


def swap_out_encoding(new_encoding="UTF-8"):
    """ Utility for mercurial incompatibility changes, can be removed after 1.3
    """
    from mercurial import encoding
    old = encoding.encoding
    encoding.encoding = new_encoding
    return old


def aresamefiles(parentctx, childctx, files):
    """Assuming all files exist in childctx and parentctx, return True
    if none of them was changed in-between.
    """
    if parentctx == childctx:
        return True
    if parentctx.rev() > childctx.rev():
        parentctx, childctx = childctx, parentctx

    def selfandancestors(selfctx):
        yield selfctx
        for ctx in selfctx.ancestors():
            yield ctx

    files = dict.fromkeys(files)
    for pctx in selfandancestors(childctx):
        if pctx.rev() <= parentctx.rev():
            return True
        for f in pctx.files():
            if f in files:
                return False
    # parentctx is not an ancestor of childctx, files are unrelated
    return False
