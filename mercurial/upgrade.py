# repair.py - functions for repository repair for mercurial
#
# Copyright 2005, 2006 Chris Mason <mason@suse.com>
# Copyright 2007 Matt Mackall
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import stat
import tempfile

from .i18n import _
from . import (
    changelog,
    error,
    manifest,
    revlog,
    scmutil,
    util,
    vfs as vfsmod,
)

def requiredsourcerequirements(repo):
    """Obtain requirements required to be present to upgrade a repo.

    An upgrade will not be allowed if the repository doesn't have the
    requirements returned by this function.
    """
    return set([
        # Introduced in Mercurial 0.9.2.
        'revlogv1',
        # Introduced in Mercurial 0.9.2.
        'store',
    ])

def upgradeblocksourcerequirements(repo):
    """Obtain requirements that will prevent an upgrade from occurring.

    An upgrade cannot be performed if the source repository contains a
    requirements in the returned set.
    """
    return set([
        # The upgrade code does not yet support these experimental features.
        # This is an artificial limitation.
        'manifestv2',
        'treemanifest',
        # This was a precursor to generaldelta and was never enabled by default.
        # It should (hopefully) not exist in the wild.
        'parentdelta',
        # Upgrade should operate on the actual store, not the shared link.
        'shared',
    ])

def upgradesupportremovedrequirements(repo):
    """Obtain requirements that can be removed during an upgrade.

    If an upgrade were to create a repository that dropped a requirement,
    the dropped requirement must appear in the returned set for the upgrade
    to be allowed.
    """
    return set()

def upgradesupporteddestrequirements(repo):
    """Obtain requirements that upgrade supports in the destination.

    If the result of the upgrade would create requirements not in this set,
    the upgrade is disallowed.

    Extensions should monkeypatch this to add their custom requirements.
    """
    return set([
        'dotencode',
        'fncache',
        'generaldelta',
        'revlogv1',
        'store',
    ])

def upgradeallowednewrequirements(repo):
    """Obtain requirements that can be added to a repository during upgrade.

    This is used to disallow proposed requirements from being added when
    they weren't present before.

    We use a list of allowed requirement additions instead of a list of known
    bad additions because the whitelist approach is safer and will prevent
    future, unknown requirements from accidentally being added.
    """
    return set([
        'dotencode',
        'fncache',
        'generaldelta',
    ])

deficiency = 'deficiency'
optimisation = 'optimization'

class upgradeimprovement(object):
    """Represents an improvement that can be made as part of an upgrade.

    The following attributes are defined on each instance:

    name
       Machine-readable string uniquely identifying this improvement. It
       will be mapped to an action later in the upgrade process.

    type
       Either ``deficiency`` or ``optimisation``. A deficiency is an obvious
       problem. An optimization is an action (sometimes optional) that
       can be taken to further improve the state of the repository.

    description
       Message intended for humans explaining the improvement in more detail,
       including the implications of it. For ``deficiency`` types, should be
       worded in the present tense. For ``optimisation`` types, should be
       worded in the future tense.

    upgrademessage
       Message intended for humans explaining what an upgrade addressing this
       issue will do. Should be worded in the future tense.

    fromdefault (``deficiency`` types only)
       Boolean indicating whether the current (deficient) state deviates
       from Mercurial's default configuration.

    fromconfig (``deficiency`` types only)
       Boolean indicating whether the current (deficient) state deviates
       from the current Mercurial configuration.
    """
    def __init__(self, name, type, description, upgrademessage, **kwargs):
        self.name = name
        self.type = type
        self.description = description
        self.upgrademessage = upgrademessage

        for k, v in kwargs.items():
            setattr(self, k, v)

def upgradefindimprovements(repo):
    """Determine improvements that can be made to the repo during upgrade.

    Returns a list of ``upgradeimprovement`` describing repository deficiencies
    and optimizations.
    """
    # Avoid cycle: cmdutil -> repair -> localrepo -> cmdutil
    from . import localrepo

    newreporeqs = localrepo.newreporequirements(repo)

    improvements = []

    # We could detect lack of revlogv1 and store here, but they were added
    # in 0.9.2 and we don't support upgrading repos without these
    # requirements, so let's not bother.

    if 'fncache' not in repo.requirements:
        improvements.append(upgradeimprovement(
            name='fncache',
            type=deficiency,
            description=_('long and reserved filenames may not work correctly; '
                          'repository performance is sub-optimal'),
            upgrademessage=_('repository will be more resilient to storing '
                             'certain paths and performance of certain '
                             'operations should be improved'),
            fromdefault=True,
            fromconfig='fncache' in newreporeqs))

    if 'dotencode' not in repo.requirements:
        improvements.append(upgradeimprovement(
            name='dotencode',
            type=deficiency,
            description=_('storage of filenames beginning with a period or '
                          'space may not work correctly'),
            upgrademessage=_('repository will be better able to store files '
                             'beginning with a space or period'),
            fromdefault=True,
            fromconfig='dotencode' in newreporeqs))

    if 'generaldelta' not in repo.requirements:
        improvements.append(upgradeimprovement(
            name='generaldelta',
            type=deficiency,
            description=_('deltas within internal storage are unable to '
                          'choose optimal revisions; repository is larger and '
                          'slower than it could be; interaction with other '
                          'repositories may require extra network and CPU '
                          'resources, making "hg push" and "hg pull" slower'),
            upgrademessage=_('repository storage will be able to create '
                             'optimal deltas; new repository data will be '
                             'smaller and read times should decrease; '
                             'interacting with other repositories using this '
                             'storage model should require less network and '
                             'CPU resources, making "hg push" and "hg pull" '
                             'faster'),
            fromdefault=True,
            fromconfig='generaldelta' in newreporeqs))

    # Mercurial 4.0 changed changelogs to not use delta chains. Search for
    # changelogs with deltas.
    cl = repo.changelog
    for rev in cl:
        chainbase = cl.chainbase(rev)
        if chainbase != rev:
            improvements.append(upgradeimprovement(
                name='removecldeltachain',
                type=deficiency,
                description=_('changelog storage is using deltas instead of '
                              'raw entries; changelog reading and any '
                              'operation relying on changelog data are slower '
                              'than they could be'),
                upgrademessage=_('changelog storage will be reformated to '
                                 'store raw entries; changelog reading will be '
                                 'faster; changelog size may be reduced'),
                fromdefault=True,
                fromconfig=True))
            break

    # Now for the optimizations.

    # These are unconditionally added. There is logic later that figures out
    # which ones to apply.

    improvements.append(upgradeimprovement(
        name='redeltaparent',
        type=optimisation,
        description=_('deltas within internal storage will be recalculated to '
                      'choose an optimal base revision where this was not '
                      'already done; the size of the repository may shrink and '
                      'various operations may become faster; the first time '
                      'this optimization is performed could slow down upgrade '
                      'execution considerably; subsequent invocations should '
                      'not run noticeably slower'),
        upgrademessage=_('deltas within internal storage will choose a new '
                         'base revision if needed')))

    improvements.append(upgradeimprovement(
        name='redeltamultibase',
        type=optimisation,
        description=_('deltas within internal storage will be recalculated '
                      'against multiple base revision and the smallest '
                      'difference will be used; the size of the repository may '
                      'shrink significantly when there are many merges; this '
                      'optimization will slow down execution in proportion to '
                      'the number of merges in the repository and the amount '
                      'of files in the repository; this slow down should not '
                      'be significant unless there are tens of thousands of '
                      'files and thousands of merges'),
        upgrademessage=_('deltas within internal storage will choose an '
                         'optimal delta by computing deltas against multiple '
                         'parents; may slow down execution time '
                         'significantly')))

    improvements.append(upgradeimprovement(
        name='redeltaall',
        type=optimisation,
        description=_('deltas within internal storage will always be '
                      'recalculated without reusing prior deltas; this will '
                      'likely make execution run several times slower; this '
                      'optimization is typically not needed'),
        upgrademessage=_('deltas within internal storage will be fully '
                         'recomputed; this will likely drastically slow down '
                         'execution time')))

    return improvements

def upgradedetermineactions(repo, improvements, sourcereqs, destreqs,
                            optimize):
    """Determine upgrade actions that will be performed.

    Given a list of improvements as returned by ``upgradefindimprovements``,
    determine the list of upgrade actions that will be performed.

    The role of this function is to filter improvements if needed, apply
    recommended optimizations from the improvements list that make sense,
    etc.

    Returns a list of action names.
    """
    newactions = []

    knownreqs = upgradesupporteddestrequirements(repo)

    for i in improvements:
        name = i.name

        # If the action is a requirement that doesn't show up in the
        # destination requirements, prune the action.
        if name in knownreqs and name not in destreqs:
            continue

        if i.type == deficiency:
            newactions.append(name)

    newactions.extend(o for o in sorted(optimize) if o not in newactions)

    # FUTURE consider adding some optimizations here for certain transitions.
    # e.g. adding generaldelta could schedule parent redeltas.

    return newactions

def _revlogfrompath(repo, path):
    """Obtain a revlog from a repo path.

    An instance of the appropriate class is returned.
    """
    if path == '00changelog.i':
        return changelog.changelog(repo.svfs)
    elif path.endswith('00manifest.i'):
        mandir = path[:-len('00manifest.i')]
        return manifest.manifestrevlog(repo.svfs, dir=mandir)
    else:
        # Filelogs don't do anything special with settings. So we can use a
        # vanilla revlog.
        return revlog.revlog(repo.svfs, path)

def _copyrevlogs(ui, srcrepo, dstrepo, tr, deltareuse, aggressivemergedeltas):
    """Copy revlogs between 2 repos."""
    revcount = 0
    srcsize = 0
    srcrawsize = 0
    dstsize = 0
    fcount = 0
    frevcount = 0
    fsrcsize = 0
    frawsize = 0
    fdstsize = 0
    mcount = 0
    mrevcount = 0
    msrcsize = 0
    mrawsize = 0
    mdstsize = 0
    crevcount = 0
    csrcsize = 0
    crawsize = 0
    cdstsize = 0

    # Perform a pass to collect metadata. This validates we can open all
    # source files and allows a unified progress bar to be displayed.
    for unencoded, encoded, size in srcrepo.store.walk():
        if unencoded.endswith('.d'):
            continue

        rl = _revlogfrompath(srcrepo, unencoded)
        revcount += len(rl)

        datasize = 0
        rawsize = 0
        idx = rl.index
        for rev in rl:
            e = idx[rev]
            datasize += e[1]
            rawsize += e[2]

        srcsize += datasize
        srcrawsize += rawsize

        # This is for the separate progress bars.
        if isinstance(rl, changelog.changelog):
            crevcount += len(rl)
            csrcsize += datasize
            crawsize += rawsize
        elif isinstance(rl, manifest.manifestrevlog):
            mcount += 1
            mrevcount += len(rl)
            msrcsize += datasize
            mrawsize += rawsize
        elif isinstance(rl, revlog.revlog):
            fcount += 1
            frevcount += len(rl)
            fsrcsize += datasize
            frawsize += rawsize

    if not revcount:
        return

    ui.write(_('migrating %d total revisions (%d in filelogs, %d in manifests, '
               '%d in changelog)\n') %
             (revcount, frevcount, mrevcount, crevcount))
    ui.write(_('migrating %s in store; %s tracked data\n') % (
             (util.bytecount(srcsize), util.bytecount(srcrawsize))))

    # Used to keep track of progress.
    progress = []
    def oncopiedrevision(rl, rev, node):
        progress[1] += 1
        srcrepo.ui.progress(progress[0], progress[1], total=progress[2])

    # Do the actual copying.
    # FUTURE this operation can be farmed off to worker processes.
    seen = set()
    for unencoded, encoded, size in srcrepo.store.walk():
        if unencoded.endswith('.d'):
            continue

        oldrl = _revlogfrompath(srcrepo, unencoded)
        newrl = _revlogfrompath(dstrepo, unencoded)

        if isinstance(oldrl, changelog.changelog) and 'c' not in seen:
            ui.write(_('finished migrating %d manifest revisions across %d '
                       'manifests; change in size: %s\n') %
                     (mrevcount, mcount, util.bytecount(mdstsize - msrcsize)))

            ui.write(_('migrating changelog containing %d revisions '
                       '(%s in store; %s tracked data)\n') %
                     (crevcount, util.bytecount(csrcsize),
                      util.bytecount(crawsize)))
            seen.add('c')
            progress[:] = [_('changelog revisions'), 0, crevcount]
        elif isinstance(oldrl, manifest.manifestrevlog) and 'm' not in seen:
            ui.write(_('finished migrating %d filelog revisions across %d '
                       'filelogs; change in size: %s\n') %
                     (frevcount, fcount, util.bytecount(fdstsize - fsrcsize)))

            ui.write(_('migrating %d manifests containing %d revisions '
                       '(%s in store; %s tracked data)\n') %
                     (mcount, mrevcount, util.bytecount(msrcsize),
                      util.bytecount(mrawsize)))
            seen.add('m')
            progress[:] = [_('manifest revisions'), 0, mrevcount]
        elif 'f' not in seen:
            ui.write(_('migrating %d filelogs containing %d revisions '
                       '(%s in store; %s tracked data)\n') %
                     (fcount, frevcount, util.bytecount(fsrcsize),
                      util.bytecount(frawsize)))
            seen.add('f')
            progress[:] = [_('file revisions'), 0, frevcount]

        ui.progress(progress[0], progress[1], total=progress[2])

        ui.note(_('cloning %d revisions from %s\n') % (len(oldrl), unencoded))
        oldrl.clone(tr, newrl, addrevisioncb=oncopiedrevision,
                    deltareuse=deltareuse,
                    aggressivemergedeltas=aggressivemergedeltas)

        datasize = 0
        idx = newrl.index
        for rev in newrl:
            datasize += idx[rev][1]

        dstsize += datasize

        if isinstance(newrl, changelog.changelog):
            cdstsize += datasize
        elif isinstance(newrl, manifest.manifestrevlog):
            mdstsize += datasize
        else:
            fdstsize += datasize

    ui.progress(progress[0], None)

    ui.write(_('finished migrating %d changelog revisions; change in size: '
               '%s\n') % (crevcount, util.bytecount(cdstsize - csrcsize)))

    ui.write(_('finished migrating %d total revisions; total change in store '
               'size: %s\n') % (revcount, util.bytecount(dstsize - srcsize)))

def _upgradefilterstorefile(srcrepo, dstrepo, requirements, path, mode, st):
    """Determine whether to copy a store file during upgrade.

    This function is called when migrating store files from ``srcrepo`` to
    ``dstrepo`` as part of upgrading a repository.

    Args:
      srcrepo: repo we are copying from
      dstrepo: repo we are copying to
      requirements: set of requirements for ``dstrepo``
      path: store file being examined
      mode: the ``ST_MODE`` file type of ``path``
      st: ``stat`` data structure for ``path``

    Function should return ``True`` if the file is to be copied.
    """
    # Skip revlogs.
    if path.endswith(('.i', '.d')):
        return False
    # Skip transaction related files.
    if path.startswith('undo'):
        return False
    # Only copy regular files.
    if mode != stat.S_IFREG:
        return False
    # Skip other skipped files.
    if path in ('lock', 'fncache'):
        return False

    return True

def _upgradefinishdatamigration(ui, srcrepo, dstrepo, requirements):
    """Hook point for extensions to perform additional actions during upgrade.

    This function is called after revlogs and store files have been copied but
    before the new store is swapped into the original location.
    """

def _upgraderepo(ui, srcrepo, dstrepo, requirements, actions):
    """Do the low-level work of upgrading a repository.

    The upgrade is effectively performed as a copy between a source
    repository and a temporary destination repository.

    The source repository is unmodified for as long as possible so the
    upgrade can abort at any time without causing loss of service for
    readers and without corrupting the source repository.
    """
    assert srcrepo.currentwlock()
    assert dstrepo.currentwlock()

    ui.write(_('(it is safe to interrupt this process any time before '
               'data migration completes)\n'))

    if 'redeltaall' in actions:
        deltareuse = revlog.revlog.DELTAREUSENEVER
    elif 'redeltaparent' in actions:
        deltareuse = revlog.revlog.DELTAREUSESAMEREVS
    elif 'redeltamultibase' in actions:
        deltareuse = revlog.revlog.DELTAREUSESAMEREVS
    else:
        deltareuse = revlog.revlog.DELTAREUSEALWAYS

    with dstrepo.transaction('upgrade') as tr:
        _copyrevlogs(ui, srcrepo, dstrepo, tr, deltareuse,
                     'redeltamultibase' in actions)

    # Now copy other files in the store directory.
    for p, kind, st in srcrepo.store.vfs.readdir('', stat=True):
        if not _upgradefilterstorefile(srcrepo, dstrepo, requirements,
                                       p, kind, st):
            continue

        srcrepo.ui.write(_('copying %s\n') % p)
        src = srcrepo.store.vfs.join(p)
        dst = dstrepo.store.vfs.join(p)
        util.copyfile(src, dst, copystat=True)

    _upgradefinishdatamigration(ui, srcrepo, dstrepo, requirements)

    ui.write(_('data fully migrated to temporary repository\n'))

    backuppath = tempfile.mkdtemp(prefix='upgradebackup.', dir=srcrepo.path)
    backupvfs = vfsmod.vfs(backuppath)

    # Make a backup of requires file first, as it is the first to be modified.
    util.copyfile(srcrepo.vfs.join('requires'), backupvfs.join('requires'))

    # We install an arbitrary requirement that clients must not support
    # as a mechanism to lock out new clients during the data swap. This is
    # better than allowing a client to continue while the repository is in
    # an inconsistent state.
    ui.write(_('marking source repository as being upgraded; clients will be '
               'unable to read from repository\n'))
    scmutil.writerequires(srcrepo.vfs,
                          srcrepo.requirements | set(['upgradeinprogress']))

    ui.write(_('starting in-place swap of repository data\n'))
    ui.write(_('replaced files will be backed up at %s\n') %
             backuppath)

    # Now swap in the new store directory. Doing it as a rename should make
    # the operation nearly instantaneous and atomic (at least in well-behaved
    # environments).
    ui.write(_('replacing store...\n'))
    tstart = util.timer()
    util.rename(srcrepo.spath, backupvfs.join('store'))
    util.rename(dstrepo.spath, srcrepo.spath)
    elapsed = util.timer() - tstart
    ui.write(_('store replacement complete; repository was inconsistent for '
               '%0.1fs\n') % elapsed)

    # We first write the requirements file. Any new requirements will lock
    # out legacy clients.
    ui.write(_('finalizing requirements file and making repository readable '
               'again\n'))
    scmutil.writerequires(srcrepo.vfs, requirements)

    # The lock file from the old store won't be removed because nothing has a
    # reference to its new location. So clean it up manually. Alternatively, we
    # could update srcrepo.svfs and other variables to point to the new
    # location. This is simpler.
    backupvfs.unlink('store/lock')

    return backuppath

def upgraderepo(ui, repo, run=False, optimize=None):
    """Upgrade a repository in place."""
    # Avoid cycle: cmdutil -> repair -> localrepo -> cmdutil
    from . import localrepo

    optimize = set(optimize or [])
    repo = repo.unfiltered()

    # Ensure the repository can be upgraded.
    missingreqs = requiredsourcerequirements(repo) - repo.requirements
    if missingreqs:
        raise error.Abort(_('cannot upgrade repository; requirement '
                            'missing: %s') % _(', ').join(sorted(missingreqs)))

    blockedreqs = upgradeblocksourcerequirements(repo) & repo.requirements
    if blockedreqs:
        raise error.Abort(_('cannot upgrade repository; unsupported source '
                            'requirement: %s') %
                          _(', ').join(sorted(blockedreqs)))

    # FUTURE there is potentially a need to control the wanted requirements via
    # command arguments or via an extension hook point.
    newreqs = localrepo.newreporequirements(repo)

    noremovereqs = (repo.requirements - newreqs -
                   upgradesupportremovedrequirements(repo))
    if noremovereqs:
        raise error.Abort(_('cannot upgrade repository; requirement would be '
                            'removed: %s') % _(', ').join(sorted(noremovereqs)))

    noaddreqs = (newreqs - repo.requirements -
                 upgradeallowednewrequirements(repo))
    if noaddreqs:
        raise error.Abort(_('cannot upgrade repository; do not support adding '
                            'requirement: %s') %
                          _(', ').join(sorted(noaddreqs)))

    unsupportedreqs = newreqs - upgradesupporteddestrequirements(repo)
    if unsupportedreqs:
        raise error.Abort(_('cannot upgrade repository; do not support '
                            'destination requirement: %s') %
                          _(', ').join(sorted(unsupportedreqs)))

    # Find and validate all improvements that can be made.
    improvements = upgradefindimprovements(repo)
    for i in improvements:
        if i.type not in (deficiency, optimisation):
            raise error.Abort(_('unexpected improvement type %s for %s') % (
                i.type, i.name))

    # Validate arguments.
    unknownoptimize = optimize - set(i.name for i in improvements
                                     if i.type == optimisation)
    if unknownoptimize:
        raise error.Abort(_('unknown optimization action requested: %s') %
                          ', '.join(sorted(unknownoptimize)),
                          hint=_('run without arguments to see valid '
                                 'optimizations'))

    actions = upgradedetermineactions(repo, improvements, repo.requirements,
                                      newreqs, optimize)

    def printrequirements():
        ui.write(_('requirements\n'))
        ui.write(_('   preserved: %s\n') %
                 _(', ').join(sorted(newreqs & repo.requirements)))

        if repo.requirements - newreqs:
            ui.write(_('   removed: %s\n') %
                     _(', ').join(sorted(repo.requirements - newreqs)))

        if newreqs - repo.requirements:
            ui.write(_('   added: %s\n') %
                     _(', ').join(sorted(newreqs - repo.requirements)))

        ui.write('\n')

    def printupgradeactions():
        for action in actions:
            for i in improvements:
                if i.name == action:
                    ui.write('%s\n   %s\n\n' %
                             (i.name, i.upgrademessage))

    if not run:
        fromdefault = []
        fromconfig = []
        optimizations = []

        for i in improvements:
            assert i.type in (deficiency, optimisation)
            if i.type == deficiency:
                if i.fromdefault:
                    fromdefault.append(i)
                if i.fromconfig:
                    fromconfig.append(i)
            else:
                optimizations.append(i)

        if fromdefault or fromconfig:
            fromconfignames = set(x.name for x in fromconfig)
            onlydefault = [i for i in fromdefault
                           if i.name not in fromconfignames]

            if fromconfig:
                ui.write(_('repository lacks features recommended by '
                           'current config options:\n\n'))
                for i in fromconfig:
                    ui.write('%s\n   %s\n\n' % (i.name, i.description))

            if onlydefault:
                ui.write(_('repository lacks features used by the default '
                           'config options:\n\n'))
                for i in onlydefault:
                    ui.write('%s\n   %s\n\n' % (i.name, i.description))

            ui.write('\n')
        else:
            ui.write(_('(no feature deficiencies found in existing '
                       'repository)\n'))

        ui.write(_('performing an upgrade with "--run" will make the following '
                   'changes:\n\n'))

        printrequirements()
        printupgradeactions()

        unusedoptimize = [i for i in improvements
                          if i.name not in actions and i.type == optimisation]
        if unusedoptimize:
            ui.write(_('additional optimizations are available by specifying '
                     '"--optimize <name>":\n\n'))
            for i in unusedoptimize:
                ui.write(_('%s\n   %s\n\n') % (i.name, i.description))
        return

    # Else we're in the run=true case.
    ui.write(_('upgrade will perform the following actions:\n\n'))
    printrequirements()
    printupgradeactions()

    ui.write(_('beginning upgrade...\n'))
    with repo.wlock():
        with repo.lock():
            ui.write(_('repository locked and read-only\n'))
            # Our strategy for upgrading the repository is to create a new,
            # temporary repository, write data to it, then do a swap of the
            # data. There are less heavyweight ways to do this, but it is easier
            # to create a new repo object than to instantiate all the components
            # (like the store) separately.
            tmppath = tempfile.mkdtemp(prefix='upgrade.', dir=repo.path)
            backuppath = None
            try:
                ui.write(_('creating temporary repository to stage migrated '
                           'data: %s\n') % tmppath)
                dstrepo = localrepo.localrepository(repo.baseui,
                                                    path=tmppath,
                                                    create=True)

                with dstrepo.wlock():
                    with dstrepo.lock():
                        backuppath = _upgraderepo(ui, repo, dstrepo, newreqs,
                                                  actions)

            finally:
                ui.write(_('removing temporary repository %s\n') % tmppath)
                repo.vfs.rmtree(tmppath, forcibly=True)

                if backuppath:
                    ui.warn(_('copy of old repository backed up at %s\n') %
                            backuppath)
                    ui.warn(_('the old repository will not be deleted; remove '
                              'it to free up disk space once the upgraded '
                              'repository is verified\n'))
