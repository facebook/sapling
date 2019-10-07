# setdiscovery.py - improved discovery of common nodeset for mercurial
#
# Copyright 2010 Benoit Boissinot <bboissin@gmail.com>
# and Peter Arrenbrecht <peter@arrenbrecht.ch>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
Algorithm works in the following way. You have two repository: local and
remote. They both contains a DAG of changelists.

The goal of the discovery protocol is to find one set of node *common*,
the set of nodes shared by local and remote.

One of the issue with the original protocol was latency, it could
potentially require lots of roundtrips to discover that the local repo was a
subset of remote (which is a very common case, you usually have few changes
compared to upstream, while upstream probably had lots of development).

The new protocol only requires one interface for the remote repo: `known()`,
which given a set of changelists tells you if they are present in the DAG.

The algorithm then works as follow:

 - We will be using three sets, `common`, `missing`, `unknown`. Originally
 all nodes are in `unknown`.
 - Take a sample from `unknown`, call `remote.known(sample)`
   - For each node that remote knows, move it and all its ancestors to `common`
   - For each node that remote doesn't know, move it and all its descendants
   to `missing`
 - Iterate until `unknown` is empty

There are a couple optimizations, first is instead of starting with a random
sample of missing, start by sending all heads, in the case where the local
repo is a subset, you computed the answer in one round trip.

Then you can do something similar to the bisecting strategy used when
finding faulty changesets. Instead of random samples, you can try picking
nodes that will maximize the number of nodes that will be
classified with it (since all ancestors or descendants will be marked as well).
"""

from __future__ import absolute_import

import collections
import random

from . import dagutil, error, progress, util
from .i18n import _
from .node import nullid, nullrev


def _updatesample(dag, nodes, sample, quicksamplesize=0):
    """update an existing sample to match the expected size

    The sample is updated with nodes exponentially distant from each head of the
    <nodes> set. (H~1, H~2, H~4, H~8, etc).

    If a target size is specified, the sampling will stop once this size is
    reached. Otherwise sampling will happen until roots of the <nodes> set are
    reached.

    :dag: a dag object from dagutil
    :nodes:  set of nodes we want to discover (if None, assume the whole dag)
    :sample: a sample to update
    :quicksamplesize: optional target size of the sample"""
    # if nodes is empty we scan the entire graph
    if nodes:
        heads = dag.headsetofconnecteds(nodes)
    else:
        heads = dag.heads()
    dist = {}
    visit = collections.deque(heads)
    seen = set()
    factor = 1
    while visit:
        curr = visit.popleft()
        if curr in seen:
            continue
        d = dist.setdefault(curr, 1)
        if d > factor:
            factor *= 2
        if d == factor:
            sample.add(curr)
            if quicksamplesize and (len(sample) >= quicksamplesize):
                return
        seen.add(curr)
        for p in dag.parents(curr):
            if not nodes or p in nodes:
                dist.setdefault(p, d + 1)
                visit.append(p)


def _takequicksample(dag, nodes, size):
    """takes a quick sample of size <size>

    It is meant for initial sampling and focuses on querying heads and close
    ancestors of heads.

    :dag: a dag object
    :nodes: set of nodes to discover
    :size: the maximum size of the sample"""
    sample = dag.headsetofconnecteds(nodes)
    if size <= len(sample):
        return _limitsample(sample, size)
    _updatesample(dag, None, sample, quicksamplesize=size)
    return sample


def _takefullsample(dag, nodes, size):
    sample = dag.headsetofconnecteds(nodes)
    # update from heads
    _updatesample(dag, nodes, sample)
    # update from roots
    _updatesample(dag.inverse(), nodes, sample)
    assert sample
    sample = _limitsample(sample, size)
    if len(sample) < size:
        more = size - len(sample)
        sample.update(random.sample(list(nodes - sample), more))
    return sample


def _limitsample(sample, desiredlen):
    """return a random subset of sample of at most desiredlen item"""
    if len(sample) > desiredlen:
        sample = set(random.sample(sample, desiredlen))
    return sample


def fastdiscovery(ui, local, remote):
    # The normal findcommonheads implementation tries to find the exact boundary
    # between what the client has and what the server has. But normally we
    # have pretty good knowledge about what local commits already exist on the
    # server, so we can short circuit all the discovery logic by just assuming
    # the current public heads are representative of what's on the server. In the
    # worst case the data might be slightly out of sync and the server sends us
    # more data than necessary, but this should be rare.
    cl = local.changelog

    publicheads = []
    # That should be equivalent to "heads(public())" but much faster
    revs = list(local.revs("head() & public() + parents(roots(draft()))"))

    for r in revs:
        publicheads.append(local[r].node())

    bookmarks = ui.configlist("discovery", "knownserverbookmarks")
    knownbookmarksvalues = []
    for book in bookmarks:
        if book in local:
            knownbookmarksvalues.append(local[book].node())

    # If we have no remotenames, fallback to normal discovery.
    if not publicheads:
        return None

    publicheads = set(publicheads)

    # Check which remote nodes still exist on the server
    ui.status(_("searching for changes\n"))
    batch = remote.iterbatch()
    batch.heads()
    batch.known(knownbookmarksvalues)
    batch.known(publicheads)
    batch.submit()
    srvheadhashes, yesnoknownbookmarks, yesnopublicheads = batch.results()

    if knownbookmarksvalues and not any(yesnoknownbookmarks):
        ui.status(_("No known server bookmarks\n"))
        # Server doesn't known any remote bookmark. That's odd and it's better
        # to fallback to normal discovery process. Otherwise we might request
        # too many commits from the server
        return None

    common = list(n for i, n in enumerate(publicheads) if yesnopublicheads[i])
    common.extend(
        (n for i, n in enumerate(knownbookmarksvalues) if yesnoknownbookmarks[i])
    )

    # If we don't know of any server commits, fall back to legacy discovery
    if not common:
        # If this path is hit, it will print "searching for changes" twice,
        # which is weird. This should be very rare though, since it only happens
        # if the client has remote names, but none of those names exist on the
        # server (i.e. the server has been completely replaced, or stripped).
        ui.status(
            _(
                "server has changed since last pull - falling back to the "
                "default search strategy\n"
            )
        )
        return None

    ui.debug("using fastdiscovery\n")
    if cl.tip() == nullid:
        if srvheadhashes != [nullid]:
            return [nullid], True, srvheadhashes
        return ([nullid], False, [])

    # early exit if we know all the specified remote heads already
    clcontains = cl.__contains__
    srvheads = list(n for n in srvheadhashes if clcontains(n))
    if len(srvheads) == len(srvheadhashes):
        ui.debug("all remote heads known locally\n")
        return (srvheadhashes, False, srvheadhashes)

    return (common, True, srvheadhashes)


def findcommonheads(
    ui,
    local,
    remote,
    initialsamplesize=100,
    fullsamplesize=200,
    abortwhenunrelated=True,
    ancestorsof=None,
    needlargestcommonset=True,
):
    """Return a tuple (common, anyincoming, remoteheads) used to identify
    missing nodes from or in remote.
    """

    # fastdiscovery might returns *some* common set, but it might not be
    # necessary the largest common set. In some cases (e.g. during `hg push`)
    # we actually want largest common set
    if ui.configbool("discovery", "fastdiscovery") and not needlargestcommonset:
        res = fastdiscovery(ui, local, remote)
        if res is not None:
            return res

    start = util.timer()

    roundtrips = 0
    cl = local.changelog
    localsubset = None
    if ancestorsof is not None:
        rev = local.changelog.rev
        localsubset = [rev(n) for n in ancestorsof]
    dag = dagutil.revlogdag(cl, localsubset=localsubset)

    # early exit if we know all the specified remote heads already
    ui.debug("query 1; heads\n")
    roundtrips += 1
    ownheads = dag.heads()
    sample = _limitsample(ownheads, initialsamplesize)
    # indices between sample and externalized version must match
    sample = list(sample)
    batch = remote.iterbatch()
    batch.heads()
    batch.known(dag.externalizeall(sample))
    batch.submit()
    srvheadhashes, yesno = batch.results()

    if cl.tip() == nullid:
        if srvheadhashes != [nullid]:
            return [nullid], True, srvheadhashes
        return [nullid], False, []

    # start actual discovery (we note this before the next "if" for
    # compatibility reasons)
    ui.status(_("searching for changes\n"))

    srvheads = dag.internalizeall(srvheadhashes, filterunknown=True)
    if len(srvheads) == len(srvheadhashes):
        ui.debug("all remote heads known locally\n")
        return (srvheadhashes, False, srvheadhashes)

    if sample and len(ownheads) <= initialsamplesize and all(yesno):
        ui.note(_("all local heads known remotely\n"))
        ownheadhashes = dag.externalizeall(ownheads)
        return (ownheadhashes, True, srvheadhashes)

    # full blown discovery

    # own nodes I know we both know
    # treat remote heads (and maybe own heads) as a first implicit sample
    # response
    common = cl.incrementalmissingrevs(srvheads)
    commoninsample = set(n for i, n in enumerate(sample) if yesno[i])
    common.addbases(commoninsample)
    # own nodes where I don't know if remote knows them
    undecided = set(common.missingancestors(ownheads))
    # own nodes I know remote lacks
    missing = set()

    full = False
    with progress.bar(ui, _("searching"), _("queries")) as prog:
        while undecided:

            if sample:
                missinginsample = [n for i, n in enumerate(sample) if not yesno[i]]
                missing.update(dag.descendantset(missinginsample, missing))

                undecided.difference_update(missing)

            if not undecided:
                break

            if full or common.hasbases():
                if full:
                    ui.note(_("sampling from both directions\n"))
                else:
                    ui.debug("taking initial sample\n")
                samplefunc = _takefullsample
                targetsize = fullsamplesize
            else:
                # use even cheaper initial sample
                ui.debug("taking quick initial sample\n")
                samplefunc = _takequicksample
                targetsize = initialsamplesize
            if len(undecided) < targetsize:
                sample = list(undecided)
            else:
                sample = samplefunc(dag, undecided, targetsize)
                sample = _limitsample(sample, targetsize)

            roundtrips += 1
            prog.value = roundtrips
            ui.debug(
                "query %i; still undecided: %i, sample size is: %i\n"
                % (roundtrips, len(undecided), len(sample))
            )
            # indices between sample and externalized version must match
            sample = list(sample)
            yesno = remote.known(dag.externalizeall(sample))
            full = True

            if sample:
                commoninsample = set(n for i, n in enumerate(sample) if yesno[i])
                common.addbases(commoninsample)
                common.removeancestorsfrom(undecided)

    # heads(common) == heads(common.bases) since common represents common.bases
    # and all its ancestors
    result = dag.headsetofconnecteds(common.bases)
    # common.bases can include nullrev, but our contract requires us to not
    # return any heads in that case, so discard that
    result.discard(nullrev)
    elapsed = util.timer() - start
    ui.debug("%d total queries in %.4fs\n" % (roundtrips, elapsed))
    msg = "found %d common and %d unknown server heads," " %d roundtrips in %.4fs\n"
    missing = set(result) - set(srvheads)
    ui.log("discovery", msg, len(result), len(missing), roundtrips, elapsed)

    if not result and srvheadhashes != [nullid]:
        if abortwhenunrelated:
            raise error.Abort(_("repository is unrelated"))
        else:
            ui.warn(_("warning: repository is unrelated\n"))
        return ({nullid}, True, srvheadhashes)

    anyincoming = srvheadhashes != [nullid]
    return dag.externalizeall(result), anyincoming, srvheadhashes
