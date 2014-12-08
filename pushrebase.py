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
commonheadsparttype = 'b2x:commonheads'

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

    exchange.b2partsgenorder.insert(
        0,
        exchange.b2partsgenorder.pop(
            exchange.b2partsgenorder.index(commonheadsparttype)
        ),
    )

    wrapfunction(discovery, 'checkheads', _checkheads)

    origpushkeyhandler = bundle2.parthandlermapping['b2x:pushkey']
    newpushkeyhandler = lambda *args, **kwargs: \
        bundle2pushkey(origpushkeyhandler, *args, **kwargs)
    newpushkeyhandler.params = origpushkeyhandler.params
    bundle2.parthandlermapping['b2x:pushkey'] = newpushkeyhandler

def validaterevset(repo, revset):
    "Abort if this is a rebasable revset, return None otherwise"
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

    repo.ui.note(_('validated revset %r::%r for rebase\n') %
                 (head.hex(), tail.hex()))

def revsettail(repo, revset):
    "Return the root changectx of a revset"
    tails = repo.revs('%r and ancestor(%r)', revset, revset)
    tail = tails.first()
    if tail is None:
        raise ValueError(_("revset doesn't have a single tail"))
    return repo[tail]
    
def getrebasepart(repo, peer, outgoing, onto, newhead=False):
    if not outgoing.missing:
        raise util.Abort(_('no commits to rebase'))
    
    if rebaseparttype not in bundle2.bundle2caps(peer):
        raise util.Abort(_('no server support for %r') % rebaseparttype)

    validaterevset(repo, revset.formatspec('%ln', outgoing.missing))

    cg = changegroup.getlocalchangegroupraw(
        repo,
        'push',
        outgoing,
    )

    return bundle2.bundlepart(
        rebaseparttype.upper(), # .upper() marks this as a mandatory part:
                                # server will abort if there's no handler
        mandatoryparams={
            'onto': onto,
            'newhead': repr(newhead),
        }.items(),
        data = cg
    )

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

#TODO: extract common heads transmission into separate extension
@exchange.b2partsgenerator(commonheadsparttype)
def commonheadspartgen(pushop, bundler):
    bundler.newpart(commonheadsparttype,
                    data=''.join(pushop.outgoing.commonheads))

@bundle2.parthandler(commonheadsparttype)
def commonheadshandler(op, inpart):
    nodeid = inpart.read(20)
    while len(nodeid) == 20:
        op.records.add(commonheadsparttype, nodeid)
        nodeid = inpart.read(20)
    assert not nodeid # data should split evenly into blocks of 20 bytes

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
    tr = op.gettransaction()
    hookargs = dict(tr.hookargs)

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
        validaterevset(bundle, 'bundle()')
        tail = revsettail(bundle, 'bundle()')

        onto = scmutil.revsingle(op.repo, params['onto'])
        bundleonto = bundle[onto.hex()]

        if not params['newhead']:
            if not op.repo.revs('%r and head()', params['onto']):
                raise util.Abort(_('rebase would produce a new head on server'))

        if bundleonto.ancestor(tail).hex() != tail.p1().hex():
            raise util.Abort(_('missing changesets between %r and %r') %
                             (bundleonto.ancestor(tail).hex(),
                              tail.p1().hex()))

        revs = [bundle[r] for r in bundle.revs('sort(bundle())')]

        #TODO: Is there a more efficient way to do this check?
        files = reduce(operator.or_, [set(rev.files()) for rev in revs], set())
        commonmanifest = tail.p1().manifest().intersectfiles(files)
        ontomanifest = bundleonto.manifest().intersectfiles(files)
        conflicts = ontomanifest.diff(commonmanifest).keys()
        if conflicts:
            raise util.Abort(_('conflicting changes in %r') % conflicts)

        op.repo.hook("prechangegroup", **hookargs)

        replacements = {}
        added = []

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
            added.append(onto.node())

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

    p = lambda: tr.writepending() and op.repo.root or ""
    op.repo.hook("pretxnchangegroup", throw=True, pending=p, **hookargs)

    def runhooks():
        op.repo.hook("changegroup", **hookargs)
        for n in added:
            args = hookargs.copy()
            args['node'] = hex(n)
            op.repo.hook("incoming", **args)

    tr.addpostclose('serverrebase-cg-hooks',
                    lambda tr: op.repo._afterlock(runhooks))

    if (op.records[commonheadsparttype]
        and op.reply
        and 'b2x:pushback' in op.reply.capabilities):
        outgoing = discovery.outgoing(
            op.repo.changelog,
            op.records[commonheadsparttype],
            [new for old, new in replacements.items() if old != new],
        )

        if outgoing.missing:
            cgversions = set(op.reply.capabilities.get('b2x:changegroup'))
            if not cgversions:
                cgversions.add('01')
            version = max(cgversions & set(changegroup.packermap.keys()))
            
            cg = changegroup.getlocalchangegroupraw(
                op.repo,
                'rebase:reply',
                outgoing,
                version = version
            )

            cgpart = op.reply.newpart('B2X:CHANGEGROUP', data = cg)
            if version != '01':
                cgpart.addparam('version', version)

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
