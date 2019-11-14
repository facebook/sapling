# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# fastlog.py - An extension to query remote servers for logs using scmquery / fastlog
"""
connect to scmquery servers for fast fetching of logs on files and directories.

Configure it by adding the following config options to your .hg/hgrc.
This relies on fbconduit being setup for the repo; this should already
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
from threading import Event, Thread

from edenscm.mercurial import (
    changelog,
    error,
    extensions,
    match as matchmod,
    node,
    phases,
    revset,
    smartset,
    util,
)
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import nullrev


conduit = None

FASTLOG_MAX = 500
FASTLOG_QUEUE_SIZE = 1000
FASTLOG_TIMEOUT = 20


def extsetup(ui):
    global conduit
    try:
        conduit = extensions.find("fbconduit")
    except KeyError:
        from . import fbconduit as conduit
    except ImportError:
        ui.warn(_("Unable to find fbconduit extension\n"))
        return
    if not util.safehasattr(conduit, "conduit_config"):
        ui.warn(_("Incompatible conduit module; disabling fastlog\n"))
        return
    if not conduit.conduit_config(ui):
        ui.warn(_("No conduit host specified in config; disabling fastlog\n"))
        return

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
        cur = -heapq.heappop(heap)
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


def dirmatches(files, paths):
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


def fastlogfollow(orig, repo, subset, x, name, followfirst=False):
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

    reponame = repo.ui.config("fbconduit", "reponame")
    if not reponame or not repo.ui.configbool("fastlog", "enabled"):
        repo.ui.debug("fastlog: not used because fastlog is disabled\n")
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

    files = [path]
    if not files or "." in files:
        # Walking the whole repo - bail on fastlog
        repo.ui.debug("fastlog: not used because walking through the entire repo\n")
        return orig(repo, subset, x, name, followfirst)

    dirs = set()
    wvfs = repo.wvfs
    for path in files:
        if wvfs.isdir(path) and not wvfs.islink(path):
            dirs.update([path + "/"])
        else:
            if repo.ui.configbool("fastlog", "files"):
                dirs.update([path])
            else:
                # bail on symlinks, and also bail on files for now
                # with follow behavior, for files, we are supposed
                # to track copies / renames, but it isn't convenient
                # to do this through scmquery
                repo.ui.debug(
                    "fastlog: not used because %s is not a directory\n" % path
                )
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

    def fastlog(repo, startrev, dirs, localmatch):
        filefunc = repo.changelog.readfiles
        for parent in lazyparents(startrev, public, parents):
            files = filefunc(parent)
            if dirmatches(files, dirs):
                yield parent
        repo.ui.debug("found common parent at %s\n" % repo[parent].hex())
        for rev in combinator(repo, parent, dirs, localmatch):
            yield rev

    def combinator(repo, rev, dirs, localmatch):
        """combinator(repo, rev, dirs, localmatch)
        Make parallel local and remote queries along ancestors of
        rev along path and combine results, eliminating duplicates,
        restricting results to those which match dirs
        """
        LOCAL = "L"
        REMOTE = "R"
        queue = util.queue(FASTLOG_QUEUE_SIZE + 100)
        hash = repo[rev].hex()

        local = LocalIteratorThread(queue, LOCAL, rev, dirs, localmatch, repo)
        remote = FastLogThread(queue, REMOTE, reponame, "hg", hash, dirs, repo)

        # Allow debugging either remote or local path
        debug = repo.ui.config("fastlog", "debug")
        if debug != "local":
            repo.ui.debug("starting fastlog at %s\n" % hash)
            remote.start()
        if debug != "remote":
            local.start()
        seen = set([rev])

        try:
            while True:
                try:
                    producer, success, msg = queue.get(True, 3600)
                except util.empty:
                    raise error.Abort("Timeout reading log data")
                if not success:
                    if producer == LOCAL:
                        raise error.Abort(msg)
                    elif msg:
                        repo.ui.log("hgfastlog", msg)
                        continue

                if msg is None:
                    # Empty message means no more results
                    return

                rev = msg
                if debug:
                    if producer == LOCAL:
                        repo.ui.debug("LOCAL:: %s\n" % msg)
                    elif producer == REMOTE:
                        repo.ui.debug("REMOTE:: %s\n" % msg)

                if rev not in seen:
                    seen.add(rev)
                    yield rev
        finally:
            local.stop()
            remote.stop()

    revgen = fastlog(repo, rev, dirs, dirmatches)
    fastlogset = smartset.generatorset(revgen, iterasc=False)
    # Optimization: typically for "reverse(:.) & follow(path)" used by
    # "hg log". The left side is more expensive, although it has smaller
    # "weight". Make sure fastlogset is on the left side to avoid slow
    # walking through ":.".
    if subset.isdescending():
        fastlogset.reverse()
        return fastlogset & subset
    return subset & fastlogset


class readonlychangelog(object):
    def __init__(self, *args, **kwargs):
        self._changelog = changelog.changelog(*args, **kwargs)

    def parentrevs(self, rev):
        return self._changelog.parentrevs(rev)

    def readfiles(self, node):
        return self._changelog.readfiles(node)

    def rev(self, node):
        return self._changelog.rev(node)


class LocalIteratorThread(Thread):
    """Class which reads from an iterator and sends results to a queue.

    Results are sent in a tuple (tag, success, result), where tag is the
    id passed to this class' initializer, success is a bool, True for
    success, False on error, and result is the output of the iterator.

    When the iterator is finished, a poison pill is sent to the queue
    with result set to None to signal completion.

    Used to allow parallel fetching of results from both a local and
    remote source.

    * queue - self explanatory
    * id - tag to use when sending messages
    * rev - rev to start iterating at
    * dirs - directories against which to match
    * localmatch - a function to match candidate results
    * repo - mercurial repository object

    If an exception is thrown, error result with the message from the
    exception will be passed along the queue.  Since local results are
    not expected to generate exceptions, this terminates iteration.
    """

    def __init__(self, queue, id, rev, dirs, localmatch, repo):
        Thread.__init__(self)
        self.daemon = True
        self.queue = queue
        self.id = id
        self.rev = rev
        self.dirs = dirs
        self.localmatch = localmatch
        self.ui = repo.ui
        self._stop = Event()

        # Create a private instance of changelog to avoid trampling
        # internal caches of other threads
        c = readonlychangelog(repo.svfs, uiconfig=repo.ui.uiconfig())
        self.generator = originator(c.parentrevs, rev)
        self.filefunc = c.readfiles
        self.ui = repo.ui

    def stop(self):
        self._stop.set()

    def stopped(self):
        return self._stop.isSet()

    def run(self):
        generator = self.generator
        match = self.localmatch
        dirs = self.dirs
        filefunc = self.filefunc
        queue = self.queue

        try:
            for result in generator:
                if self.stopped():
                    break
                if not match or match(filefunc(result), dirs):
                    queue.put((self.id, True, result))
        except Exception as e:
            self.ui.traceback()
            queue.put((self.id, False, str(e)))
        finally:
            queue.put((self.id, True, None))


class FastLogThread(Thread):
    """Class which talks to a remote SCMQuery

    Like the above, results are sent to a queue, and tagged with the
    id passed to this class' initializer.  Same rules for termination.

    We page results in windows of up to FASTLOG_MAX to avoid generating
    too many results; this has been optimized on the server to cache
    fast continuations but this assumes service stickiness.

    * queue - self explanatory
    * id - tag to use when sending messages
    * reponame - repository name (str)
    * scm - scm type (str)
    * rev - revision to start logging from
    * paths - paths to request logs
    * repo - mercurial repository object
    """

    def __init__(self, queue, id, reponame, scm, rev, paths, repo):
        Thread.__init__(self)
        self.daemon = True
        self.queue = queue
        self.id = id
        self.reponame = reponame
        self.scm = scm
        self.rev = rev
        self.paths = list(paths)
        self.ui = repo.ui
        self.changelog = readonlychangelog(repo.svfs, uiconfig=repo.ui.uiconfig())
        self._stop = Event()
        self._paths_to_fetch = 0

    def stop(self):
        self._stop.set()

    def stopped(self):
        return self._stop.isSet()

    def finishpath(self, path):
        self._paths_to_fetch -= 1

    def gettodo(self):
        return max(FASTLOG_MAX / self._paths_to_fetch, 100)

    def generate(self, path):
        start = str(self.rev)
        reponame = self.reponame
        revfn = self.changelog.rev
        skip = 0

        while True:
            if self.stopped():
                break

            results = None
            todo = self.gettodo()
            try:
                results = conduit.call_conduit(
                    "scmquery.log_v2",
                    repo=reponame,
                    scm_type=self.scm,
                    rev=start,
                    file_paths=[path],
                    skip=skip,
                    number=todo,
                )
            except Exception as e:
                if self.ui.config("fastlog", "debug"):
                    self.ui.traceback(force=True)
                self.queue.put((self.id, False, str(e)))
                self.stop()
                return

            if results is None:
                self.queue.put((self.id, False, "Unknown error"))
                self.stop()
                return

            for result in results:
                hash = result["hash"]
                try:
                    if len(hash) != 40:
                        raise ValueError("Received invalid hash %s" % hash)
                    rev = revfn(node.bin(hash))
                    if rev is None:
                        raise KeyError("Hash %s not in local repo" % hash)
                except Exception as e:
                    if self.ui.config("fastlog", "debug"):
                        self.ui.traceback(force=True)
                    self.queue.put((self.id, False, str(e)))
                else:
                    yield rev

            skip += todo
            if len(results) < todo:
                self.finishpath(path)
                return

    def run(self):
        revs = None
        paths = self.paths

        self._paths_to_fetch = len(paths)
        for path in paths:
            g = self.generate(path)
            gen = smartset.generatorset(g, iterasc=False)
            gen.reverse()
            if revs:
                revs = smartset.addset(revs, gen, ascending=False)
            else:
                revs = gen

        for rev in revs:
            if self.stopped():
                break
            self.queue.put((self.id, True, rev))
        # The end marker (self.id, True, None) indicates that the thread
        # completed successfully. Don't send it if the thread is stopped.
        # The thread can be stopped for one of two reasons:
        #  1. The fastlog service failed - in this case, flagging a successful
        #     finish is harmful, because it will stop us continuing with local
        #     results, truncating output.
        #  2. The caller is going to ignore all future results from us. In this
        #     case, it'll ignore the end marker anyway - it's discarding the
        #     entire queue.
        if not self.stopped():
            self.queue.put((self.id, True, None))


if __name__ == "__main__":
    import doctest

    doctest.testmod()
