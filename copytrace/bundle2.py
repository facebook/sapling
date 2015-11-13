# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import bundle2, util, exchange
import dbutil


# Temporarily used to force to load the module, will be completed later
def _pullbundle2extraprepare(orig, pullop, kwargs):
    return orig(pullop, kwargs)


@exchange.getbundle2partsgenerator('pull:movedata')
def _getbundlemovedata(bundler, repo, source, bundlecaps=None, heads=None,
                       common=None,  b2caps=None, **kwargs):
    """
    add parts containing the movedata requested to the bundle -- server-side
    """
    ctxlist = [repo[node].hex() for node in kwargs.get('movedatareq', [])]
    ctxlist.extend(_processctxlist(repo, common, heads))

    if ctxlist:
        dic = dbutil.retrieverawdata(repo, ctxlist, remote=True)
        data = _encodedict(dic)

        part = bundler.newpart('pull:movedata', data=data)


@bundle2.parthandler('pull:movedata')
def _handlemovedatarequest(op, inpart):
    """
    process a movedata reply -- client-side
    """
    dic = _decodedict(inpart)
    op.records.add('movedata', {'mvdict': dic})
    op.repo.ui.warn('moves for %d changesets retrieved\n' % len(dic.keys()))
    dbutil.insertrawdata(op.repo, dic)


def _processctxlist(repo, remoteheads, localheads):
    """
    Processes the ctx list between remoteheads and localheads
    """

    if not localheads:
        localheads = [repo[rev].node() for rev in repo.changelog.headrevs()]
    if not remoteheads:
        remoteheads = []

    return [ctx.hex() for ctx in
            repo.set("only(%ln, %ln)", localheads, remoteheads)]


def _encodedict(dic):
    """
    encode the content of the move data for exchange over the wire
    dic = {ctxhash: [(src, dst, mv)]}
    """
    expandedlist = []
    for ctxhash, mvlist in dic.iteritems():
        for src, dst, mv in mvlist:
             expandedlist.append('%s\t%s\t%s\t%s' % (ctxhash, src, dst, mv))
    return '\n'.join(expandedlist)


def _decodedict(data):
    """
    decode the content of the move data from exchange over the wire
    """
    result = {}
    for l in data.read().splitlines():
        ctxhash, src, dst, mv = l.split('\t')
        result.setdefault(ctxhash, []).append((src, dst, mv))
    return result
