# pushrebase.py - server-side rebasing of pushed commits
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import errno, os, tempfile, sys, operator, resource

try:
    import json
except ImportError:
    import simplejson as json

from mercurial import bundle2, cmdutil, hg, scmutil, exchange, commands
from mercurial import util, error, discovery, changegroup, context, revset
from mercurial import obsolete, pushkey, phases
from mercurial.extensions import wrapcommand, wrapfunction
from mercurial.bundlerepo import bundlerepository
from mercurial.node import nullid, hex, bin
from mercurial.i18n import _

cmdtable = {}
command = cmdutil.command(cmdtable)

rebaseparttype = 'b2x:rebase'

experimental = 'experimental'
configonto = 'server-rebase-onto'

def extsetup(ui):
    entry = wrapcommand(commands.table, 'push', _push)
    entry[1].append(('', 'onto', '', _('server revision to rebase onto')))

    exchange.b2partsgenorder.insert(
        exchange.b2partsgenorder.index('changeset'),
        exchange.b2partsgenorder.pop(
            exchange.b2partsgenorder.index(rebaseparttype)
        ),
    )

    wrapfunction(discovery, 'checkheads', _checkheads)

    origpushkeyhandler = bundle2.parthandlermapping['b2x:pushkey']
    newpushkeyhandler = lambda *args, **kwargs: \
        bundle2pushkey(origpushkeyhandler, *args, **kwargs)
    newpushkeyhandler.params = origpushkeyhandler.params
    bundle2.parthandlermapping['b2x:pushkey'] = newpushkeyhandler

def getrevsetbounds(repo, revset):
    if not repo.revs(revset):
        raise util.Abort(_('nothing to rebase'))

    if repo.revs('%r and merge()', revset):
        raise util.Abort(_('cannot rebase merge changesets'))

    tails = repo.revs('%r and ancestor(%r)', revset, revset)
    if not tails:
        raise util.Abort(_('cannot rebase unrelated changesets'))
    if len(tails) != 1:
        raise util.Abort(_('logic error: multiple tails not possible'))
    tail = repo[tails.first()]

    heads = repo.revs('heads(%r)', revset)
    if len(heads) > 1:
        raise util.Abort(_('cannot rebase divergent changesets'))
    head = repo[heads.first()]

    return head, tail
    
def getrebasepart(repo, peer, outgoing, onto, newhead=False):
    if not outgoing.missing:
        raise util.Abort(_('no commits to rebase'))
    
    if rebaseparttype not in bundle2.bundle2caps(peer):
        raise util.Abort(_('no server support for %r') % rebaseparttype)

    head, tail = getrevsetbounds(
        repo, 
        revset.formatspec('%ln', outgoing.missing),
    )

    repo.ui.note(
        _('asking remote to rebase %r::%r onto %r\n') %
        (head.hex(), tail.hex(), onto)
    )

    cg = changegroup.getlocalchangegroupraw(
        repo,
        'push',
        outgoing,
    )

    return bundle2.bundlepart(
        rebaseparttype,
        mandatoryparams={
            'onto': onto,
            'newhead': repr(newhead),
        }.items(),
        advisoryparams={
            'head': head.node(),
            'tail': tail.node(),
            'commonheads':
                json.dumps([hex(n) for n in outgoing.commonheads]),
        }.items(),
        data = cg
    )

@command('debugserverrebase', [
    ('r', 'rev', 'tip', _('revision to push (includes ancestors)')),
    ('d', 'dest', 'default', _('server revision to rebase onto')),
    ('e', 'ssh', None, _('specify ssh command to use')),
    ('', 'newhead', None, _('allow pushing a new head')),
    ('', 'remotecmd', 'hg', _('specify hg command to run on the remote side')),
], _('hg debugserverrebase [options] [server]'))
def debugserverrebase(ui, repo, server='default', **opts):
    '''For debugging only: manually issues a server-side rebase request.

    Use 'hg push --onto' if you're not hacking on this extension'''
    
    rev = scmutil.revsingle(repo, opts['rev'])
    dest = opts['dest']
    if not dest:
        raise util.Abort(_('destination is required for server-side rebase'))
    peer = hg.peer(repo, opts, ui.expandpath(server))

    outgoing = discovery.findcommonoutgoing(
        repo.unfiltered(),
        peer,
        onlyheads=[rev.node()],
        commoninc=discovery.findcommonincoming(repo.unfiltered(), peer),
    )

    remotecaps = bundle2.bundle2caps(peer)
    bundler = bundle2.bundle20(ui, remotecaps)
    bundler.newpart(
        'b2x:replycaps',
        data=bundle2.encodecaps(bundle2.getrepocaps(repo)),
    )

    rebasepart = getrebasepart(repo, peer, outgoing, dest, opts.get('newhead'))
    bundler.addpart(rebasepart)

    # TODO: check whether the following code (copied from standard push) is
    # appropriate
    stream = util.chunkbuffer(bundler.getchunks())
    try:
        reply = peer.unbundle(stream, ['force'], 'push')
    except error.BundleValueError, exc:
        raise util.Abort('missing support for %s' % exc)
    try:
        op = bundle2.processbundle(repo, reply)
    except error.BundleValueError, exc:
        raise util.Abort('missing support for %s' % exc)
    #for rephand in replyhandlers:
    #    rephand(op)

    raise util.Abort('stop')

def _checkheads(orig, repo, remote, *args, **kwargs):
    onto = repo.ui.config(experimental, configonto)
    if onto: # This is a rebasing push
        # The rest of the checks are performed during bundle2 part processing;
        # we need to bypass the regular push checks because it will look like
        # we're pushing a new head, which isn't normally allowed
        if not repo.ui.configbool('experimental', 'bundle2-exp', False):
            raise util.Abort(_('bundle2 needs to be enabled on client'))
        if not remote.capable('bundle2-exp'):
            raise util.Abort(_('bundle2 needs to be enabled on server'))
        return
    else:
        return orig(repo, remote, *args, **kwargs)
    
def _push(orig, ui, repo, *args, **opts):
    oldonto = ui.backupconfig(experimental, configonto)

    ui.setconfig(experimental, configonto, opts.get('onto'), '--onto')
    if ui.config(experimental, configonto):
        oldphasemove = wrapfunction(exchange, '_localphasemove', _phasemove)
    result = orig(ui, repo, *args, **opts)

    ui.restoreconfig(oldonto)
    if oldphasemove:
        exchange._localphasemove = oldphasemove
    return result

def _phasemove(orig, pushop, nodes, phase=phases.public):
    """prevent commits from being marked public

    Since these are going to be mutated on the server, they aren't really being
    published, their successors are.  If we mark these as public now, hg evolve
    will refuse to fix them for us later."""
    
    if phase != phases.public:
        orig(pushop, nodes, phase)

# TODO: verify that commit hooks fire appropriately
@exchange.b2partsgenerator(rebaseparttype)
def partgen(pushop, bundler):
    onto = pushop.ui.config(experimental, configonto)
    if 'changesets' in pushop.stepsdone or not onto:
        return

    pushop.stepsdone.add('changesets')
    if not pushop.outgoing.missing:
        upshop.ui.note(_('no changes to push'))
        pushop.cgresult = 0
        return
    
    rebasepart = getrebasepart(
        pushop.repo,
        pushop.remote,
        pushop.outgoing,
        onto,
        pushop.newbranch
    )

    bundler.addpart(rebasepart)

    def handlereply(op):
        # TODO: read result from server?
        pushop.cgresult = 1

    return handlereply

bundle2.capabilities[rebaseparttype] = ()

# TODO: split this function into smaller pieces
@bundle2.parthandler(rebaseparttype, ('onto', 'newhead'))
def bundle2rebase(op, part):
    '''unbundle a bundle2 containing a changegroup to rebase'''

    params = part.params
    op.gettransaction()

    bundlefile = None
    fp = None

    try: # guards bundlefile
        fd, bundlefile = tempfile.mkstemp()
        try: # guards fp
            fp = os.fdopen(fd, 'wb')
            magic = 'HG10UN'
            fp.write(magic)
            data = part.read(resource.getpagesize() - len(magic))
            while data:
                fp.write(data)
                data = part.read(resource.getpagesize())
        finally:
            fp.close()

        bundle = bundlerepository(op.repo.ui, op.repo.root, bundlefile)
        head, tail = getrevsetbounds(bundle, 'bundle()')

        if head.node() != params.get('head', head.node()):
            raise util.Abort(
                _('was told to expect head %r but received %r instead'),
                (head.node(), hex(params['head'])),
            );

        if tail.node() != params.get('tail', tail.node()):
            raise util.Abort(
                _('was told to expect tail %r but received %r instead'),
                (tail.node(), hex(params['tail'])),
            );

        onto = scmutil.revsingle(op.repo, params['onto'])
        bundleonto = bundle[onto.hex()]

        if not params['newhead']:
            if not op.repo.revs('%r and head()', params['onto']):
                raise util.Abort(_('rebase would produce a new head on server'))

        if bundleonto.ancestor(tail).hex() != tail.p1().hex():
            raise util.Abort(
                _('changegroup not forked from an ancestor of %r') %
                ((params['onto'], bundleonto.ancestor(tail).hex(), tail.p1().hex()),)
            )

        revs = [bundle[r] for r in bundle.revs('sort(bundle())')]

        #TODO: Is there a more efficient way to do this check?
        files = reduce(operator.or_, [set(rev.files()) for rev in revs], set())
        commonmanifest = tail.p1().manifest().intersectfiles(files)
        ontomanifest = bundleonto.manifest().intersectfiles(files)
        conflicts = ontomanifest.diff(commonmanifest).keys()
        if conflicts:
            raise util.Abort(_('conflicting changes in %r') % conflicts)

        replacements = {}

        for rev in revs:
            newrev = context.memctx(
                op.repo,
                [onto.node(), nullid],
                rev.description(),
                rev.files(),
                lambda repo, memctx, path: context.memfilectx(
                    repo,
                    path,
                    rev[path].data(),
                ),
                rev.user(),
                rev.date(),
                rev.extra(),
            ).commit()

            onto = op.repo[newrev]
            replacements[rev.node()] = onto.node()

        if obsolete.isenabled(op.repo, obsolete.createmarkersopt):
            markers = [
                (bundle[oldrev], (op.repo[newrev],))
                for oldrev, newrev in replacements.items()
                if newrev != oldrev
            ]

            # TODO: make sure these weren't public originally
            for old, new in markers:
                old.mutable = lambda *args: True

            obsolete.createmarkers(op.repo, markers)


    finally:
        try:
            if bundlefile:
                os.unlink(bundlefile)
        except OSError, e:
            if e.errno != errno.ENOENT:
                raise

    if (params['commonheads']
        and op.reply
        and 'b2x:pushback' in op.reply.capabilities):
        # TODO: investigate alternatives for determining which commits to send
        #       (via @pyd: https://phabricator.fb.com/D1689551#inline-12737312 )
        outgoing = discovery.outgoing(
            op.repo.changelog,
            [bin(n) for n in json.loads(params['commonheads'])],
            [new for old, new in replacements.items() if old != new],
        )

        if outgoing.missing:
            cg = changegroup.getlocalchangegroupraw(
                op.repo,
                'rebase:reply',
                outgoing,
            )

            # TODO: fix version handshake; use newest mutually supported version
            cgpart = op.reply.newpart('b2x:changegroup', data = cg)
            cgpart.addparam('version', '01')

            if (obsolete.isenabled(op.repo, obsolete.exchangeopt)
                and op.repo.obsstore):
                try:
                    exchange.buildobsmarkerspart(
                        op.reply,
                        op.repo.obsstore.relevantmarkers(replacements.values())
                    )
                except ValueError, exc:
                    op.repo.ui.status(_("can't send obsolete markers: %s") %
                                      exc.message)

    for k in replacements.keys():
        replacements[hex(k)] = hex(replacements[k])

    op.records.add(rebaseparttype, replacements)

    return 1

def bundle2pushkey(orig, op, part):
    replacements = dict(sum([record.items()
                             for record
                             in op.records[rebaseparttype]],
                            []))
    
    namespace = pushkey.decode(part.params['namespace'])
    if namespace == 'phases':
        key = pushkey.decode(part.params['key'])
        part.params['key'] = pushkey.encode(replacements.get(key, key))
    if namespace == 'bookmarks':
        new = pushkey.decode(part.params['new'])
        part.params['new'] = pushkey.encode(replacements.get(new, new))

    return orig(op, part)
