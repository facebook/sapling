# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# upgrade.py - functions for in place upgrade of Mercurial repository
#
# Copyright (c) 2016-present, Gregory Szorc
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import stat
import tempfile

from . import (
    changelog,
    error,
    filelog,
    hg,
    localrepo,
    manifest,
    progress,
    revlog,
    scmutil,
    util,
    vfs as vfsmod,
)
from .i18n import _


def requiredsourcerequirements(repo):
    """Obtain requirements required to be present to upgrade a repo.

    An upgrade will not be allowed if the repository doesn't have the
    requirements returned by this function.
    """
    return {
        # Introduced in Mercurial 0.9.2.
        "revlogv1",
        # Introduced in Mercurial 0.9.2.
        "store",
    }


def blocksourcerequirements(repo):
    """Obtain requirements that will prevent an upgrade from occurring.

    An upgrade cannot be performed if the source repository contains a
    requirements in the returned set.
    """
    return {
        # The upgrade code does not yet support these experimental features.
        # This is an artificial limitation.
        "treemanifest",
        # This was a precursor to generaldelta and was never enabled by default.
        # It should (hopefully) not exist in the wild.
        "parentdelta",
        # Upgrade should operate on the actual store, not the shared link.
        "shared",
    }


def supportremovedrequirements(repo):
    """Obtain requirements that can be removed during an upgrade.

    If an upgrade were to create a repository that dropped a requirement,
    the dropped requirement must appear in the returned set for the upgrade
    to be allowed.
    """
    return set()


def supporteddestrequirements(repo):
    """Obtain requirements that upgrade supports in the destination.

    If the result of the upgrade would create requirements not in this set,
    the upgrade is disallowed.

    Extensions should monkeypatch this to add their custom requirements.
    """
    return {
        "dotencode",
        "fncache",
        "generaldelta",
        "revlogv1",
        "store",
        "storerequirements",
    }


def allowednewrequirements(repo):
    """Obtain requirements that can be added to a repository during upgrade.

    This is used to disallow proposed requirements from being added when
    they weren't present before.

    We use a list of allowed requirement additions instead of a list of known
    bad additions because the whitelist approach is safer and will prevent
    future, unknown requirements from accidentally being added.
    """
    return {"dotencode", "fncache", "generaldelta", "storerequirements"}


def preservedrequirements(repo):
    return set()


deficiency = "deficiency"
optimisation = "optimization"


class improvement(object):
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
    """

    def __init__(self, name, type, description, upgrademessage):
        self.name = name
        self.type = type
        self.description = description
        self.upgrademessage = upgrademessage

    def __eq__(self, other):
        if not isinstance(other, improvement):
            # This is what python tell use to do
            return NotImplemented
        return self.name == other.name

    def __ne__(self, other):
        return not self == other

    def __hash__(self):
        return hash(self.name)


allformatvariant = []


def registerformatvariant(cls):
    allformatvariant.append(cls)
    return cls


class formatvariant(improvement):
    """an improvement subclass dedicated to repository format"""

    type = deficiency
    ### The following attributes should be defined for each class:

    # machine-readable string uniquely identifying this improvement. it will be
    # mapped to an action later in the upgrade process.
    name = None

    # message intended for humans explaining the improvement in more detail,
    # including the implications of it ``deficiency`` types, should be worded
    # in the present tense.
    description = None

    # message intended for humans explaining what an upgrade addressing this
    # issue will do. should be worded in the future tense.
    upgrademessage = None

    # value of current Mercurial default for new repository
    default = None

    def __init__(self):
        raise NotImplementedError()

    @staticmethod
    def fromrepo(repo):
        """current value of the variant in the repository"""
        raise NotImplementedError()

    @staticmethod
    def fromconfig(repo):
        """current value of the variant in the configuration"""
        raise NotImplementedError()


class requirementformatvariant(formatvariant):
    """formatvariant based on a 'requirement' name.

    Many format variant are controlled by a 'requirement'. We define a small
    subclass to factor the code.
    """

    # the requirement that control this format variant
    _requirement = None

    @staticmethod
    def _newreporequirements(repo):
        return localrepo.newreporequirements(repo)

    @classmethod
    def fromrepo(cls, repo):
        assert cls._requirement is not None
        return cls._requirement in repo.requirements

    @classmethod
    def fromconfig(cls, repo):
        assert cls._requirement is not None
        return cls._requirement in cls._newreporequirements(repo)


@registerformatvariant
class fncache(requirementformatvariant):
    # pyre-fixme[15]: `name` overrides attribute defined in `formatvariant`
    #  inconsistently.
    name = "fncache"

    # pyre-fixme[15]: `_requirement` overrides attribute defined in
    #  `requirementformatvariant` inconsistently.
    _requirement = "fncache"

    # pyre-fixme[15]: `default` overrides attribute defined in `formatvariant`
    #  inconsistently.
    default = True

    description = _(
        "long and reserved filenames may not work correctly; "
        "repository performance is sub-optimal"
    )

    upgrademessage = _(
        "repository will be more resilient to storing "
        "certain paths and performance of certain "
        "operations should be improved"
    )


@registerformatvariant
class dotencode(requirementformatvariant):
    # pyre-fixme[15]: `name` overrides attribute defined in `formatvariant`
    #  inconsistently.
    name = "dotencode"

    # pyre-fixme[15]: `_requirement` overrides attribute defined in
    #  `requirementformatvariant` inconsistently.
    _requirement = "dotencode"

    # pyre-fixme[15]: `default` overrides attribute defined in `formatvariant`
    #  inconsistently.
    default = True

    description = _(
        "storage of filenames beginning with a period or "
        "space may not work correctly"
    )

    upgrademessage = _(
        "repository will be better able to store files "
        "beginning with a space or period"
    )


@registerformatvariant
class generaldelta(requirementformatvariant):
    # pyre-fixme[15]: `name` overrides attribute defined in `formatvariant`
    #  inconsistently.
    name = "generaldelta"

    # pyre-fixme[15]: `_requirement` overrides attribute defined in
    #  `requirementformatvariant` inconsistently.
    _requirement = "generaldelta"

    # pyre-fixme[15]: `default` overrides attribute defined in `formatvariant`
    #  inconsistently.
    default = True

    description = _(
        "deltas within internal storage are unable to "
        "choose optimal revisions; repository is larger and "
        "slower than it could be; interaction with other "
        "repositories may require extra network and CPU "
        'resources, making "hg push" and "hg pull" slower'
    )

    upgrademessage = _(
        "repository storage will be able to create "
        "optimal deltas; new repository data will be "
        "smaller and read times should decrease; "
        "interacting with other repositories using this "
        "storage model should require less network and "
        'CPU resources, making "hg push" and "hg pull" '
        "faster"
    )


@registerformatvariant
class removecldeltachain(formatvariant):
    # pyre-fixme[15]: `name` overrides attribute defined in `formatvariant`
    #  inconsistently.
    name = "plain-cl-delta"

    # pyre-fixme[15]: `default` overrides attribute defined in `formatvariant`
    #  inconsistently.
    default = True

    description = _(
        "changelog storage is using deltas instead of "
        "raw entries; changelog reading and any "
        "operation relying on changelog data are slower "
        "than they could be"
    )

    upgrademessage = _(
        "changelog storage will be reformated to "
        "store raw entries; changelog reading will be "
        "faster; changelog size may be reduced"
    )

    @staticmethod
    def fromrepo(repo):
        # Mercurial 4.0 changed changelogs to not use delta chains. Search for
        # changelogs with deltas.
        cl = repo.changelog
        chainbase = cl.chainbase
        return all(rev == chainbase(rev) for rev in cl)

    @staticmethod
    def fromconfig(repo):
        return True


@registerformatvariant
class compressionengine(formatvariant):
    # pyre-fixme[15]: `name` overrides attribute defined in `formatvariant`
    #  inconsistently.
    name = "compression"
    # pyre-fixme[15]: `default` overrides attribute defined in `formatvariant`
    #  inconsistently.
    default = "zlib"

    description = _(
        "Compression algorithm used to compress data. "
        "Some engine are faster than other"
    )

    upgrademessage = _("revlog content will be recompressed with the new " "algorithm.")

    @classmethod
    def fromrepo(cls, repo):
        for req in repo.requirements:
            if req.startswith("exp-compression-"):
                return req.split("-", 2)[2]
        return "zlib"

    @classmethod
    def fromconfig(cls, repo):
        return repo.ui.config("experimental", "format.compression")


def finddeficiencies(repo):
    """returns a list of deficiencies that the repo suffer from"""
    deficiencies = []

    # We could detect lack of revlogv1 and store here, but they were added
    # in 0.9.2 and we don't support upgrading repos without these
    # requirements, so let's not bother.

    for fv in allformatvariant:
        if not fv.fromrepo(repo):
            deficiencies.append(fv)

    return deficiencies


def findoptimizations(repo):
    """Determine optimisation that could be used during upgrade"""
    # These are unconditionally added. There is logic later that figures out
    # which ones to apply.
    optimizations = []

    optimizations.append(
        improvement(
            name="redeltaparent",
            type=optimisation,
            description=_(
                "deltas within internal storage will be recalculated to "
                "choose an optimal base revision where this was not "
                "already done; the size of the repository may shrink and "
                "various operations may become faster; the first time "
                "this optimization is performed could slow down upgrade "
                "execution considerably; subsequent invocations should "
                "not run noticeably slower"
            ),
            upgrademessage=_(
                "deltas within internal storage will choose a new "
                "base revision if needed"
            ),
        )
    )

    optimizations.append(
        improvement(
            name="redeltamultibase",
            type=optimisation,
            description=_(
                "deltas within internal storage will be recalculated "
                "against multiple base revision and the smallest "
                "difference will be used; the size of the repository may "
                "shrink significantly when there are many merges; this "
                "optimization will slow down execution in proportion to "
                "the number of merges in the repository and the amount "
                "of files in the repository; this slow down should not "
                "be significant unless there are tens of thousands of "
                "files and thousands of merges"
            ),
            upgrademessage=_(
                "deltas within internal storage will choose an "
                "optimal delta by computing deltas against multiple "
                "parents; may slow down execution time "
                "significantly"
            ),
        )
    )

    optimizations.append(
        improvement(
            name="redeltaall",
            type=optimisation,
            description=_(
                "deltas within internal storage will always be "
                "recalculated without reusing prior deltas; this will "
                "likely make execution run several times slower; this "
                "optimization is typically not needed"
            ),
            upgrademessage=_(
                "deltas within internal storage will be fully "
                "recomputed; this will likely drastically slow down "
                "execution time"
            ),
        )
    )

    optimizations.append(
        improvement(
            name="redeltafulladd",
            type=optimisation,
            description=_(
                "every revision will be re-added as if it was new "
                "content. It will go through the full storage "
                "mechanism giving extensions a chance to process it "
                '(eg. lfs). This is similar to "redeltaall" but even '
                "slower since more logic is involved."
            ),
            upgrademessage=_(
                "each revision will be added as new content to the "
                "internal storage; this will likely drastically slow "
                "down execution time, but some extensions might need "
                "it"
            ),
        )
    )

    return optimizations


def determineactions(repo, deficiencies, sourcereqs, destreqs):
    """Determine upgrade actions that will be performed.

    Given a list of improvements as returned by ``finddeficiencies`` and
    ``findoptimizations``, determine the list of upgrade actions that
    will be performed.

    The role of this function is to filter improvements if needed, apply
    recommended optimizations from the improvements list that make sense,
    etc.

    Returns a list of action names.
    """
    newactions = []

    knownreqs = supporteddestrequirements(repo)

    for d in deficiencies:
        name = d.name

        # If the action is a requirement that doesn't show up in the
        # destination requirements, prune the action.
        if name in knownreqs and name not in destreqs:
            continue

        newactions.append(d)

    # FUTURE consider adding some optimizations here for certain transitions.
    # e.g. adding generaldelta could schedule parent redeltas.

    return newactions


def _revlogfrompath(repo, path):
    """Obtain a revlog from a repo path.

    An instance of the appropriate class is returned.
    """
    if path == "00changelog.i":
        return changelog.changelog(repo.svfs, uiconfig=repo.ui.uiconfig())
    elif path.endswith("00manifest.i"):
        mandir = path[: -len("00manifest.i")]
        return manifest.manifestrevlog(repo.svfs, dir=mandir)
    else:
        # reverse of "/".join(("data", path + ".i"))
        return filelog.filelog(repo.svfs, path[5:-2])


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
        if unencoded.endswith(".d"):
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

    ui.write(
        _(
            "migrating %d total revisions (%d in filelogs, %d in manifests, "
            "%d in changelog)\n"
        )
        % (revcount, frevcount, mrevcount, crevcount)
    )
    ui.write(
        _("migrating %s in store; %s tracked data\n")
        % ((util.bytecount(srcsize), util.bytecount(srcrawsize)))
    )

    # Used to keep track of progress.
    prog = progress.bar(ui, _("migrating"))

    def oncopiedrevision(rl, rev, node):
        prog.value += 1

    # Do the actual copying.
    # FUTURE this operation can be farmed off to worker processes.
    seen = set()
    with prog:
        for unencoded, encoded, size in srcrepo.store.walk():
            if unencoded.endswith(".d"):
                continue

            oldrl = _revlogfrompath(srcrepo, unencoded)
            newrl = _revlogfrompath(dstrepo, unencoded)

            if isinstance(oldrl, changelog.changelog) and "c" not in seen:
                ui.write(
                    _(
                        "finished migrating %d manifest revisions across %d "
                        "manifests; change in size: %s\n"
                    )
                    % (mrevcount, mcount, util.bytecount(mdstsize - msrcsize))
                )

                ui.write(
                    _(
                        "migrating changelog containing %d revisions "
                        "(%s in store; %s tracked data)\n"
                    )
                    % (crevcount, util.bytecount(csrcsize), util.bytecount(crawsize))
                )
                seen.add("c")
                prog.reset(_("changelog revisions"), total=crevcount)
            elif isinstance(oldrl, manifest.manifestrevlog) and "m" not in seen:
                ui.write(
                    _(
                        "finished migrating %d filelog revisions across %d "
                        "filelogs; change in size: %s\n"
                    )
                    % (frevcount, fcount, util.bytecount(fdstsize - fsrcsize))
                )

                ui.write(
                    _(
                        "migrating %d manifests containing %d revisions "
                        "(%s in store; %s tracked data)\n"
                    )
                    % (
                        mcount,
                        mrevcount,
                        util.bytecount(msrcsize),
                        util.bytecount(mrawsize),
                    )
                )
                seen.add("m")
                prog.reset(_("manifest revisions"), total=crevcount)
            elif "f" not in seen:
                ui.write(
                    _(
                        "migrating %d filelogs containing %d revisions "
                        "(%s in store; %s tracked data)\n"
                    )
                    % (
                        fcount,
                        frevcount,
                        util.bytecount(fsrcsize),
                        util.bytecount(frawsize),
                    )
                )
                seen.add("f")
                prog.reset(_("file revisions"), total=crevcount)

            ui.note(_("cloning %d revisions from %s\n") % (len(oldrl), unencoded))
            oldrl.clone(
                tr,
                newrl,
                addrevisioncb=oncopiedrevision,
                deltareuse=deltareuse,
                aggressivemergedeltas=aggressivemergedeltas,
            )

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

    ui.write(
        _("finished migrating %d changelog revisions; change in size: " "%s\n")
        % (crevcount, util.bytecount(cdstsize - csrcsize))
    )

    ui.write(
        _("finished migrating %d total revisions; total change in store " "size: %s\n")
        % (revcount, util.bytecount(dstsize - srcsize))
    )


def _filterstorefile(srcrepo, dstrepo, requirements, path, mode, st):
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
    if path.endswith((".i", ".d")):
        return False
    # Skip transaction related files.
    if path.startswith("undo"):
        return False
    # Only copy regular files.
    if mode != stat.S_IFREG:
        return False
    # Skip other skipped files.
    if path in ("lock", "fncache"):
        return False

    return True


def _finishdatamigration(ui, srcrepo, dstrepo, requirements):
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

    ui.write(
        _(
            "(it is safe to interrupt this process any time before "
            "data migration completes)\n"
        )
    )

    if "redeltaall" in actions:
        deltareuse = revlog.revlog.DELTAREUSENEVER
    elif "redeltaparent" in actions:
        deltareuse = revlog.revlog.DELTAREUSESAMEREVS
    elif "redeltamultibase" in actions:
        deltareuse = revlog.revlog.DELTAREUSESAMEREVS
    if "redeltafulladd" in actions:
        deltareuse = revlog.revlog.DELTAREUSEFULLADD
    else:
        deltareuse = revlog.revlog.DELTAREUSEALWAYS

    with dstrepo.transaction("upgrade") as tr:
        _copyrevlogs(
            ui, srcrepo, dstrepo, tr, deltareuse, "redeltamultibase" in actions
        )

    # Now copy other files in the store directory.
    # The sorted() makes execution deterministic.
    for p, kind, st in sorted(srcrepo.store.vfs.readdir("", stat=True)):
        if not _filterstorefile(srcrepo, dstrepo, requirements, p, kind, st):
            continue

        srcrepo.ui.write(_("copying %s\n") % p)
        src = srcrepo.store.rawvfs.join(p)
        dst = dstrepo.store.rawvfs.join(p)
        util.copyfile(src, dst, copystat=True)

    _finishdatamigration(ui, srcrepo, dstrepo, requirements)

    ui.write(_("data fully migrated to temporary repository\n"))

    backuppath = tempfile.mkdtemp(prefix="upgradebackup.", dir=srcrepo.path)
    backupvfs = vfsmod.vfs(backuppath)

    # Make a backup of requires file first, as it is the first to be modified.
    util.copyfile(srcrepo.localvfs.join("requires"), backupvfs.join("requires"))

    # We install an arbitrary requirement that clients must not support
    # as a mechanism to lock out new clients during the data swap. This is
    # better than allowing a client to continue while the repository is in
    # an inconsistent state.
    ui.write(
        _(
            "marking source repository as being upgraded; clients will be "
            "unable to read from repository\n"
        )
    )
    scmutil.writerequires(
        srcrepo.localvfs, srcrepo.requirements | {"upgradeinprogress"}
    )

    ui.write(_("starting in-place swap of repository data\n"))
    ui.write(_("replaced files will be backed up at %s\n") % backuppath)

    # Now swap in the new store directory. Doing it as a rename should make
    # the operation nearly instantaneous and atomic (at least in well-behaved
    # environments).
    ui.write(_("replacing store...\n"))
    tstart = util.timer()
    util.rename(srcrepo.spath, backupvfs.join("store"))
    util.rename(dstrepo.spath, srcrepo.spath)
    elapsed = util.timer() - tstart
    ui.write(
        _("store replacement complete; repository was inconsistent for " "%0.1fs\n")
        % elapsed
    )

    # We first write the requirements file. Any new requirements will lock
    # out legacy clients.
    ui.write(
        _("finalizing requirements file and making repository readable " "again\n")
    )
    scmutil.writerequires(srcrepo.localvfs, requirements)

    # The lock file from the old store won't be removed because nothing has a
    # reference to its new location. So clean it up manually. Alternatively, we
    # could update srcrepo.svfs and other variables to point to the new
    # location. This is simpler.
    backupvfs.unlink("store/lock")

    return backuppath


def upgraderepo(ui, repo, run=False, optimize=None):
    """Upgrade a repository in place."""
    optimize = set(optimize or [])
    repo = repo.unfiltered()

    # Ensure the repository can be upgraded.
    missingreqs = requiredsourcerequirements(repo) - repo.requirements
    if missingreqs:
        raise error.Abort(
            _("cannot upgrade repository; requirement " "missing: %s")
            % _(", ").join(sorted(missingreqs))
        )

    blockedreqs = blocksourcerequirements(repo) & repo.requirements
    if blockedreqs:
        raise error.Abort(
            _("cannot upgrade repository; unsupported source " "requirement: %s")
            % _(", ").join(sorted(blockedreqs))
        )

    # FUTURE there is potentially a need to control the wanted requirements via
    # command arguments or via an extension hook point.
    newreqs = localrepo.newreporequirements(repo)
    newreqs.update(preservedrequirements(repo))

    noremovereqs = repo.requirements - newreqs - supportremovedrequirements(repo)
    if noremovereqs:
        raise error.Abort(
            _("cannot upgrade repository; requirement would be " "removed: %s")
            % _(", ").join(sorted(noremovereqs))
        )

    noaddreqs = newreqs - repo.requirements - allowednewrequirements(repo)
    if noaddreqs:
        raise error.Abort(
            _("cannot upgrade repository; do not support adding " "requirement: %s")
            % _(", ").join(sorted(noaddreqs))
        )

    unsupportedreqs = newreqs - supporteddestrequirements(repo)
    if unsupportedreqs:
        raise error.Abort(
            _(
                "cannot upgrade repository; do not support "
                "destination requirement: %s"
            )
            % _(", ").join(sorted(unsupportedreqs))
        )

    # Find and validate all improvements that can be made.
    alloptimizations = findoptimizations(repo)

    # Apply and Validate arguments.
    optimizations = []
    for o in alloptimizations:
        if o.name in optimize:
            optimizations.append(o)
            optimize.discard(o.name)

    if optimize:  # anything left is unknown
        raise error.Abort(
            _("unknown optimization action requested: %s")
            % ", ".join(sorted(optimize)),
            hint=_("run without arguments to see valid " "optimizations"),
        )

    deficiencies = finddeficiencies(repo)
    actions = determineactions(repo, deficiencies, repo.requirements, newreqs)
    actions.extend(
        o
        for o in sorted(optimizations)
        # determineactions could have added optimisation
        if o not in actions
    )

    def printrequirements():
        ui.write(_("requirements\n"))
        ui.write(
            _("   preserved: %s\n") % _(", ").join(sorted(newreqs & repo.requirements))
        )

        if repo.requirements - newreqs:
            ui.write(
                _("   removed: %s\n")
                % _(", ").join(sorted(repo.requirements - newreqs))
            )

        if newreqs - repo.requirements:
            ui.write(
                _("   added: %s\n") % _(", ").join(sorted(newreqs - repo.requirements))
            )

        ui.write("\n")

    def printupgradeactions():
        for a in actions:
            ui.write("%s\n   %s\n\n" % (a.name, a.upgrademessage))

    if not run:
        fromconfig = []
        onlydefault = []

        for d in deficiencies:
            if d.fromconfig(repo):
                fromconfig.append(d)
            elif d.default:
                onlydefault.append(d)

        if fromconfig or onlydefault:

            if fromconfig:
                ui.write(
                    _(
                        "repository lacks features recommended by "
                        "current config options:\n\n"
                    )
                )
                for i in fromconfig:
                    ui.write("%s\n   %s\n\n" % (i.name, i.description))

            if onlydefault:
                ui.write(
                    _(
                        "repository lacks features used by the default "
                        "config options:\n\n"
                    )
                )
                for i in onlydefault:
                    ui.write("%s\n   %s\n\n" % (i.name, i.description))

            ui.write("\n")
        else:
            ui.write(_("(no feature deficiencies found in existing " "repository)\n"))

        ui.write(
            _(
                'performing an upgrade with "--run" will make the following '
                "changes:\n\n"
            )
        )

        printrequirements()
        printupgradeactions()

        unusedoptimize = [i for i in alloptimizations if i not in actions]

        if unusedoptimize:
            ui.write(
                _(
                    "additional optimizations are available by specifying "
                    '"--optimize <name>":\n\n'
                )
            )
            for i in unusedoptimize:
                ui.write(_("%s\n   %s\n\n") % (i.name, i.description))
        return

    # Else we're in the run=true case.
    ui.write(_("upgrade will perform the following actions:\n\n"))
    printrequirements()
    printupgradeactions()

    upgradeactions = [a.name for a in actions]

    ui.write(_("beginning upgrade...\n"))
    with repo.wlock(), repo.lock():
        ui.write(_("repository locked and read-only\n"))
        # Our strategy for upgrading the repository is to create a new,
        # temporary repository, write data to it, then do a swap of the
        # data. There are less heavyweight ways to do this, but it is easier
        # to create a new repo object than to instantiate all the components
        # (like the store) separately.
        tmppath = tempfile.mkdtemp(prefix="upgrade.", dir=repo.path)
        backuppath = None
        try:
            ui.write(
                _("creating temporary repository to stage migrated " "data: %s\n")
                % tmppath
            )

            # clone ui without using ui.copy because repo.ui is protected
            repoui = repo.ui.__class__(repo.ui)
            dstrepo = hg.repository(repoui, path=tmppath, create=True)

            with dstrepo.wlock(), dstrepo.lock():
                backuppath = _upgraderepo(ui, repo, dstrepo, newreqs, upgradeactions)

        finally:
            ui.write(_("removing temporary repository %s\n") % tmppath)
            repo.localvfs.rmtree(tmppath, forcibly=True)

            if backuppath:
                ui.warn(_("copy of old repository backed up at %s\n") % backuppath)
                ui.warn(
                    _(
                        "the old repository will not be deleted; remove "
                        "it to free up disk space once the upgraded "
                        "repository is verified\n"
                    )
                )
