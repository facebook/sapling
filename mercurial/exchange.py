# exchange.py - utily to exchange data between repo.
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
from node import hex, nullid
import errno
import util, scmutil, changegroup
import discovery, phases, obsolete, bookmarks


class pushoperation(object):
    """A object that represent a single push operation

    It purpose is to carry push related state and very common operation.

    A new should be created at the begining of each push and discarded
    afterward.
    """

    def __init__(self, repo, remote, force=False, revs=None, newbranch=False):
        # repo we push from
        self.repo = repo
        self.ui = repo.ui
        # repo we push to
        self.remote = remote
        # force option provided
        self.force = force
        # revs to be pushed (None is "all")
        self.revs = revs
        # allow push of new branch
        self.newbranch = newbranch
        # did a local lock get acquired?
        self.locallocked = None
        # Integer version of the push result
        # - None means nothing to push
        # - 0 means HTTP error
        # - 1 means we pushed and remote head count is unchanged *or*
        #   we have outgoing changesets but refused to push
        # - other values as described by addchangegroup()
        self.ret = None
        # discover.outgoing object (contains common and outgoin data)
        self.outgoing = None
        # all remote heads before the push
        self.remoteheads = None
        # testable as a boolean indicating if any nodes are missing locally.
        self.incoming = None
        # set of all heads common after changeset bundle push
        self.commonheads = None

def push(repo, remote, force=False, revs=None, newbranch=False):
    '''Push outgoing changesets (limited by revs) from a local
    repository to remote. Return an integer:
      - None means nothing to push
      - 0 means HTTP error
      - 1 means we pushed and remote head count is unchanged *or*
        we have outgoing changesets but refused to push
      - other values as described by addchangegroup()
    '''
    pushop = pushoperation(repo, remote, force, revs, newbranch)
    if pushop.remote.local():
        missing = (set(pushop.repo.requirements)
                   - pushop.remote.local().supported)
        if missing:
            msg = _("required features are not"
                    " supported in the destination:"
                    " %s") % (', '.join(sorted(missing)))
            raise util.Abort(msg)

    # there are two ways to push to remote repo:
    #
    # addchangegroup assumes local user can lock remote
    # repo (local filesystem, old ssh servers).
    #
    # unbundle assumes local user cannot lock remote repo (new ssh
    # servers, http servers).

    if not pushop.remote.canpush():
        raise util.Abort(_("destination does not support push"))
    # get local lock as we might write phase data
    locallock = None
    try:
        locallock = pushop.repo.lock()
        pushop.locallocked = True
    except IOError, err:
        pushop.locallocked = False
        if err.errno != errno.EACCES:
            raise
        # source repo cannot be locked.
        # We do not abort the push, but just disable the local phase
        # synchronisation.
        msg = 'cannot lock source repository: %s\n' % err
        pushop.ui.debug(msg)
    try:
        pushop.repo.checkpush(pushop.force, pushop.revs)
        lock = None
        unbundle = pushop.remote.capable('unbundle')
        if not unbundle:
            lock = pushop.remote.lock()
        try:
            _pushdiscovery(pushop)
            if _pushcheckoutgoing(pushop):
                _pushchangeset(pushop)
            _pushcomputecommonheads(pushop)
            _pushsyncphase(pushop)
            _pushobsolete(pushop)
        finally:
            if lock is not None:
                lock.release()
    finally:
        if locallock is not None:
            locallock.release()

    _pushbookmark(pushop)
    return pushop.ret

def _pushdiscovery(pushop):
    # discovery
    unfi = pushop.repo.unfiltered()
    fci = discovery.findcommonincoming
    commoninc = fci(unfi, pushop.remote, force=pushop.force)
    common, inc, remoteheads = commoninc
    fco = discovery.findcommonoutgoing
    outgoing = fco(unfi, pushop.remote, onlyheads=pushop.revs,
                   commoninc=commoninc, force=pushop.force)
    pushop.outgoing = outgoing
    pushop.remoteheads = remoteheads
    pushop.incoming = inc

def _pushcheckoutgoing(pushop):
    outgoing = pushop.outgoing
    unfi = pushop.repo.unfiltered()
    if not outgoing.missing:
        # nothing to push
        scmutil.nochangesfound(unfi.ui, unfi, outgoing.excluded)
        return False
    # something to push
    if not pushop.force:
        # if repo.obsstore == False --> no obsolete
        # then, save the iteration
        if unfi.obsstore:
            # this message are here for 80 char limit reason
            mso = _("push includes obsolete changeset: %s!")
            mst = "push includes %s changeset: %s!"
            # plain versions for i18n tool to detect them
            _("push includes unstable changeset: %s!")
            _("push includes bumped changeset: %s!")
            _("push includes divergent changeset: %s!")
            # If we are to push if there is at least one
            # obsolete or unstable changeset in missing, at
            # least one of the missinghead will be obsolete or
            # unstable. So checking heads only is ok
            for node in outgoing.missingheads:
                ctx = unfi[node]
                if ctx.obsolete():
                    raise util.Abort(mso % ctx)
                elif ctx.troubled():
                    raise util.Abort(_(mst)
                                     % (ctx.troubles()[0],
                                        ctx))
        newbm = pushop.ui.configlist('bookmarks', 'pushing')
        discovery.checkheads(unfi, pushop.remote, outgoing,
                             pushop.remoteheads,
                             pushop.newbranch,
                             bool(pushop.incoming),
                             newbm)
    return True

def _pushchangeset(pushop):
    """Make the actual push of changeset bundle to remote repo"""
    outgoing = pushop.outgoing
    unbundle = pushop.remote.capable('unbundle')
    # TODO: get bundlecaps from remote
    bundlecaps = None
    # create a changegroup from local
    if pushop.revs is None and not (outgoing.excluded
                            or pushop.repo.changelog.filteredrevs):
        # push everything,
        # use the fast path, no race possible on push
        bundler = changegroup.bundle10(pushop.repo, bundlecaps)
        cg = pushop.repo._changegroupsubset(outgoing,
                                            bundler,
                                            'push',
                                            fastpath=True)
    else:
        cg = pushop.repo.getlocalbundle('push', outgoing, bundlecaps)

    # apply changegroup to remote
    if unbundle:
        # local repo finds heads on server, finds out what
        # revs it must push. once revs transferred, if server
        # finds it has different heads (someone else won
        # commit/push race), server aborts.
        if pushop.force:
            remoteheads = ['force']
        else:
            remoteheads = pushop.remoteheads
        # ssh: return remote's addchangegroup()
        # http: return remote's addchangegroup() or 0 for error
        pushop.ret = pushop.remote.unbundle(cg, remoteheads,
                                            'push')
    else:
        # we return an integer indicating remote head count
        # change
        pushop.ret = pushop.remote.addchangegroup(cg, 'push',
                                                              pushop.repo.url())

def _pushcomputecommonheads(pushop):
    unfi = pushop.repo.unfiltered()
    if pushop.ret:
        # push succeed, synchronize target of the push
        cheads = pushop.outgoing.missingheads
    elif pushop.revs is None:
        # All out push fails. synchronize all common
        cheads = pushop.outgoing.commonheads
    else:
        # I want cheads = heads(::missingheads and ::commonheads)
        # (missingheads is revs with secret changeset filtered out)
        #
        # This can be expressed as:
        #     cheads = ( (missingheads and ::commonheads)
        #              + (commonheads and ::missingheads))"
        #              )
        #
        # while trying to push we already computed the following:
        #     common = (::commonheads)
        #     missing = ((commonheads::missingheads) - commonheads)
        #
        # We can pick:
        # * missingheads part of common (::commonheads)
        common = set(pushop.outgoing.common)
        nm = pushop.repo.changelog.nodemap
        cheads = [node for node in pushop.revs if nm[node] in common]
        # and
        # * commonheads parents on missing
        revset = unfi.set('%ln and parents(roots(%ln))',
                         pushop.outgoing.commonheads,
                         pushop.outgoing.missing)
        cheads.extend(c.node() for c in revset)
    pushop.commonheads = cheads

def _pushsyncphase(pushop):
    """synchronise phase information locally and remotly"""
    unfi = pushop.repo.unfiltered()
    cheads = pushop.commonheads
    if pushop.ret:
        # push succeed, synchronize target of the push
        cheads = pushop.outgoing.missingheads
    elif pushop.revs is None:
        # All out push fails. synchronize all common
        cheads = pushop.outgoing.commonheads
    else:
        # I want cheads = heads(::missingheads and ::commonheads)
        # (missingheads is revs with secret changeset filtered out)
        #
        # This can be expressed as:
        #     cheads = ( (missingheads and ::commonheads)
        #              + (commonheads and ::missingheads))"
        #              )
        #
        # while trying to push we already computed the following:
        #     common = (::commonheads)
        #     missing = ((commonheads::missingheads) - commonheads)
        #
        # We can pick:
        # * missingheads part of common (::commonheads)
        common = set(pushop.outgoing.common)
        nm = pushop.repo.changelog.nodemap
        cheads = [node for node in pushop.revs if nm[node] in common]
        # and
        # * commonheads parents on missing
        revset = unfi.set('%ln and parents(roots(%ln))',
                         pushop.outgoing.commonheads,
                         pushop.outgoing.missing)
        cheads.extend(c.node() for c in revset)
    pushop.commonheads = cheads
    # even when we don't push, exchanging phase data is useful
    remotephases = pushop.remote.listkeys('phases')
    if (pushop.ui.configbool('ui', '_usedassubrepo', False)
        and remotephases    # server supports phases
        and pushop.ret is None # nothing was pushed
        and remotephases.get('publishing', False)):
        # When:
        # - this is a subrepo push
        # - and remote support phase
        # - and no changeset was pushed
        # - and remote is publishing
        # We may be in issue 3871 case!
        # We drop the possible phase synchronisation done by
        # courtesy to publish changesets possibly locally draft
        # on the remote.
        remotephases = {'publishing': 'True'}
    if not remotephases: # old server or public only rer
        _localphasemove(pushop, cheads)
        # don't push any phase data as there is nothing to push
    else:
        ana = phases.analyzeremotephases(pushop.repo, cheads,
                                         remotephases)
        pheads, droots = ana
        ### Apply remote phase on local
        if remotephases.get('publishing', False):
            _localphasemove(pushop, cheads)
        else: # publish = False
            _localphasemove(pushop, pheads)
            _localphasemove(pushop, cheads, phases.draft)
        ### Apply local phase on remote

        # Get the list of all revs draft on remote by public here.
        # XXX Beware that revset break if droots is not strictly
        # XXX root we may want to ensure it is but it is costly
        outdated =  unfi.set('heads((%ln::%ln) and public())',
                             droots, cheads)
        for newremotehead in outdated:
            r = pushop.remote.pushkey('phases',
                                      newremotehead.hex(),
                                      str(phases.draft),
                                      str(phases.public))
            if not r:
                pushop.ui.warn(_('updating %s to public failed!\n')
                                       % newremotehead)

def _localphasemove(pushop, nodes, phase=phases.public):
    """move <nodes> to <phase> in the local source repo"""
    if pushop.locallocked:
        phases.advanceboundary(pushop.repo, phase, nodes)
    else:
        # repo is not locked, do not change any phases!
        # Informs the user that phases should have been moved when
        # applicable.
        actualmoves = [n for n in nodes if phase < pushop.repo[n].phase()]
        phasestr = phases.phasenames[phase]
        if actualmoves:
            pushop.ui.status(_('cannot lock source repo, skipping '
                               'local %s phase update\n') % phasestr)

def _pushobsolete(pushop):
    """utility function to push obsolete markers to a remote"""
    pushop.ui.debug('try to push obsolete markers to remote\n')
    repo = pushop.repo
    remote = pushop.remote
    if (obsolete._enabled and repo.obsstore and
        'obsolete' in remote.listkeys('namespaces')):
        rslts = []
        remotedata = repo.listkeys('obsolete')
        for key in sorted(remotedata, reverse=True):
            # reverse sort to ensure we end with dump0
            data = remotedata[key]
            rslts.append(remote.pushkey('obsolete', key, '', data))
        if [r for r in rslts if not r]:
            msg = _('failed to push some obsolete markers!\n')
            repo.ui.warn(msg)

def _pushbookmark(pushop):
    """Update bookmark position on remote"""
    ui = pushop.ui
    repo = pushop.repo.unfiltered()
    remote = pushop.remote
    ui.debug("checking for updated bookmarks\n")
    revnums = map(repo.changelog.rev, pushop.revs or [])
    ancestors = [a for a in repo.changelog.ancestors(revnums, inclusive=True)]
    (addsrc, adddst, advsrc, advdst, diverge, differ, invalid
     ) = bookmarks.compare(repo, repo._bookmarks, remote.listkeys('bookmarks'),
                           srchex=hex)

    for b, scid, dcid in advsrc:
        if ancestors and repo[scid].rev() not in ancestors:
            continue
        if remote.pushkey('bookmarks', b, dcid, scid):
            ui.status(_("updating bookmark %s\n") % b)
        else:
            ui.warn(_('updating bookmark %s failed!\n') % b)


def pull(repo, remote, heads=None, force=False):
    if remote.local():
        missing = set(remote.requirements) - repo.supported
        if missing:
            msg = _("required features are not"
                    " supported in the destination:"
                    " %s") % (', '.join(sorted(missing)))
            raise util.Abort(msg)

    # don't open transaction for nothing or you break future useful
    # rollback call
    tr = None
    trname = 'pull\n' + util.hidepassword(remote.url())
    lock = repo.lock()
    try:
        tmp = discovery.findcommonincoming(repo.unfiltered(), remote,
                                           heads=heads, force=force)
        common, fetch, rheads = tmp
        if not fetch:
            repo.ui.status(_("no changes found\n"))
            result = 0
        else:
            tr = repo.transaction(trname)
            if heads is None and list(common) == [nullid]:
                repo.ui.status(_("requesting all changes\n"))
            elif heads is None and remote.capable('changegroupsubset'):
                # issue1320, avoid a race if remote changed after discovery
                heads = rheads

            if remote.capable('getbundle'):
                # TODO: get bundlecaps from remote
                cg = remote.getbundle('pull', common=common,
                                      heads=heads or rheads)
            elif heads is None:
                cg = remote.changegroup(fetch, 'pull')
            elif not remote.capable('changegroupsubset'):
                raise util.Abort(_("partial pull cannot be done because "
                                       "other repository doesn't support "
                                       "changegroupsubset."))
            else:
                cg = remote.changegroupsubset(fetch, heads, 'pull')
            result = repo.addchangegroup(cg, 'pull', remote.url())

        # compute target subset
        if heads is None:
            # We pulled every thing possible
            # sync on everything common
            subset = common + rheads
        else:
            # We pulled a specific subset
            # sync on this subset
            subset = heads

        # Get remote phases data from remote
        remotephases = remote.listkeys('phases')
        publishing = bool(remotephases.get('publishing', False))
        if remotephases and not publishing:
            # remote is new and unpublishing
            pheads, _dr = phases.analyzeremotephases(repo, subset,
                                                     remotephases)
            phases.advanceboundary(repo, phases.public, pheads)
            phases.advanceboundary(repo, phases.draft, subset)
        else:
            # Remote is old or publishing all common changesets
            # should be seen as public
            phases.advanceboundary(repo, phases.public, subset)

        def gettransaction():
            if tr is None:
                return repo.transaction(trname)
            return tr

        obstr = obsolete.syncpull(repo, remote, gettransaction)
        if obstr is not None:
            tr = obstr

        if tr is not None:
            tr.close()
    finally:
        if tr is not None:
            tr.release()
        lock.release()

    return result
