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

from sapling import extensions, match as matchmod, revset, smartset
from sapling.i18n import _
from sapling.node import nullrev
from sapling.pathlog import (
    findpublic,
    is_fastlog_enabled,
    lazyparents,
    strategy_fastlog,
)
from sapling.utils import subtreeutil


class MultiPathError(ValueError):
    """Error for following multiple paths"""

    pass


def extsetup(ui) -> None:
    extensions.wrapfunction(revset, "_follow", fastlogfollow)


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

    if not is_fastlog_enabled(repo):
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
        dirs.add(path)
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

    def fastlog(repo, startrev, dirs, files):
        if len(dirs) + len(files) != 1:
            raise MultiPathError()

        path = next(iter(dirs.union(files)))
        public = findpublic(repo, startrev, path, parents)
        matched_revs = []
        for parent, path in lazyparents(startrev, path, public, parents):
            if any(subtreeutil.path_starts_with(f, path) for f in repo[parent].files()):
                matched_revs.append(parent)

        repo.ui.debug(
            "found common parent at %s with path '%s'\n" % (repo[parent].hex(), path)
        )

        # avoid duplicates, as `Fastlog` below will include it as well.
        if matched_revs and matched_revs[-1] == parent:
            matched_revs.pop()

        yield from matched_revs

        start_node = repo[parent].node()
        while True:
            log = strategy_fastlog(repo, start_node, path)
            last_rev = None
            for node in log:
                last_rev = repo.changelog.rev(node)
                yield last_rev

            # log history returned by server (via `FastLog`) does not follow copy, the following
            # logic handle the copy on Sapling side.
            if last_rev is not None:
                next_parents = list(parents(last_rev, path))
                # XXX: only handle non-merge commits
                if len(next_parents) == 1:
                    next_rev, next_path = next_parents[0]
                    # copy -- continue query server with new start_node and path
                    if next_path != path:
                        start_node = repo[next_rev].node()
                        path = next_path
                        continue
            break

    def parents(rev, path):
        # XXX: handle subtree merge

        # subtree copy
        if copy_source := subtreeutil.find_subtree_copy(repo, rev, path):
            source_commit, source_path = copy_source
            yield repo[source_commit].rev(), source_path
        else:
            ctx = repo[rev]
            # regular copy
            if r := (path in ctx and ctx[path].renamed()):
                path = r[0]
            for p in repo.changelog.parentrevs(rev):
                if p != nullrev:
                    yield p, path

    try:
        revgen = fastlog(repo, rev, dirs, files)
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


if __name__ == "__main__":
    import doctest

    doctest.testmod()
