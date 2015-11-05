# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import bundle2, util, exchange
import dbutil
import binascii

# Temporarily used to force to load the module, will be completed later
def _pullbundle2extraprepare(orig, pullop, kwargs):
    return orig(pullop, kwargs)


@exchange.getbundle2partsgenerator('pull:movedata')
def _getbundlemovedata(bundler, repo, source, bundlecaps=None, heads=None,
                       common=None,  b2caps=None, **kwargs):
    """
    add parts containing the movedata requested to the bundle -- server-side
    """
    ctxlist = kwargs.get('movedatareq', [])
    if common:
        common = [binascii.hexlify(ctx) for ctx in common]
    else:
        common = []
    if heads:
        heads = [binascii.hexlify(ctx) for ctx in heads]
        for ctx in heads:
            while ctx and not ctx in common and ctx not in ctxlist:
                ctxlist.append(ctx)
                if repo[ctx].p2():
                    heads.append(repo[ctx].p2().hex())
                ctx = repo[ctx].p1().hex()
    if ctxlist:
        mvdict = dbutil.retrievedatapkg(repo, ctxlist, remote=True)
        ret = encodemvdict(mvdict)
        part = bundler.newpart('pull:movedata')
        part.addparam('movedata', ret)


@bundle2.parthandler('pull:movedata', ('movedata',))
def handlemovedatarequest(op, inpart):
    """
    process a movedata reply -- client-side
    """
    mvdict = decodemvdict(inpart.params['movedata'])
    op.records.add('movedata', {'mvdict': mvdict})
    dbutil.insertdatapkg(op.repo, mvdict)


def encodemvdict(mvdict):
    """
    encode the content of the move data for exchange over the wire
    """
    expandedlist = []
    for ctxhash, renames in mvdict.iteritems():
        for src, dst, mv in renames:
            expandedlist.append('%s\t%s\t%s\t%s' % (ctxhash, src, dst, mv))
    return '\n'.join(expandedlist)


def decodemvdict(data):
    """
    decode the content of the move data from exchange over the wire
    """
    result = {}
    for l in data.splitlines():
        ctxhash, src, dst, mv = l.split('\t')
        result.setdefault(ctxhash, []).append((src, dst, mv))
    return result
