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

from .i18n import _
from .node import (
    nullid,
    nullrev,
)
from . import (
    dagutil,
    error,
)

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

def findcommonheads(ui, local, remote,
                    initialsamplesize=100,
                    fullsamplesize=200,
                    abortwhenunrelated=True):
    '''Return a tuple (common, anyincoming, remoteheads) used to identify
    missing nodes from or in remote.
    '''
    roundtrips = 0
    cl = local.changelog
    dag = dagutil.revlogdag(cl)

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
        return (srvheadhashes, False, srvheadhashes,)

    if sample and len(ownheads) <= initialsamplesize and all(yesno):
        ui.note(_("all local heads known remotely\n"))
        ownheadhashes = dag.externalizeall(ownheads)
        return (ownheadhashes, True, srvheadhashes,)

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
        ui.progress(_('searching'), roundtrips, unit=_('queries'))
        ui.debug("query %i; still undecided: %i, sample size is: %i\n"
                 % (roundtrips, len(undecided), len(sample)))
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
    ui.progress(_('searching'), None)
    ui.debug("%d total queries\n" % roundtrips)

    if not result and srvheadhashes != [nullid]:
        if abortwhenunrelated:
            raise error.Abort(_("repository is unrelated"))
        else:
            ui.warn(_("warning: repository is unrelated\n"))
        return (set([nullid]), True, srvheadhashes,)

    anyincoming = (srvheadhashes != [nullid])
    return dag.externalizeall(result), anyincoming, srvheadhashes
