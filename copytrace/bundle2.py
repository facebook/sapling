# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import bundle2, util, exchange, hg, error
from mercurial.i18n import _
import dbutil, error


# Temporarily used to force to load the module
def _pullbundle2extraprepare(orig, pullop, kwargs):
    return orig(pullop, kwargs)


def pullmoves(repo, nodelist, source="default"):
    """
    Fetch move data from the server
    """
    source, branches = hg.parseurl(repo.ui.expandpath(source))
    # No default server defined
    try:
        remote = hg.peer(repo, {}, source)
    except Exception:
        return
    repo.ui.status(_('pulling move data from %s\n') % util.hidepassword(source))
    pullop = exchange.pulloperation(repo, remote, nodelist, False)
    lock = pullop.repo.lock()
    try:
        pullop.trmanager = exchange.transactionmanager(repo, 'pull',
                                                       remote.url())
        _pullmovesbundle2(pullop)
        pullop.trmanager.close()
    finally:
        pullop.trmanager.release()
        lock.release()


def _pullmovesbundle2(pullop):
    """
    Pull move data
    """
    kwargs = {}
    kwargs['bundlecaps'] = exchange.caps20to10(pullop.repo)
    kwargs['movedatareq'] = pullop.heads
    kwargs['common'] = pullop.heads
    kwargs['heads'] = pullop.heads
    kwargs['cg'] = False
    bundle = pullop.remote.getbundle('pull', **kwargs)
    try:
        op = bundle2.processbundle(pullop.repo, bundle, pullop.gettransaction)
    except error.BundleValueError as exc:
        raise error.Abort('missing support for %s' % exc)


@exchange.b2partsgenerator('push:movedata')
def _pushb2movedata(pushop, bundler):
    """
    add parts containing the movedata when pushing new commits -- client-side
    """
    repo = pushop.repo
    ctxlist = _processctxlist(repo, pushop.remoteheads, pushop.revs)
    if ctxlist:
        try:
            dic = dbutil.retrieverawdata(repo, ctxlist)
        except Exception as e:
            _fail(repo, e, "_pushb2movedata")
        data = _encodedict(dic)
        repo.ui.status('moves for %d changesets pushed\n' % len(dic.keys()))

        part = bundler.newpart('push:movedata', data=data)


@bundle2.parthandler('push:movedata')
def _handlemovedatarequest(op, inpart):
    """
    process a movedata push -- server-side
    """
    dic = _decodedict(inpart)
    op.records.add('movedata', {'mvdict': dic})
    try:
        dbutil.insertrawdata(op.repo, dic)
    except Exception as e:
        error.logfailure(op.repo, e, "_handlemovedatarequest-push")


@exchange.getbundle2partsgenerator('pull:movedata')
def _getbundlemovedata(bundler, repo, source, bundlecaps=None, heads=None,
                       common=None,  b2caps=None, **kwargs):
    """
    add parts containing the movedata requested to the bundle -- server-side
    """
    ctxlist = [repo[node].hex() for node in kwargs.get('movedatareq', [])]
    ctxlist.extend(_processctxlist(repo, common, heads))
    if ctxlist:
        try:
            dic = dbutil.retrieverawdata(repo, ctxlist)
        except Exception as e:
            _fail(repo, e, "_getbundlemovedata")
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
    try:
        dbutil.insertrawdata(op.repo, dic)
    except Exception as e:
        error.logfailure(op.repo, e, "_handlemovedatarequest-pull")

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
