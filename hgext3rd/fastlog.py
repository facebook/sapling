# fastlog.py
#
# An extension to query remote servers for logs using scmquery / fastlog
#
# Copyright 2016 Facebook, Inc.
"""
connect to scmquery servers for fast fetching of logs on files and directories.

Configure it by adding the following config options to your .hg/hgrc.
This relies on fbconduit being setup for the repo; this should already
be configured if supported by your repo.

[fastlog]
enabled=true
"""

from mercurial import (
    cmdutil,
    error,
    extensions,
    node,
    phases,
    revset,
    scmutil,
    util
)
from mercurial.i18n import _
from mercurial.node import nullrev

import os
import heapq
from threading import Thread, Event
from collections import deque

conduit = None

FASTLOG_MAX = 500
FASTLOG_QUEUE_SIZE = 1000
FASTLOG_TIMEOUT = 20

USE_FASTLOG = False

def extsetup(ui):
    global conduit
    try:
        conduit = extensions.find("fbconduit")
    except KeyError:
        from hgext3rd import fbconduit as conduit
    except ImportError:
        ui.warn(_('Unable to find fbconduit extension\n'))
        return
    if not util.safehasattr(conduit, 'conduit_config'):
        ui.warn(_('Incompatible conduit module; disabling fastlog\n'))
        return
    if not conduit.conduit_config(ui):
        ui.warn(_('No conduit host specified in config; disabling fastlog\n'))
        return

    extensions.wrapfunction(cmdutil, 'getlogrevs', getfastlogrevs)

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

def matches(filefunc, rev, paths):
    assert rev is not None
    assert paths
    files = filefunc(rev)
    for f in files:
        for path in paths:
            assert path[-1] == '/'
            if f.startswith(path):
                return True
    return False

def originator(repo, rev):
    """originator(repo, rev)
    Yield parents of rev from repo in reverse order
    """
    # Use set(nullrev, rev) to iterate until termination
    parentfunc = repo.changelog.parentrevs
    for p in lazyparents(rev, set([nullrev, rev]), parentfunc):
        if rev != p:
            yield p

def getfastlogrevs(orig, repo, pats, opts):
    blacklist = ['all', 'branch', 'rev', 'sparse']
    if any(opts.get(opt) for opt in blacklist) or not opts.get('follow'):
        return orig(repo, pats, opts)

    reponame = repo.ui.config('fbconduit', 'reponame')
    if reponame and repo.ui.configbool('fastlog', 'enabled'):
        wctx = repo[None]
        match, pats = scmutil.matchandpats(wctx, pats, opts)
        files = match.files()
        if not files or '.' in files:
            # Walking the whole repo - bail on fastlog
            return orig(repo, pats, opts)

        dirs = set()
        wvfs = repo.wvfs
        for path in files:
            if wvfs.isdir(path) and not wvfs.islink(path):
                dirs.update([path + '/'])
            else:
                # bail on symlinks, and also bail on files for now
                # with follow behavior, for files, we are supposed
                # to track copies / renames, but it isn't convenient
                # to do this through scmquery
                return orig(repo, pats, opts)

        rev = repo['.'].rev()

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

        def fastlog(repo, startrev, dirs):
            filefunc = repo.changelog.readfiles
            for p in lazyparents(startrev, public, parents):
                if matches(filefunc, p, dirs):
                    yield p
            repo.ui.debug('found common parent at %s\n' % repo[p].hex())
            for rev in combinator(repo, p, dirs):
                yield rev

        def combinator(repo, rev, dirs):
            """combinator(repo, rev, dirs)
            Make parallel local and remote queries along ancestors of
            rev along path and combine results, eliminating duplicates,
            restricting results to those which match dirs
            """
            LOCAL = 'L'
            REMOTE = 'R'
            queue = util.queue(FASTLOG_QUEUE_SIZE + 100)
            hash = repo[rev].hex()

            local = LocalIteratorThread(queue, LOCAL, originator(repo, rev))
            remote = FastLogThread(queue, REMOTE, reponame, 'hg', hash, dirs)

            # Allow debugging either remote or local path
            debug = repo.ui.config('fastlog', 'debug')
            if debug != 'local':
                repo.ui.debug('starting fastlog at %s\n' % hash)
                remote.start()
            if debug != 'remote':
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
                            repo.ui.warn(msg + '\n')
                            continue

                    if msg is None:
                        # Empty message means no more results
                        return

                    rev = None
                    if producer == LOCAL:
                        if debug:
                            repo.ui.debug('LOCAL:: %s\n' % msg)
                        rev = msg
                    elif producer == REMOTE:
                        if debug:
                            repo.ui.debug('REMOTE:: %s\n' % msg)
                        rev = repo.changelog.rev(node.bin(msg))

                    if rev is not None and rev not in seen:
                        seen.add(rev)
                        yield rev
            finally:
                local.stop()
                remote.stop()

        # Complex match - use a revset.
        complex = ['date', 'exclude', 'include', 'keyword', 'no_merges',
                    'only_merges', 'prune', 'user']
        if match.anypats() or len(dirs) > 1 or \
                any(opts.get(opt) for opt in complex):
            f = fastlog(repo, rev, dirs)
            revs = revset.generatorset(f, iterasc=False)
            revs.reverse()
            if not revs:
                return revset.baseset([]), None, None
            expr, filematcher = cmdutil._makelogrevset(repo, pats, opts, revs)
            matcher = revset.match(repo.ui, expr)
            matched = matcher(repo, revs)
            return matched, expr, filematcher
        else:
            # Simple match without revset shaves ~0.5 seconds off
            # hg log -l 100 -T ' ' on common directories.
            expr = 'fastlog(%s)' % ','.join(dirs)
            return fastlog(repo, rev, dirs), expr, None

    return orig(repo, pats, opts)


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
    * generator - a generator which creates results to put on the queue

    If an exception is thrown, error result with the message from the
    exception will be passed along the queue.  Since local results are
    not expected to generate exceptions, this terminates iteration.
    """

    def __init__(self, queue, id, generator):
        Thread.__init__(self)
        self.daemon = True
        self.queue = queue
        self.id = id
        self.generator = generator
        self._stop = Event()

    def stop(self):
        self._stop.set()

    def stopped(self):
        return self._stop.isSet()

    def run(self):
        try:
            for result in self.generator:
                self.queue.put((self.id, True, result))
                if self.stopped():
                    break
        except Exception as e:
            self.queue.put((self.id, False, str(e)))
        finally:
            self.queue.put((self.id, True, None))


class FastLogThread(Thread):
    """Class which talks to a remote SCMQuery or fastlog server

    Like the above, results are sent to a queue, and tagged with the
    id passed to this class' initializer.  Same rules for termination.

    We page results in windows of up to FASTLOG_MAX to avoid generating
    too many results; this has been optimized on the server to cache
    fast continuations but this assumes service stickiness.

    * queue - self explanatory
    * id - tag to use when sending messages
    * repo - repository name (str)
    * scm - scm type (str)
    * rev - revision to start logging from
    * paths - paths to request logs.  Due to missing implementation,
              we simply convert this to the common prefix, which
              means consumers of the queue may need to filter results
              if multiple paths are passed.
    """

    def __init__(self, queue, id, repo, scm, rev, paths):
        Thread.__init__(self)
        self.daemon = True
        self.queue = queue
        self.id = id
        self.repo = repo
        self.scm = scm
        self.rev = rev
        self.paths = [os.path.commonprefix(paths)]
        self._stop = Event()

    def stop(self):
        self._stop.set()

    def stopped(self):
        return self._stop.isSet()

    def run(self):
        skip = 0

        paths = self.paths
        rev = str(self.rev)
        repo = self.repo

        while True:
            todo = FASTLOG_MAX
            results = None
            try:
                if USE_FASTLOG:
                    results = conduit.call_conduit('fastlog.log',
                        rev = rev,
                        file_paths = paths,
                        skip = skip,
                        number = todo,
                    )
                else:
                    results = conduit.call_conduit('scmquery.log_v2',
                        repo = repo,
                        scm = self.scm,
                        rev = rev,
                        file_paths = paths,
                        skip = skip,
                        number = todo,
                    )
            except conduit.ConduitError as e:
                self.queue.put((self.id, False, str(e)))
                return
            except Exception as e:
                self.queue.put((self.id, False, str(e)))
                return

            if results is None:
                self.queue.put((self.id, False, 'Unknown error'))
                return

            for result in results:
                hash = result['hash']
                if len(hash) == 40:
                    self.queue.put((self.id, True, hash))
                else:
                    self.queue.put(
                        (self.id, False, 'Received bad hash %s' % hash))
                skip += 1

            if len(results) < todo or self.stopped():
                # signal completion
                self.queue.put((self.id, True, None))
                return


if __name__ == '__main__':
    import doctest
    doctest.testmod()
