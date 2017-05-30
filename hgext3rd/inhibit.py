"""reduce the changesets evolution feature scope for early and noob friendly ui

the full scale changeset evolution have some massive bleeding edge and it is
very easy for people not very intimate with the concept to end up in intricate
situation. in order to get some of the benefit sooner, this extension is
disabling some of the less polished aspect of evolution. it should gradually
get thinner and thinner as changeset evolution will get more polished. this
extension is only recommended for large scale organisations. individual user
should probably stick on using evolution in its current state, understand its
concept and provide feedback

This extension provides the ability to "inhibit" obsolescence markers. obsolete
revision can be cheaply brought back to life that way.
However as the inhibitor are not fitting in an append only model, this is
incompatible with sharing mutable history.
"""
from mercurial import bookmarks
from mercurial import commands
from mercurial import error
from mercurial import extensions
from mercurial import localrepo
from mercurial import lock as lockmod
from mercurial import obsolete
from mercurial import registrar
from mercurial import scmutil
from mercurial import util
from mercurial.i18n import _

cmdtable = {}

if util.safehasattr(registrar, 'command'):
    command = registrar.command(cmdtable)
else: # compat with hg < 4.3
    from mercurial import cmdutil
    command = cmdutil.command(cmdtable)

def _inhibitenabled(repo):
    return util.safehasattr(repo, '_obsinhibit')

def reposetup(ui, repo):

    class obsinhibitedrepo(repo.__class__):

        @localrepo.storecache('obsinhibit')
        def _obsinhibit(self):
            # XXX we should make sure it is invalidated by transaction failure
            obsinhibit = set()
            raw = self.svfs.tryread('obsinhibit')
            for i in xrange(0, len(raw), 20):
                obsinhibit.add(raw[i:i + 20])
            return obsinhibit

        def commit(self, *args, **kwargs):
            newnode = super(obsinhibitedrepo, self).commit(*args, **kwargs)
            if newnode is not None:
                _inhibitmarkers(repo, [newnode])
            return newnode

    repo.__class__ = obsinhibitedrepo

def _update(orig, ui, repo, *args, **kwargs):
    """
    When moving to a commit we want to inhibit any obsolete commit affecting
    the changeset we are updating to. In other words we don't want any visible
    commit to be obsolete.
    """
    wlock = None
    try:
        # Evolve is running a hook on lock release to display a warning message
        # if the workind dir's parent is obsolete.
        # We take the lock here to make sure that we inhibit the parent before
        # that hook get a chance to run.
        wlock = repo.wlock()
        res = orig(ui, repo, *args, **kwargs)
        newhead = repo['.'].node()
        _inhibitmarkers(repo, [newhead])
        return res
    finally:
        lockmod.release(wlock)

def _bookmarkchanged(orig, bkmstoreinst, *args, **kwargs):
    """ Add inhibition markers to every obsolete bookmarks """
    repo = bkmstoreinst._repo
    bkmstorenodes = [repo[v].node() for v in bkmstoreinst.values()]
    _inhibitmarkers(repo, bkmstorenodes)
    return orig(bkmstoreinst, *args, **kwargs)

def _bookmark(orig, ui, repo, *bookmarks, **opts):
    """ Add a -D option to the bookmark command, map it to prune -B """
    haspruneopt = opts.get('prune', False)
    if not haspruneopt:
        return orig(ui, repo, *bookmarks, **opts)
    elif opts.get('rename'):
        raise error.Abort('Cannot use both -m and -D')
    elif len(bookmarks) == 0:
        hint = _('make sure to put a space between -D and your bookmark name')
        raise error.Abort(_('Error, please check your command'), hint=hint)

    # Call prune -B
    evolve = extensions.find('evolve')
    optsdict = {
        'new': [],
        'succ': [],
        'rev': [],
        'bookmark': bookmarks,
        'keep': None,
        'biject': False,
    }
    evolve.cmdprune(ui, repo, **optsdict)

# obsolescence inhibitor
########################

def _schedulewrite(tr, obsinhibit):
    """Make sure on disk content will be updated on transaction commit"""
    def writer(fp):
        """Serialize the inhibited list to disk.
        """
        raw = ''.join(obsinhibit)
        fp.write(raw)
    tr.addfilegenerator('obsinhibit', ('obsinhibit',), writer)
    tr.hookargs['obs_inbihited'] = '1'

def _filterpublic(repo, nodes):
    """filter out inhibitor on public changeset

    Public changesets are already immune to obsolescence"""
    getrev = repo.changelog.nodemap.get
    getphase = repo._phasecache.phase
    return (n for n in nodes
            if getrev(n) is not None and getphase(repo, getrev(n)))

def _inhibitmarkers(repo, nodes):
    """add marker inhibitor for all obsolete revision under <nodes>

    Content of <nodes> and all mutable ancestors are considered. Marker for
    obsolete revision only are created.
    """
    if not _inhibitenabled(repo):
        return

    # we add (non public()) as a lower boundary to
    # - use the C code in 3.6 (no ancestors in C as this is written)
    # - restrict the search space. Otherwise, the ancestors can spend a lot of
    #   time iterating if you have a check very low in the repo. We do not need
    #   to iterate over tens of thousand of public revisions with higher
    #   revision number
    #
    # In addition, the revset logic could be made significantly smarter here.
    newinhibit = repo.revs('(not public())::%ln and obsolete()', nodes)
    if newinhibit:
        node = repo.changelog.node
        lock = tr = None
        try:
            lock = repo.lock()
            tr = repo.transaction('obsinhibit')
            repo._obsinhibit.update(node(r) for r in newinhibit)
            _schedulewrite(tr, _filterpublic(repo, repo._obsinhibit))
            repo.invalidatevolatilesets()
            tr.close()
        finally:
            lockmod.release(tr, lock)

def _deinhibitmarkers(repo, nodes):
    """lift obsolescence inhibition on a set of nodes

    This will be triggered when inhibited nodes received new obsolescence
    markers. Otherwise the new obsolescence markers would also be inhibited.
    """
    if not _inhibitenabled(repo):
        return

    deinhibited = repo._obsinhibit & set(nodes)
    if deinhibited:
        tr = repo.transaction('obsinhibit')
        try:
            repo._obsinhibit -= deinhibited
            _schedulewrite(tr, _filterpublic(repo, repo._obsinhibit))
            repo.invalidatevolatilesets()
            tr.close()
        finally:
            tr.release()

def _createmarkers(orig, repo, relations, *args, **kwargs):
    """wrap markers create to make sure we de-inhibit target nodes"""
    # wrapping transactio to unify the one in each function
    lock = tr = None
    try:
        lock = repo.lock()
        tr = repo.transaction('add-obsolescence-marker')
        orig(repo, relations, *args, **kwargs)
        precs = (r[0].node() for r in relations)
        _deinhibitmarkers(repo, precs)
        tr.close()
    finally:
        lockmod.release(tr, lock)

def _filterobsoleterevswrap(orig, repo, rebasesetrevs, *args, **kwargs):
    repo._notinhibited = rebasesetrevs
    try:
        repo.invalidatevolatilesets()
        r = orig(repo, rebasesetrevs, *args, **kwargs)
    finally:
        del repo._notinhibited
        repo.invalidatevolatilesets()
    return r

def transactioncallback(orig, repo, desc, *args, **kwargs):
    """ Wrap localrepo.transaction to inhibit new obsolete changes """
    def inhibitposttransaction(transaction):
        # At the end of the transaction we catch all the new visible and
        # obsolete commit to inhibit them
        visibleobsolete = repo.revs('obsolete() - hidden()')
        ignoreset = set(getattr(repo, '_rebaseset', []))
        ignoreset |= set(getattr(repo, '_obsoletenotrebased', []))
        visibleobsolete = list(r for r in visibleobsolete if r not in ignoreset)
        if visibleobsolete:
            _inhibitmarkers(repo, [repo[r].node() for r in visibleobsolete])
    transaction = orig(repo, desc, *args, **kwargs)
    if desc != 'strip' and _inhibitenabled(repo):
        transaction.addpostclose('inhibitposttransaction',
                                 inhibitposttransaction)
    return transaction


# We wrap these two functions to address the following scenario:
# - Assuming that we have markers between commits in the rebase set and
#   destination and that these markers are inhibited
# - At the end of the rebase the nodes are still visible because rebase operate
#   without inhibition and skip these nodes
# We keep track in repo._obsoletenotrebased of the obsolete commits skipped by
# the rebase and lift the inhibition in the end of the rebase.

def _computeobsoletenotrebased(orig, repo, *args, **kwargs):
    r = orig(repo, *args, **kwargs)
    repo._obsoletenotrebased = r.keys()
    return r

def _clearrebased(orig, ui, repo, *args, **kwargs):
    r = orig(ui, repo, *args, **kwargs)
    tonode = repo.changelog.node
    if util.safehasattr(repo, '_obsoletenotrebased'):
        _deinhibitmarkers(repo, [tonode(k) for k in repo._obsoletenotrebased])
    return r


def extsetup(ui):
    # lets wrap the computation of the obsolete set
    # We apply inhibition there
    obsfunc = obsolete.cachefuncs['obsolete']

    def _computeobsoleteset(repo):
        """remove any inhibited nodes from the obsolete set

        This will trickle down to other part of mercurial (hidden, log, etc)"""
        obs = obsfunc(repo)
        if _inhibitenabled(repo):
            getrev = repo.changelog.nodemap.get
            blacklist = getattr(repo, '_notinhibited', set())
            for n in repo._obsinhibit:
                if getrev(n) not in blacklist:
                    obs.discard(getrev(n))
        return obs
    try:
        extensions.find('directaccess')
    except KeyError:
        errormsg = _('cannot use inhibit without the direct access extension\n')
        hint = _("(please enable it or inhibit won\'t work)\n")
        ui.warn(errormsg)
        ui.warn(hint)
        return

    # Wrapping this to inhibit obsolete revs resulting from a transaction
    extensions.wrapfunction(localrepo.localrepository,
                            'transaction', transactioncallback)

    obsolete.cachefuncs['obsolete'] = _computeobsoleteset
    # wrap create marker to make it able to lift the inhibition
    extensions.wrapfunction(obsolete, 'createmarkers', _createmarkers)
    # drop divergence computation since it is incompatible with "light revive"
    obsolete.cachefuncs['divergent'] = lambda repo: set()
    # drop bumped computation since it is incompatible with "light revive"
    obsolete.cachefuncs['bumped'] = lambda repo: set()
    # wrap update to make sure that no obsolete commit is visible after an
    # update
    extensions.wrapcommand(commands.table, 'update', _update)
    try:
        rebase = extensions.find('rebase')
        if rebase:
            if util.safehasattr(rebase, '_filterobsoleterevs'):
                extensions.wrapfunction(rebase,
                                        '_filterobsoleterevs',
                                        _filterobsoleterevswrap)
            extensions.wrapfunction(rebase, 'clearrebased', _clearrebased)
            if util.safehasattr(rebase, '_computeobsoletenotrebased'):
                extensions.wrapfunction(rebase,
                                        '_computeobsoletenotrebased',
                                        _computeobsoletenotrebased)

    except KeyError:
        pass
    # There are two ways to save bookmark changes during a transation, we
    # wrap both to add inhibition markers.
    extensions.wrapfunction(bookmarks.bmstore, 'recordchange', _bookmarkchanged)
    if getattr(bookmarks.bmstore, 'write', None) is not None:# mercurial < 3.9
        extensions.wrapfunction(bookmarks.bmstore, 'write', _bookmarkchanged)
    # Add bookmark -D option
    entry = extensions.wrapcommand(commands.table, 'bookmark', _bookmark)
    entry[1].append(('D', 'prune', None,
                    _('delete the bookmark and prune the commits underneath')))

@command('debugobsinhibit', [], '')
def cmddebugobsinhibit(ui, repo, *revs):
    """inhibit obsolescence markers effect on a set of revs"""
    nodes = (repo[r].node() for r in scmutil.revrange(repo, revs))
    _inhibitmarkers(repo, nodes)
