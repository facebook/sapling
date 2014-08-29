# exchange.py - utility to exchange data between repos.
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
from node import hex, nullid
import errno, urllib
import util, scmutil, changegroup, base85, error
import discovery, phases, obsolete, bookmarks, bundle2, pushkey

def readbundle(ui, fh, fname, vfs=None):
    header = changegroup.readexactly(fh, 4)

    alg = None
    if not fname:
        fname = "stream"
        if not header.startswith('HG') and header.startswith('\0'):
            fh = changegroup.headerlessfixup(fh, header)
            header = "HG10"
            alg = 'UN'
    elif vfs:
        fname = vfs.join(fname)

    magic, version = header[0:2], header[2:4]

    if magic != 'HG':
        raise util.Abort(_('%s: not a Mercurial bundle') % fname)
    if version == '10':
        if alg is None:
            alg = changegroup.readexactly(fh, 2)
        return changegroup.unbundle10(fh, alg)
    elif version == '2X':
        return bundle2.unbundle20(ui, fh, header=magic + version)
    else:
        raise util.Abort(_('%s: unknown bundle version %s') % (fname, version))

def buildobsmarkerspart(bundler, markers):
    """add an obsmarker part to the bundler with <markers>

    No part is created if markers is empty.
    Raises ValueError if the bundler doesn't support any known obsmarker format.
    """
    if markers:
        remoteversions = bundle2.obsmarkersversion(bundler.capabilities)
        version = obsolete.commonversion(remoteversions)
        if version is None:
            raise ValueError('bundler do not support common obsmarker format')
        stream = obsolete.encodemarkers(markers, True, version=version)
        return bundler.newpart('B2X:OBSMARKERS', data=stream)
    return None

class pushoperation(object):
    """A object that represent a single push operation

    It purpose is to carry push related state and very common operation.

    A new should be created at the beginning of each push and discarded
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
        # step already performed
        # (used to check what steps have been already performed through bundle2)
        self.stepsdone = set()
        # Integer version of the push result
        # - None means nothing to push
        # - 0 means HTTP error
        # - 1 means we pushed and remote head count is unchanged *or*
        #   we have outgoing changesets but refused to push
        # - other values as described by addchangegroup()
        self.ret = None
        # discover.outgoing object (contains common and outgoing data)
        self.outgoing = None
        # all remote heads before the push
        self.remoteheads = None
        # testable as a boolean indicating if any nodes are missing locally.
        self.incoming = None
        # phases changes that must be pushed along side the changesets
        self.outdatedphases = None
        # phases changes that must be pushed if changeset push fails
        self.fallbackoutdatedphases = None
        # outgoing obsmarkers
        self.outobsmarkers = set()
        # outgoing bookmarks
        self.outbookmarks = []

    @util.propertycache
    def futureheads(self):
        """future remote heads if the changeset push succeeds"""
        return self.outgoing.missingheads

    @util.propertycache
    def fallbackheads(self):
        """future remote heads if the changeset push fails"""
        if self.revs is None:
            # not target to push, all common are relevant
            return self.outgoing.commonheads
        unfi = self.repo.unfiltered()
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
        common = set(self.outgoing.common)
        nm = self.repo.changelog.nodemap
        cheads = [node for node in self.revs if nm[node] in common]
        # and
        # * commonheads parents on missing
        revset = unfi.set('%ln and parents(roots(%ln))',
                         self.outgoing.commonheads,
                         self.outgoing.missing)
        cheads.extend(c.node() for c in revset)
        return cheads

    @property
    def commonheads(self):
        """set of all common heads after changeset bundle push"""
        if self.ret:
            return self.futureheads
        else:
            return self.fallbackheads

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
        pushop.repo.checkpush(pushop)
        lock = None
        unbundle = pushop.remote.capable('unbundle')
        if not unbundle:
            lock = pushop.remote.lock()
        try:
            _pushdiscovery(pushop)
            if (pushop.repo.ui.configbool('experimental', 'bundle2-exp',
                                          False)
                and pushop.remote.capable('bundle2-exp')):
                _pushbundle2(pushop)
            _pushchangeset(pushop)
            _pushsyncphase(pushop)
            _pushobsolete(pushop)
            _pushbookmark(pushop)
        finally:
            if lock is not None:
                lock.release()
    finally:
        if locallock is not None:
            locallock.release()

    return pushop.ret

# list of steps to perform discovery before push
pushdiscoveryorder = []

# Mapping between step name and function
#
# This exists to help extensions wrap steps if necessary
pushdiscoverymapping = {}

def pushdiscovery(stepname):
    """decorator for function performing discovery before push

    The function is added to the step -> function mapping and appended to the
    list of steps.  Beware that decorated function will be added in order (this
    may matter).

    You can only use this decorator for a new step, if you want to wrap a step
    from an extension, change the pushdiscovery dictionary directly."""
    def dec(func):
        assert stepname not in pushdiscoverymapping
        pushdiscoverymapping[stepname] = func
        pushdiscoveryorder.append(stepname)
        return func
    return dec

def _pushdiscovery(pushop):
    """Run all discovery steps"""
    for stepname in pushdiscoveryorder:
        step = pushdiscoverymapping[stepname]
        step(pushop)

@pushdiscovery('changeset')
def _pushdiscoverychangeset(pushop):
    """discover the changeset that need to be pushed"""
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

@pushdiscovery('phase')
def _pushdiscoveryphase(pushop):
    """discover the phase that needs to be pushed

    (computed for both success and failure case for changesets push)"""
    outgoing = pushop.outgoing
    unfi = pushop.repo.unfiltered()
    remotephases = pushop.remote.listkeys('phases')
    publishing = remotephases.get('publishing', False)
    ana = phases.analyzeremotephases(pushop.repo,
                                     pushop.fallbackheads,
                                     remotephases)
    pheads, droots = ana
    extracond = ''
    if not publishing:
        extracond = ' and public()'
    revset = 'heads((%%ln::%%ln) %s)' % extracond
    # Get the list of all revs draft on remote by public here.
    # XXX Beware that revset break if droots is not strictly
    # XXX root we may want to ensure it is but it is costly
    fallback = list(unfi.set(revset, droots, pushop.fallbackheads))
    if not outgoing.missing:
        future = fallback
    else:
        # adds changeset we are going to push as draft
        #
        # should not be necessary for pushblishing server, but because of an
        # issue fixed in xxxxx we have to do it anyway.
        fdroots = list(unfi.set('roots(%ln  + %ln::)',
                       outgoing.missing, droots))
        fdroots = [f.node() for f in fdroots]
        future = list(unfi.set(revset, fdroots, pushop.futureheads))
    pushop.outdatedphases = future
    pushop.fallbackoutdatedphases = fallback

@pushdiscovery('obsmarker')
def _pushdiscoveryobsmarkers(pushop):
    if (obsolete._enabled
        and pushop.repo.obsstore
        and 'obsolete' in pushop.remote.listkeys('namespaces')):
        repo = pushop.repo
        # very naive computation, that can be quite expensive on big repo.
        # However: evolution is currently slow on them anyway.
        nodes = (c.node() for c in repo.set('::%ln', pushop.futureheads))
        pushop.outobsmarkers = pushop.repo.obsstore.relevantmarkers(nodes)

@pushdiscovery('bookmarks')
def _pushdiscoverybookmarks(pushop):
    ui = pushop.ui
    repo = pushop.repo.unfiltered()
    remote = pushop.remote
    ui.debug("checking for updated bookmarks\n")
    ancestors = ()
    if pushop.revs:
        revnums = map(repo.changelog.rev, pushop.revs)
        ancestors = repo.changelog.ancestors(revnums, inclusive=True)
    remotebookmark = remote.listkeys('bookmarks')

    comp = bookmarks.compare(repo, repo._bookmarks, remotebookmark, srchex=hex)
    addsrc, adddst, advsrc, advdst, diverge, differ, invalid = comp
    for b, scid, dcid in advsrc:
        if not ancestors or repo[scid].rev() in ancestors:
            pushop.outbookmarks.append((b, dcid, scid))

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

# List of names of steps to perform for an outgoing bundle2, order matters.
b2partsgenorder = []

# Mapping between step name and function
#
# This exists to help extensions wrap steps if necessary
b2partsgenmapping = {}

def b2partsgenerator(stepname):
    """decorator for function generating bundle2 part

    The function is added to the step -> function mapping and appended to the
    list of steps.  Beware that decorated functions will be added in order
    (this may matter).

    You can only use this decorator for new steps, if you want to wrap a step
    from an extension, attack the b2partsgenmapping dictionary directly."""
    def dec(func):
        assert stepname not in b2partsgenmapping
        b2partsgenmapping[stepname] = func
        b2partsgenorder.append(stepname)
        return func
    return dec

@b2partsgenerator('changeset')
def _pushb2ctx(pushop, bundler):
    """handle changegroup push through bundle2

    addchangegroup result is stored in the ``pushop.ret`` attribute.
    """
    if 'changesets' in pushop.stepsdone:
        return
    pushop.stepsdone.add('changesets')
    # Send known heads to the server for race detection.
    if not _pushcheckoutgoing(pushop):
        return
    pushop.repo.prepushoutgoinghooks(pushop.repo,
                                     pushop.remote,
                                     pushop.outgoing)
    if not pushop.force:
        bundler.newpart('B2X:CHECK:HEADS', data=iter(pushop.remoteheads))
    cg = changegroup.getlocalbundle(pushop.repo, 'push', pushop.outgoing)
    cgpart = bundler.newpart('B2X:CHANGEGROUP', data=cg.getchunks())
    def handlereply(op):
        """extract addchangroup returns from server reply"""
        cgreplies = op.records.getreplies(cgpart.id)
        assert len(cgreplies['changegroup']) == 1
        pushop.ret = cgreplies['changegroup'][0]['return']
    return handlereply

@b2partsgenerator('phase')
def _pushb2phases(pushop, bundler):
    """handle phase push through bundle2"""
    if 'phases' in pushop.stepsdone:
        return
    b2caps = bundle2.bundle2caps(pushop.remote)
    if not 'b2x:pushkey' in b2caps:
        return
    pushop.stepsdone.add('phases')
    part2node = []
    enc = pushkey.encode
    for newremotehead in pushop.outdatedphases:
        part = bundler.newpart('b2x:pushkey')
        part.addparam('namespace', enc('phases'))
        part.addparam('key', enc(newremotehead.hex()))
        part.addparam('old', enc(str(phases.draft)))
        part.addparam('new', enc(str(phases.public)))
        part2node.append((part.id, newremotehead))
    def handlereply(op):
        for partid, node in part2node:
            partrep = op.records.getreplies(partid)
            results = partrep['pushkey']
            assert len(results) <= 1
            msg = None
            if not results:
                msg = _('server ignored update of %s to public!\n') % node
            elif not int(results[0]['return']):
                msg = _('updating %s to public failed!\n') % node
            if msg is not None:
                pushop.ui.warn(msg)
    return handlereply

@b2partsgenerator('obsmarkers')
def _pushb2obsmarkers(pushop, bundler):
    if 'obsmarkers' in pushop.stepsdone:
        return
    remoteversions = bundle2.obsmarkersversion(bundler.capabilities)
    if obsolete.commonversion(remoteversions) is None:
        return
    pushop.stepsdone.add('obsmarkers')
    if pushop.outobsmarkers:
        buildobsmarkerspart(bundler, pushop.outobsmarkers)

@b2partsgenerator('bookmarks')
def _pushb2bookmarks(pushop, bundler):
    """handle phase push through bundle2"""
    if 'bookmarks' in pushop.stepsdone:
        return
    b2caps = bundle2.bundle2caps(pushop.remote)
    if 'b2x:pushkey' not in b2caps:
        return
    pushop.stepsdone.add('bookmarks')
    part2book = []
    enc = pushkey.encode
    for book, old, new in pushop.outbookmarks:
        part = bundler.newpart('b2x:pushkey')
        part.addparam('namespace', enc('bookmarks'))
        part.addparam('key', enc(book))
        part.addparam('old', enc(old))
        part.addparam('new', enc(new))
        part2book.append((part.id, book))
    def handlereply(op):
        for partid, book in part2book:
            partrep = op.records.getreplies(partid)
            results = partrep['pushkey']
            assert len(results) <= 1
            if not results:
                pushop.ui.warn(_('server ignored bookmark %s update\n') % book)
            else:
                ret = int(results[0]['return'])
                if ret:
                    pushop.ui.status(_("updating bookmark %s\n") % book)
                else:
                    pushop.ui.warn(_('updating bookmark %s failed!\n') % book)
    return handlereply


def _pushbundle2(pushop):
    """push data to the remote using bundle2

    The only currently supported type of data is changegroup but this will
    evolve in the future."""
    bundler = bundle2.bundle20(pushop.ui, bundle2.bundle2caps(pushop.remote))
    # create reply capability
    capsblob = bundle2.encodecaps(bundle2.getrepocaps(pushop.repo))
    bundler.newpart('b2x:replycaps', data=capsblob)
    replyhandlers = []
    for partgenname in b2partsgenorder:
        partgen = b2partsgenmapping[partgenname]
        ret = partgen(pushop, bundler)
        if callable(ret):
            replyhandlers.append(ret)
    # do not push if nothing to push
    if bundler.nbparts <= 1:
        return
    stream = util.chunkbuffer(bundler.getchunks())
    try:
        reply = pushop.remote.unbundle(stream, ['force'], 'push')
    except error.BundleValueError, exc:
        raise util.Abort('missing support for %s' % exc)
    try:
        op = bundle2.processbundle(pushop.repo, reply)
    except error.BundleValueError, exc:
        raise util.Abort('missing support for %s' % exc)
    for rephand in replyhandlers:
        rephand(op)

def _pushchangeset(pushop):
    """Make the actual push of changeset bundle to remote repo"""
    if 'changesets' in pushop.stepsdone:
        return
    pushop.stepsdone.add('changesets')
    if not _pushcheckoutgoing(pushop):
        return
    pushop.repo.prepushoutgoinghooks(pushop.repo,
                                     pushop.remote,
                                     pushop.outgoing)
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
        cg = changegroup.getsubset(pushop.repo,
                                   outgoing,
                                   bundler,
                                   'push',
                                   fastpath=True)
    else:
        cg = changegroup.getlocalbundle(pushop.repo, 'push', outgoing,
                                        bundlecaps)

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
                                            pushop.repo.url())
    else:
        # we return an integer indicating remote head count
        # change
        pushop.ret = pushop.remote.addchangegroup(cg, 'push', pushop.repo.url())

def _pushsyncphase(pushop):
    """synchronise phase information locally and remotely"""
    cheads = pushop.commonheads
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
    if not remotephases: # old server or public only reply from non-publishing
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

        if pushop.ret:
            if 'phases' in pushop.stepsdone:
                # phases already pushed though bundle2
                return
            outdated = pushop.outdatedphases
        else:
            outdated = pushop.fallbackoutdatedphases

        pushop.stepsdone.add('phases')

        # filter heads already turned public by the push
        outdated = [c for c in outdated if c.node() not in pheads]
        b2caps = bundle2.bundle2caps(pushop.remote)
        if 'b2x:pushkey' in b2caps:
            # server supports bundle2, let's do a batched push through it
            #
            # This will eventually be unified with the changesets bundle2 push
            bundler = bundle2.bundle20(pushop.ui, b2caps)
            capsblob = bundle2.encodecaps(bundle2.getrepocaps(pushop.repo))
            bundler.newpart('b2x:replycaps', data=capsblob)
            part2node = []
            enc = pushkey.encode
            for newremotehead in outdated:
                part = bundler.newpart('b2x:pushkey')
                part.addparam('namespace', enc('phases'))
                part.addparam('key', enc(newremotehead.hex()))
                part.addparam('old', enc(str(phases.draft)))
                part.addparam('new', enc(str(phases.public)))
                part2node.append((part.id, newremotehead))
            stream = util.chunkbuffer(bundler.getchunks())
            try:
                reply = pushop.remote.unbundle(stream, ['force'], 'push')
                op = bundle2.processbundle(pushop.repo, reply)
            except error.BundleValueError, exc:
                raise util.Abort('missing support for %s' % exc)
            for partid, node in part2node:
                partrep = op.records.getreplies(partid)
                results = partrep['pushkey']
                assert len(results) <= 1
                msg = None
                if not results:
                    msg = _('server ignored update of %s to public!\n') % node
                elif not int(results[0]['return']):
                    msg = _('updating %s to public failed!\n') % node
                if msg is not None:
                    pushop.ui.warn(msg)

        else:
            # fallback to independant pushkey command
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
        tr = pushop.repo.transaction('push-phase-sync')
        try:
            phases.advanceboundary(pushop.repo, tr, phase, nodes)
            tr.close()
        finally:
            tr.release()
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
    if 'obsmarkers' in pushop.stepsdone:
        return
    pushop.ui.debug('try to push obsolete markers to remote\n')
    repo = pushop.repo
    remote = pushop.remote
    pushop.stepsdone.add('obsmarkers')
    if pushop.outobsmarkers:
        rslts = []
        remotedata = obsolete._pushkeyescape(pushop.outobsmarkers)
        for key in sorted(remotedata, reverse=True):
            # reverse sort to ensure we end with dump0
            data = remotedata[key]
            rslts.append(remote.pushkey('obsolete', key, '', data))
        if [r for r in rslts if not r]:
            msg = _('failed to push some obsolete markers!\n')
            repo.ui.warn(msg)

def _pushbookmark(pushop):
    """Update bookmark position on remote"""
    if pushop.ret == 0 or 'bookmarks' in pushop.stepsdone:
        return
    pushop.stepsdone.add('bookmarks')
    ui = pushop.ui
    remote = pushop.remote
    for b, old, new in pushop.outbookmarks:
        if remote.pushkey('bookmarks', b, old, new):
            ui.status(_("updating bookmark %s\n") % b)
        else:
            ui.warn(_('updating bookmark %s failed!\n') % b)

class pulloperation(object):
    """A object that represent a single pull operation

    It purpose is to carry push related state and very common operation.

    A new should be created at the beginning of each pull and discarded
    afterward.
    """

    def __init__(self, repo, remote, heads=None, force=False):
        # repo we pull into
        self.repo = repo
        # repo we pull from
        self.remote = remote
        # revision we try to pull (None is "all")
        self.heads = heads
        # do we force pull?
        self.force = force
        # the name the pull transaction
        self._trname = 'pull\n' + util.hidepassword(remote.url())
        # hold the transaction once created
        self._tr = None
        # set of common changeset between local and remote before pull
        self.common = None
        # set of pulled head
        self.rheads = None
        # list of missing changeset to fetch remotely
        self.fetch = None
        # result of changegroup pulling (used as return code by pull)
        self.cgresult = None
        # list of step remaining todo (related to future bundle2 usage)
        self.todosteps = set(['changegroup', 'phases', 'obsmarkers'])

    @util.propertycache
    def pulledsubset(self):
        """heads of the set of changeset target by the pull"""
        # compute target subset
        if self.heads is None:
            # We pulled every thing possible
            # sync on everything common
            c = set(self.common)
            ret = list(self.common)
            for n in self.rheads:
                if n not in c:
                    ret.append(n)
            return ret
        else:
            # We pulled a specific subset
            # sync on this subset
            return self.heads

    def gettransaction(self):
        """get appropriate pull transaction, creating it if needed"""
        if self._tr is None:
            self._tr = self.repo.transaction(self._trname)
        return self._tr

    def closetransaction(self):
        """close transaction if created"""
        if self._tr is not None:
            self._tr.close()

    def releasetransaction(self):
        """release transaction if created"""
        if self._tr is not None:
            self._tr.release()

def pull(repo, remote, heads=None, force=False):
    pullop = pulloperation(repo, remote, heads, force)
    if pullop.remote.local():
        missing = set(pullop.remote.requirements) - pullop.repo.supported
        if missing:
            msg = _("required features are not"
                    " supported in the destination:"
                    " %s") % (', '.join(sorted(missing)))
            raise util.Abort(msg)

    lock = pullop.repo.lock()
    try:
        _pulldiscovery(pullop)
        if (pullop.repo.ui.configbool('experimental', 'bundle2-exp', False)
            and pullop.remote.capable('bundle2-exp')):
            _pullbundle2(pullop)
        if 'changegroup' in pullop.todosteps:
            _pullchangeset(pullop)
        if 'phases' in pullop.todosteps:
            _pullphase(pullop)
        if 'obsmarkers' in pullop.todosteps:
            _pullobsolete(pullop)
        pullop.closetransaction()
    finally:
        pullop.releasetransaction()
        lock.release()

    return pullop.cgresult

def _pulldiscovery(pullop):
    """discovery phase for the pull

    Current handle changeset discovery only, will change handle all discovery
    at some point."""
    tmp = discovery.findcommonincoming(pullop.repo.unfiltered(),
                                       pullop.remote,
                                       heads=pullop.heads,
                                       force=pullop.force)
    pullop.common, pullop.fetch, pullop.rheads = tmp

def _pullbundle2(pullop):
    """pull data using bundle2

    For now, the only supported data are changegroup."""
    remotecaps = bundle2.bundle2caps(pullop.remote)
    kwargs = {'bundlecaps': caps20to10(pullop.repo)}
    # pulling changegroup
    pullop.todosteps.remove('changegroup')

    kwargs['common'] = pullop.common
    kwargs['heads'] = pullop.heads or pullop.rheads
    kwargs['cg'] = pullop.fetch
    if 'b2x:listkeys' in remotecaps:
        kwargs['listkeys'] = ['phase']
    if not pullop.fetch:
        pullop.repo.ui.status(_("no changes found\n"))
        pullop.cgresult = 0
    else:
        if pullop.heads is None and list(pullop.common) == [nullid]:
            pullop.repo.ui.status(_("requesting all changes\n"))
    _pullbundle2extraprepare(pullop, kwargs)
    if kwargs.keys() == ['format']:
        return # nothing to pull
    bundle = pullop.remote.getbundle('pull', **kwargs)
    try:
        op = bundle2.processbundle(pullop.repo, bundle, pullop.gettransaction)
    except error.BundleValueError, exc:
        raise util.Abort('missing support for %s' % exc)

    if pullop.fetch:
        assert len(op.records['changegroup']) == 1
        pullop.cgresult = op.records['changegroup'][0]['return']

    # processing phases change
    for namespace, value in op.records['listkeys']:
        if namespace == 'phases':
            _pullapplyphases(pullop, value)

def _pullbundle2extraprepare(pullop, kwargs):
    """hook function so that extensions can extend the getbundle call"""
    pass

def _pullchangeset(pullop):
    """pull changeset from unbundle into the local repo"""
    # We delay the open of the transaction as late as possible so we
    # don't open transaction for nothing or you break future useful
    # rollback call
    pullop.todosteps.remove('changegroup')
    if not pullop.fetch:
            pullop.repo.ui.status(_("no changes found\n"))
            pullop.cgresult = 0
            return
    pullop.gettransaction()
    if pullop.heads is None and list(pullop.common) == [nullid]:
        pullop.repo.ui.status(_("requesting all changes\n"))
    elif pullop.heads is None and pullop.remote.capable('changegroupsubset'):
        # issue1320, avoid a race if remote changed after discovery
        pullop.heads = pullop.rheads

    if pullop.remote.capable('getbundle'):
        # TODO: get bundlecaps from remote
        cg = pullop.remote.getbundle('pull', common=pullop.common,
                                     heads=pullop.heads or pullop.rheads)
    elif pullop.heads is None:
        cg = pullop.remote.changegroup(pullop.fetch, 'pull')
    elif not pullop.remote.capable('changegroupsubset'):
        raise util.Abort(_("partial pull cannot be done because "
                           "other repository doesn't support "
                           "changegroupsubset."))
    else:
        cg = pullop.remote.changegroupsubset(pullop.fetch, pullop.heads, 'pull')
    pullop.cgresult = changegroup.addchangegroup(pullop.repo, cg, 'pull',
                                                 pullop.remote.url())

def _pullphase(pullop):
    # Get remote phases data from remote
    remotephases = pullop.remote.listkeys('phases')
    _pullapplyphases(pullop, remotephases)

def _pullapplyphases(pullop, remotephases):
    """apply phase movement from observed remote state"""
    pullop.todosteps.remove('phases')
    publishing = bool(remotephases.get('publishing', False))
    if remotephases and not publishing:
        # remote is new and unpublishing
        pheads, _dr = phases.analyzeremotephases(pullop.repo,
                                                 pullop.pulledsubset,
                                                 remotephases)
        dheads = pullop.pulledsubset
    else:
        # Remote is old or publishing all common changesets
        # should be seen as public
        pheads = pullop.pulledsubset
        dheads = []
    unfi = pullop.repo.unfiltered()
    phase = unfi._phasecache.phase
    rev = unfi.changelog.nodemap.get
    public = phases.public
    draft = phases.draft

    # exclude changesets already public locally and update the others
    pheads = [pn for pn in pheads if phase(unfi, rev(pn)) > public]
    if pheads:
        tr = pullop.gettransaction()
        phases.advanceboundary(pullop.repo, tr, public, pheads)

    # exclude changesets already draft locally and update the others
    dheads = [pn for pn in dheads if phase(unfi, rev(pn)) > draft]
    if dheads:
        tr = pullop.gettransaction()
        phases.advanceboundary(pullop.repo, tr, draft, dheads)

def _pullobsolete(pullop):
    """utility function to pull obsolete markers from a remote

    The `gettransaction` is function that return the pull transaction, creating
    one if necessary. We return the transaction to inform the calling code that
    a new transaction have been created (when applicable).

    Exists mostly to allow overriding for experimentation purpose"""
    pullop.todosteps.remove('obsmarkers')
    tr = None
    if obsolete._enabled:
        pullop.repo.ui.debug('fetching remote obsolete markers\n')
        remoteobs = pullop.remote.listkeys('obsolete')
        if 'dump0' in remoteobs:
            tr = pullop.gettransaction()
            for key in sorted(remoteobs, reverse=True):
                if key.startswith('dump'):
                    data = base85.b85decode(remoteobs[key])
                    pullop.repo.obsstore.mergemarkers(tr, data)
            pullop.repo.invalidatevolatilesets()
    return tr

def caps20to10(repo):
    """return a set with appropriate options to use bundle20 during getbundle"""
    caps = set(['HG2X'])
    capsblob = bundle2.encodecaps(bundle2.getrepocaps(repo))
    caps.add('bundle2=' + urllib.quote(capsblob))
    return caps

def getbundle(repo, source, heads=None, common=None, bundlecaps=None,
              **kwargs):
    """return a full bundle (with potentially multiple kind of parts)

    Could be a bundle HG10 or a bundle HG2X depending on bundlecaps
    passed. For now, the bundle can contain only changegroup, but this will
    changes when more part type will be available for bundle2.

    This is different from changegroup.getbundle that only returns an HG10
    changegroup bundle. They may eventually get reunited in the future when we
    have a clearer idea of the API we what to query different data.

    The implementation is at a very early stage and will get massive rework
    when the API of bundle is refined.
    """
    cg = None
    if kwargs.get('cg', True):
        # build changegroup bundle here.
        cg = changegroup.getbundle(repo, source, heads=heads,
                                   common=common, bundlecaps=bundlecaps)
    elif 'HG2X' not in bundlecaps:
        raise ValueError(_('request for bundle10 must include changegroup'))
    if bundlecaps is None or 'HG2X' not in bundlecaps:
        if kwargs:
            raise ValueError(_('unsupported getbundle arguments: %s')
                             % ', '.join(sorted(kwargs.keys())))
        return cg
    # very crude first implementation,
    # the bundle API will change and the generation will be done lazily.
    b2caps = {}
    for bcaps in bundlecaps:
        if bcaps.startswith('bundle2='):
            blob = urllib.unquote(bcaps[len('bundle2='):])
            b2caps.update(bundle2.decodecaps(blob))
    bundler = bundle2.bundle20(repo.ui, b2caps)
    if cg:
        bundler.newpart('b2x:changegroup', data=cg.getchunks())
    listkeys = kwargs.get('listkeys', ())
    for namespace in listkeys:
        part = bundler.newpart('b2x:listkeys')
        part.addparam('namespace', namespace)
        keys = repo.listkeys(namespace).items()
        part.data = pushkey.encodekeys(keys)
    _getbundleobsmarkerpart(bundler, repo, source, heads=heads, common=common,
                            bundlecaps=bundlecaps, **kwargs)
    _getbundleextrapart(bundler, repo, source, heads=heads, common=common,
                        bundlecaps=bundlecaps, **kwargs)
    return util.chunkbuffer(bundler.getchunks())

def _getbundleobsmarkerpart(bundler, repo, source, heads=None, common=None,
                            bundlecaps=None, **kwargs):
    if kwargs.get('obsmarkers', False):
        if heads is None:
            heads = repo.heads()
        subset = [c.node() for c in repo.set('::%ln', heads)]
        markers = repo.obsstore.relevantmarkers(subset)
        buildobsmarkerspart(bundler, markers)

def _getbundleextrapart(bundler, repo, source, heads=None, common=None,
                        bundlecaps=None, **kwargs):
    """hook function to let extensions add parts to the requested bundle"""
    pass

def check_heads(repo, their_heads, context):
    """check if the heads of a repo have been modified

    Used by peer for unbundling.
    """
    heads = repo.heads()
    heads_hash = util.sha1(''.join(sorted(heads))).digest()
    if not (their_heads == ['force'] or their_heads == heads or
            their_heads == ['hashed', heads_hash]):
        # someone else committed/pushed/unbundled while we
        # were transferring data
        raise error.PushRaced('repository changed while %s - '
                              'please try again' % context)

def unbundle(repo, cg, heads, source, url):
    """Apply a bundle to a repo.

    this function makes sure the repo is locked during the application and have
    mechanism to check that no push race occurred between the creation of the
    bundle and its application.

    If the push was raced as PushRaced exception is raised."""
    r = 0
    # need a transaction when processing a bundle2 stream
    tr = None
    lock = repo.lock()
    try:
        check_heads(repo, heads, 'uploading changes')
        # push can proceed
        if util.safehasattr(cg, 'params'):
            try:
                tr = repo.transaction('unbundle')
                tr.hookargs['bundle2-exp'] = '1'
                r = bundle2.processbundle(repo, cg, lambda: tr).reply
                cl = repo.unfiltered().changelog
                p = cl.writepending() and repo.root or ""
                repo.hook('b2x-pretransactionclose', throw=True, source=source,
                          url=url, pending=p, **tr.hookargs)
                tr.close()
                repo.hook('b2x-transactionclose', source=source, url=url,
                          **tr.hookargs)
            except Exception, exc:
                exc.duringunbundle2 = True
                raise
        else:
            r = changegroup.addchangegroup(repo, cg, source, url)
    finally:
        if tr is not None:
            tr.release()
        lock.release()
    return r
