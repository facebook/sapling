# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# fastlog.py - An extension to query remote servers for logs using scmquery / fastlog
"""
connect to scmquery servers for fast fetching of logs on files and directories.

Configure it by adding the following config options to your .hg/hgrc.
This relies on fbscmquery being setup for the repo; this should already
be configured if supported by your repo.

Config::

    [fastlog]
    enabled=true

    # Also use fastlog for files. Otherwise only use fastlog for directories.
    # (default: false)
    files=true
"""

import heapq
from collections import deque

from sapling import error, extensions, match as matchmod, phases, revset, smartset
from sapling.i18n import _
from sapling.node import bin, hex, nullrev

from .extlib.phabricator import graphql


conduit = None

FASTLOG_MAX = 100
FASTLOG_QUEUE_SIZE = 1000
FASTLOG_TIMEOUT = 50


class MultiPathError(ValueError):
    """Error for following multiple paths"""

    pass


def extsetup(ui) -> None:
    extensions.wrapfunction(revset, "_follow", fastlogfollow)


def lazyparents(rev, public, parentfunc):
    """lazyparents(rev, public)
    Lazily yield parents of rev in reverse order until all nodes
    in public have been reached or all revs have been exhausted

    10
     | \
     9  8
     |  | \
     7  6  5
     |  | /
     4 *3   First move, 4 -3
     | /
     2 *2   Second move, 4 -1
     | *
     1

    For example:
    >>> parents = { 10:[9, 8], 9:[7], 8:[6,5], 7:[4], 6:[3], 5:[3], 4:[2] }
    >>> parents.update({ 3:[2], 2:[1], 1:[] })
    >>> parentfunc = lambda k: parents[k]
    >>> public = set([1])
    >>> for p in lazyparents(10, public, parentfunc): print p,
    10 9 8 7 6 5 4 3 2 1
    >>> public = set([2,3])
    >>> for p in lazyparents(10, public, parentfunc): print p,
    10 9 8 7 6 5 4 3 2
    >>> parents[4] = [3]
    >>> public = set([3,4,5])
    >>> for p in lazyparents(10, public, parentfunc): print p,
    10 9 8 7 6 5 4 3
    >>> parents[4] = [1]
    >>> public = set([3,5,7])
    >>> for p in lazyparents(10, public, parentfunc): print p,
    10 9 8 7 6 5 4 3 2 1
    """
    seen = set()
    heap = [-rev]

    while heap:
        cur = -(heapq.heappop(heap))
        if cur not in seen:
            seen.add(cur)
            yield cur

            published = cur in public
            if published:
                # Down to one public ancestor; end generation
                if len(public) == 1:
                    return
                public.remove(cur)

            for p in parentfunc(cur):
                if p != nullrev:
                    heapq.heappush(heap, -p)
                    if published:
                        public.add(p)


def dirmatches(files, paths) -> bool:
    """dirmatches(files, paths)
    Return true if any files match directories in paths
    Expects paths to end in '/' if they are directories.

    >>> dirmatches(['holy/grail'], ['holy/'])
    True
    >>> dirmatches(['holy/grail'], ['holly/'])
    False
    >>> dirmatches(['holy/grail'], ['holy/grail'])
    True
    >>> dirmatches(['holy/grail'], ['holy/grail1'])
    False
    >>> dirmatches(['holy/grail1'], ['holy/grail'])
    False
    """
    assert paths
    for path in paths:
        if path[-1] == "/":
            for f in files:
                if f.startswith(path):
                    return True
        else:
            for f in files:
                if f == path:
                    return True
    return False


def originator(parentfunc, rev):
    """originator(repo, rev)
    Yield parents of rev from repo in reverse order
    """
    # Use set(nullrev, rev) to iterate until termination
    for p in lazyparents(rev, set([nullrev, rev]), parentfunc):
        if rev != p:
            yield p


def fastlogfollow(orig, repo, subset, x, name, followfirst: bool = False):
    if followfirst:
        # fastlog does not support followfirst=True
        repo.ui.debug("fastlog: not used because 'followfirst' is set\n")
        return orig(repo, subset, x, name, followfirst)

    args = revset.getargsdict(x, name, "file startrev")
    if "file" not in args:
        # Not interesting for fastlog case.
        repo.ui.debug("fastlog: not used because 'file' is not provided\n")
        return orig(repo, subset, x, name, followfirst)

    if "startrev" in args:
        revs = revset.getset(repo, smartset.fullreposet(repo), args["startrev"])
        it = iter(revs)
        try:
            startrev = next(it)
        except StopIteration:
            startrev = repo["."].rev()
        try:
            next(it)
            # fastlog does not support multiple startrevs
            repo.ui.debug("fastlog: not used because multiple revs are provided\n")
            return orig(repo, subset, x, name, followfirst)
        except StopIteration:
            # supported by fastlog: startrev contains a single rev
            pass
    else:
        startrev = repo["."].rev()

    reponame = repo.ui.config("fbscmquery", "reponame")
    if not reponame or not repo.ui.configbool("fastlog", "enabled"):
        repo.ui.debug("fastlog: not used because fastlog is disabled\n")
        return orig(repo, subset, x, name, followfirst)

    try:
        # Test that the GraphQL client can be constructed, to rule
        # out configuration issues like missing `.arcrc` etc.
        graphql.Client(repo=repo)
    except Exception as ex:
        repo.ui.debug(
            "fastlog: not used because graphql client cannot be constructed: %r\n" % ex
        )
        return orig(repo, subset, x, name, followfirst)

    path = revset.getstring(args["file"], _("%s expected a pattern") % name)
    if path.startswith("path:"):
        # strip "path:" prefix
        path = path[5:]

    if any(path.startswith("%s:" % prefix) for prefix in matchmod.allpatternkinds):
        # Patterns other than "path:" are not supported
        repo.ui.debug(
            "fastlog: not used because '%s:' patterns are not supported\n"
            % path.split(":", 1)[0]
        )
        return orig(repo, subset, x, name, followfirst)

    if not path or path == ".":
        # Walking the whole repo - bail on fastlog
        repo.ui.debug("fastlog: not used because walking through the entire repo\n")
        return orig(repo, subset, x, name, followfirst)

    dirs = set()
    files = set()
    wvfs = repo.wvfs

    if wvfs.isdir(path) and not wvfs.islink(path):
        dirs.add(path + "/")
    else:
        if repo.ui.configbool("fastlog", "files"):
            files.add(path)

        else:
            # bail on symlinks, and also bail on files for now
            # with follow behavior, for files, we are supposed
            # to track copies / renames, but it isn't convenient
            # to do this through scmquery
            repo.ui.debug("fastlog: not used because %s is not a directory\n" % path)
            return orig(repo, subset, x, name, followfirst)

    rev = startrev

    parents = repo.changelog.parentrevs
    public = set()

    # Our criterion for invoking fastlog is finding a single
    # common public ancestor from the current head.  First we
    # have to walk back through drafts to find all interesting
    # public parents.  Typically this will just be one, but if
    # there are merged drafts, we may have multiple parents.
    if repo[rev].phase() == phases.public:
        public.add(rev)
    else:
        queue = deque()
        queue.append(rev)
        seen = set()
        while queue:
            cur = queue.popleft()
            if cur not in seen:
                seen.add(cur)
                if repo[cur].mutable():
                    for p in parents(cur):
                        if p != nullrev:
                            queue.append(p)
                else:
                    public.add(cur)

    def fastlog(repo, startrev, dirs, files, localmatch):
        if len(dirs) + len(files) != 1:
            raise MultiPathError()
        filefunc = repo.changelog.readfiles
        draft_revs = []
        for parent in lazyparents(startrev, public, parents):
            # Undo relevant file renames in parent so we end up
            # passing the renamee to scmquery. Note that this will not
            # work for non-linear drafts where a file does not have
            # linear rename history.
            undorenames(repo[parent], files)

            if dirmatches(filefunc(parent), dirs.union(files)):
                draft_revs.append(parent)

        repo.ui.debug("found common parent at %s\n" % repo[parent].hex())

        if len(dirs) + len(files) != 1:
            raise MultiPathError()

        path = next(iter(dirs.union(files)))
        yield from draft_revs

        start_node = repo[parent].node()
        log = FastLog(reponame, "hg", start_node, path, repo)
        for node in log.generate_nodes():
            yield repo.changelog.rev(node)

    def undorenames(ctx, files):
        """mutate files to undo any file renames in ctx"""
        renamed = []
        for f in files:
            r = f in ctx and ctx[f].renamed()
            if r:
                renamed.append((r[0], f))
        for (src, dst) in renamed:
            files.remove(dst)
            files.add(src)

    try:
        revgen = fastlog(repo, rev, dirs, files, dirmatches)
    except MultiPathError:
        repo.ui.debug("fastlog: not used for multiple paths\n")
        return orig(repo, subset, x, name, followfirst)

    fastlogset = smartset.generatorset(revgen, iterasc=False, repo=repo)
    # Make the set order match generator order.
    fastlogset.reverse()
    # Optimization: typically for "reverse(:.) & follow(path)" used by
    # "hg log". The left side is more expensive, although it has smaller
    # "weight". Make sure fastlogset is on the left side to avoid slow
    # walking through ":.".
    # Note: this code path assumes `subset.__contains__` is fast.
    if subset.isdescending():
        return fastlogset & subset
    elif subset.isascending():
        fastlogset.reverse()
        return fastlogset & subset
    return subset & fastlogset


class FastLog:
    """Class which talks to a remote SCMQuery

    We page results in windows of up to FASTLOG_MAX to avoid generating
    too many results; this has been optimized on the server to cache
    fast continuations but this assumes service stickiness.

    * reponame - repository name (str)
    * scm - scm type (str)
    * start_node - node to start logging from
    * path - path to request logs
    * repo - mercurial repository object
    """

    def __init__(self, reponame, scm, node, path, repo):
        self.reponame = reponame
        self.scm = scm
        self.start_node = node
        self.path = path
        self.repo = repo
        self.ui = repo.ui

    def gettodo(self):
        return FASTLOG_MAX

    def generate_nodes(self):
        path = self.path
        start_hex = hex(self.start_node)
        reponame = self.reponame
        skip = 0
        usemutablehistory = self.ui.configbool("fastlog", "followmutablehistory")

        while True:
            results = None
            todo = self.gettodo()
            client = graphql.Client(repo=self.repo)
            results = client.scmquery_log(
                reponame,
                self.scm,
                start_hex,
                file_paths=[path],
                skip=skip,
                number=todo,
                use_mutable_history=usemutablehistory,
                timeout=FASTLOG_TIMEOUT,
            )

            if results is None:
                raise error.Abort(_("ScmQuery fastlog returned nothing unexpectedly"))

            server_nodes = [bin(commit["hash"]) for commit in results]

            # `filternodes` has a desired side effect that fetches nodes
            # (in lazy changelog) in batch.
            nodes = self.repo.changelog.filternodes(server_nodes)
            if len(nodes) != len(server_nodes):
                missing_nodes = set(server_nodes) - set(nodes)
                self.repo.ui.status_err(
                    _("fastlog: server returned extra nodes unknown locally: %s\n")
                    % " ".join(sorted([hex(n) for n in missing_nodes]))
                )
            yield from nodes

            skip += todo
            if len(results) < todo:
                break


if __name__ == "__main__":
    import doctest

    doctest.testmod()
