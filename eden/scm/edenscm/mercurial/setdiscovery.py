# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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

import random

from edenscm import tracing

from . import error, progress, util
from .eagerpeer import unwrap
from .i18n import _
from .node import bin, nullid


def _limitsample(sample, desiredlen):
    """return a random subset of sample of at most desiredlen item"""
    if util.istest():
        # Stabilize test across Python 2 / Python 3.
        return set(sorted(sample)[:desiredlen])
    if len(sample) > desiredlen:
        sample = set(random.sample(list(sample), desiredlen))
    return sample


def findcommonheads(
    ui,
    local,
    remote,
    initialsamplesize=None,
    fullsamplesize=None,
    abortwhenunrelated=True,
    ancestorsof=None,
    explicitremoteheads=None,
):
    """Return a tuple (commonheads, anyincoming, remoteheads) used to
    identify missing nodes from or in remote.

    Read the module-level docstring for important concepts: 'common',
    'missing', and 'unknown'.

    To (greatly) reduce round-trips, setting 'ancestorsof' is necessary.
    - Push: Figure out what to push exactly, and pass 'ancestorsof' as the
      heads of them. If it's 'push -r .', 'ancestorsof' should be just the
      commit hash of '.'.
    - Pull: Figure out what remote names to pull (ex. selectivepull), pass the
      current local commit hashes of those bookmark as 'ancestorsof'.

    Parameters:
    - abortwhenunrelated: aborts if 'common' is empty.
    - ancestorsof: heads (in nodes) to consider. 'unknown' is initially
      '::ancestorsof'.
    - explicitremoteheads: if not None, a list of nodes that are known existed
      on the remote server.

    Return values:
    - 'anyincoming' is a boolean. Its usefulness is questionable.
    - 'localheads % commonheads' (in nodes) defines what is unique in the local
       repo.  'localheads' is not returned, but can be calculated via 'local'.
    - 'remoteheads % commonheads' (in nodes) defines what is unique in the
      remote repo. 'remoteheads' might include commit hashes unknown to the
      local repo.
    """
    if initialsamplesize is None:
        initialsamplesize = max(ui.configint("discovery", "initial-sample-size"), 1)
    if fullsamplesize is None:
        fullsamplesize = max(ui.configint("discovery", "full-sample-size"), 1)
    return _findcommonheadsnew(
        ui,
        local,
        remote,
        initialsamplesize,
        fullsamplesize,
        abortwhenunrelated,
        ancestorsof,
        explicitremoteheads,
    )


def _findcommonheadsnew(
    ui,
    local,
    remote,
    initialsamplesize=100,
    fullsamplesize=200,
    abortwhenunrelated=True,
    ancestorsof=None,
    explicitremoteheads=None,
):
    """New implementation that does not depend on dagutil.py or ancestor.py,
    for easy Rust migration.

    Read the module-level docstring for important concepts: 'common',
    'missing', and 'unknown'.

    Variable names:
    - 'local' prefix: from local
    - 'remote' prefix: from remote, maybe unknown by local
    - 'sample': from local, to be tested by remote
    - 'common' prefix: known by local, known by remote
    - 'unknown' prefix: known by local, maybe unknown by remote
      (unknown means we don't know if it's known by remote or not yet)
    - 'missing' prefix: known by local, unknown by remote

    This function uses binary commit hashes and avoids revision numbers if
    possible. It's not efficient with the revlog backend (correctness first)
    but the Rust DAG will make it possible to be efficient.
    """
    cl = local.changelog
    dag = cl.dag
    start = util.timer()

    isselectivepull = local.ui.configbool(
        "remotenames", "selectivepull"
    ) and local.ui.configbool("remotenames", "selectivepulldiscovery")

    if ancestorsof is None:
        if isselectivepull:
            # With selectivepull, limit heads for discovery for both local and
            # remote repo - no invisible heads for the local repo.
            localheads = local.heads()
            if cl.algorithmbackend == "segments":
                localheads = list(set(localheads) | set(dag.heads(dag.mastergroup())))
        else:
            localheads = list(dag.headsancestors(dag.all()))
    else:
        localheads = ancestorsof

    # localheads can be empty in special case: after initial streamclone,
    # because both remotenames and visible heads are empty. Ensure 'tip' is
    # part of 'localheads' so we don't pull the entire repo.
    # TODO: Improve clone protocol so streamclone transfers remote names.
    if not localheads:
        localheads = [local["tip"].node()]

    # Filter out 'nullid' immediately.
    localheads = sorted(h for h in localheads if h != nullid)
    unknown = set()
    commonheads = set()

    def sampleunknownboundary(size):
        if not commonheads:
            # Avoid calculating heads(unknown) + roots(unknown) as it can be
            # quite expensive if 'unknown' is large (when there are no common
            # heads).
            # TODO: Revisit this after segmented changelog, which makes it
            # much cheaper.
            return []
        boundary = set(local.nodes("heads(%ln) + roots(%ln)", unknown, unknown))
        picked = _limitsample(boundary, size)
        if boundary:
            ui.debug(
                "sampling from both directions (%d of %d)\n"
                % (len(picked), len(boundary))
            )
        return list(picked)

    def sampleunknownrandom(size):
        size = min(size, len(unknown))
        ui.debug("sampling undecided commits (%d of %d)\n" % (size, len(unknown)))
        return list(_limitsample(unknown, size))

    def samplemultiple(funcs, size):
        """Call multiple sample functions, up to limited size"""
        sample = set()
        for func in funcs:
            picked = func(size - len(sample))
            assert len(picked) <= size
            sample.update(picked)
            if len(sample) >= size:
                break
        return sorted(sample)

    def httpcommitlookup(repo, sample):
        knownresponse = local.edenapi.commitknown(sample)
        commonsample = set()
        for res in knownresponse:
            tracing.debug(
                "edenapi commitknown: %s" % str(res),
                target="exchange::httpcommitlookup",
            )
            if unwrap(res["known"]):
                commonsample.add(res["hgid"])
        return commonsample

    def httpenabled():
        return (
            isselectivepull
            and ui.configbool("pull", "httpbookmarks")
            and ui.configbool("exchange", "httpcommitlookup")
            and local.nullableedenapi is not None
        )

    from .bookmarks import remotenameforurl, selectivepullbookmarknames

    sample = set(_limitsample(localheads, initialsamplesize))
    remotename = remotenameforurl(ui, remote.url())  # ex. 'default' or 'remote'
    selected = list(selectivepullbookmarknames(local, remotename))

    # Include names (public heads) that the server might have in sample.
    # This can efficiently extend the "common" set, if the server does
    # have them.
    for name in selected:
        if name in local:
            node = local[name].node()
            if node not in sample:
                sample.add(node)

    # Drop nullid special case.
    sample.discard(nullid)
    sample = sorted(sample)

    ui.debug("query 1; heads\n")
    batch = remote.iterbatch()
    commonsample = set()

    if httpenabled():
        fetchedbookmarks = local.edenapi.bookmarks(list(selected))
        remoteheads = {bm: n for (bm, n) in fetchedbookmarks.items() if n is not None}
        commonsample = httpcommitlookup(local, sample)
    else:
        if isselectivepull:
            # With selectivepull, limit heads for discovery for both local and
            # remote repo - only list selected heads on remote.
            # Return type: sorteddict[name: str, hex: str].
            batch.listkeyspatterns("bookmarks", patterns=selected)
        else:
            # Legacy pull: list all heads on remote.
            # Return type: List[node: bytes].
            batch.heads()
        batch.known(sample)
        batch.submit()
        remoteheads, remotehassample = batch.results()
        commonsample = {n for n, known in zip(sample, remotehassample) if known}

    # If the server has no selected names (ex. master), fallback to fetch all
    # heads.
    #
    # Note: This behavior is not needed for production use-cases. However, many
    # tests setup the server repo without a "master" bookmark. They need the
    # fallback path to not error out like "repository is unrelated" (details
    # in the note below).
    if not remoteheads and isselectivepull:
        isselectivepull = False
        remoteheads = remote.heads()

    # Normalize 'remoteheads' to Set[node].
    if isselectivepull:
        remoteheads = set(bin(h) for h in remoteheads.values())
    else:
        remoteheads = set(remoteheads)

    # Unconditionally include 'explicitremoteheads', if selectivepull is used.
    #
    # Without selectivepull, the "remoteheads" should already contain all the
    # heads and there is no need to consider explicitremoteheads.
    #
    # Note: It's actually a bit more complicated with non-Mononoke infinitepush
    # branches - those heads are not visible via "remote.heads()". There are
    # tests relying on scratch heads _not_ visible in "remote.heads()" to
    # return early (both commonheads and remoteheads are empty) and not error
    # out like "repository is unrelated".
    if explicitremoteheads and isselectivepull:
        remoteheads = remoteheads.union(explicitremoteheads)
    # Remove 'nullid' that the Rust layer dislikes.
    remoteheads = sorted(h for h in remoteheads if h != nullid)

    if cl.tip() == nullid:
        # The local repo is empty. Everything is 'unknown'.
        return [], bool(remoteheads), remoteheads

    ui.status_err(_("searching for changes\n"))

    commonremoteheads = cl.filternodes(remoteheads)

    # Mononoke tests do not want this output.
    ui.debug(
        "local heads: %s; remote heads: %s (explicit: %s); initial common: %s\n"
        % (
            len(localheads),
            len(remoteheads),
            len(explicitremoteheads or ()),
            len(commonremoteheads),
        )
    )

    # fast paths

    if commonsample.issuperset(set(localheads) - {nullid}):
        ui.note(_("all local heads known remotely\n"))
        # TODO: Check how 'remoteheads' is used at upper layers, and if we
        # can avoid listing all heads remotely (which can be expensive).
        anyincoming = bool(set(remoteheads) - set(localheads))
        return localheads, anyincoming, remoteheads

    # slow path: full blown discovery

    # unknown = localheads % commonheads
    commonheads = dag.sort(commonremoteheads + list(commonsample))
    unknown = dag.only(localheads, commonheads)
    missing = dag.sort([])

    roundtrips = 1
    with progress.bar(ui, _("searching"), _("queries")) as prog:
        while len(unknown) > 0:
            # Quote from module doc: For each node that remote doesn't know,
            # move it and all its descendants to `missing`.
            missingsample = set(sample) - commonsample
            if missingsample:
                descendants = dag.range(missingsample, localheads)
                missing += descendants
                unknown -= missing

            if not unknown:
                break

            # Decide 'sample'.
            sample = samplemultiple(
                [sampleunknownboundary, sampleunknownrandom], fullsamplesize
            )

            roundtrips += 1
            progmsg = _("checking %i commits, %i left") % (
                len(sample),
                len(unknown) - len(sample),
            )
            prog.value = (roundtrips, progmsg)
            ui.debug(
                "query %i; still undecided: %i, sample size is: %i\n"
                % (roundtrips, len(unknown), len(sample))
            )
            if httpenabled():
                commonsample = httpcommitlookup(local, sample)
            else:
                remotehassample = remote.known(sample)
                commonsample = {n for n, known in zip(sample, remotehassample) if known}

            # Quote from module doc: For each node that remote knows, move it
            # and all its ancestors to `common`.
            # Don't maintain 'common' directly as it's less efficient with
            # revlog backend. Maintain 'commonheads' and 'unknown' instead.
            if commonsample:
                newcommon = dag.only(commonsample, commonheads)
                commonheads += dag.sort(commonsample)
                unknown -= newcommon

    commonheads = set(dag.headsancestors(commonheads))

    elapsed = util.timer() - start
    ui.debug("%d total queries in %.4fs\n" % (roundtrips, elapsed))
    msg = "found %d common and %d unknown server heads," " %d roundtrips in %.4fs\n"
    remoteonlyheads = set(remoteheads) - commonheads
    ui.log(
        "discovery", msg, len(commonheads), len(remoteonlyheads), roundtrips, elapsed
    )

    if not commonheads and remoteheads:
        if abortwhenunrelated:
            raise error.Abort(_("repository is unrelated"))
        else:
            ui.warn(_("warning: repository is unrelated\n"))
        return [], True, remoteheads

    anyincoming = bool(remoteonlyheads)
    return sorted(commonheads), anyincoming, remoteheads
