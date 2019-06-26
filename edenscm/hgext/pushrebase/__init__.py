# pushrebase.py - server-side rebasing of pushed changesets
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""rebases commits during push

The pushrebase extension allows the server to rebase incoming commits as part of
the push process. This helps solve the problem of push contention where many
clients try to push at once and all but one fail. Instead of failing, the
pushrebase extension will rebase the incoming commit onto the target bookmark
(i.e. @ or master) as long as the commit doesn't touch any files that have been
modified in the target bookmark. Put another way, pushrebase will not perform
any file content merges. It only performs the rebase when there is no chance of
a file merge.

Configs:

    ``pushrebase.forcetreereceive`` forces pushrebase to read incoming
    treemanifests instead of incoming flat manifests. This is useful for the
    transition to treemanifest.

    ``pushrebase.trystackpush`` use potentially faster "stackpush" code path
    if possible.

    ``pushrebase.verbose`` print verbose messages from the server.

    ``pushrebase.enablerecording`` whether to enable the recording of pushrebase
    requests.

    ``pushrebase.bundlepartuploadbinary`` binary and command line arguments that
    will be called to upload bundle2 part. One of the arguments should contain
    '{filename}' to specify a filename with a bundle2 part. It should return
    a handle, that can later be used to access the part. Note: handles MUST NOT
    contain whitespaces.

    ``pushrebase.recordingrepoid`` id of the repo for the pushrebase recording

    ``pushrebase.recordingsqlargs`` sql arguments for the pushrebase recording

    ``pushrebase.syncondispatch`` perform a full SQL sync when receiving pushes

    ``pushrebase.commitdatesfile`` is a file with map {commit hash -> timestamp}
    in a json format.
"""
from __future__ import absolute_import

import errno
import json
import mmap
import os
import tempfile
import time

from edenscm.mercurial import (
    bundle2,
    changegroup,
    commands,
    context,
    discovery,
    encoding,
    error,
    exchange,
    extensions,
    hg,
    manifest,
    mutation,
    obsolete,
    phases as phasesmod,
    pushkey,
    registrar,
    revsetlang,
    scmutil,
    util,
    visibility,
    wireproto,
)
from edenscm.mercurial.extensions import unwrapfunction, wrapcommand, wrapfunction
from edenscm.mercurial.i18n import _, _n
from edenscm.mercurial.node import bin, hex, nullid, nullrev, short

from . import common, recording, stackpush
from .. import hgsql
from ..remotefilelog import (
    contentstore,
    datapack,
    historypack,
    metadatastore,
    mutablestores,
    shallowbundle,
    wirepack,
)
from .errors import ConflictsError, StackPushUnsupportedError


testedwith = "ships-with-fb-hgext"

cmdtable = {}
command = registrar.command(cmdtable)

configtable = {}
configitem = registrar.configitem(configtable)

configitem("pushrebase", "blocknonpushrebase", default=False)

rebaseparttype = "b2x:rebase"
rebasepackparttype = "b2x:rebasepackpart"
commonheadsparttype = "b2x:commonheads"

treepackrecords = "tempmanifestspackdir"

experimental = "experimental"
configonto = "server-rebase-onto"
pushrebasemarker = "__pushrebase_processed__"
donotrebasemarker = "__pushrebase_donotrebase__"


def uisetup(ui):
    # remotenames circumvents the default push implementation entirely, so make
    # sure we load after it so that we wrap it.
    order = extensions._order
    order.remove("pushrebase")
    order.append("pushrebase")
    extensions._order = order

    cache = {}

    def manifestlogrevision(orig, self, nodeorrev, **kwargs):
        if nodeorrev == nullrev:
            return orig(self, nodeorrev, **kwargs)

        try:
            # Convert rev numbers to nodes if needed
            if isinstance(nodeorrev, int):
                node = self.node(nodeorrev)
            else:
                node = nodeorrev

            haslock = util.islocked(os.path.join(self.opener.join(""), "../wlock"))
            wasincache = node in cache
            cache[node] = True

            msg = "%s manifest read for %s (%s lock)\n" % (
                "cached" if wasincache else "*FULL*",
                short(node),
                "*inside*" if haslock else "outside",
            )

            # Write to user (stderr) if configured
            # internal config: pushrebase.debugprintmanifestreads.user
            if ui.configbool("pushrebase", "debugprintmanifestreads.user", False):
                ui.write_err(msg)
            ui.log("pushrebase", msg)

        except Exception as e:
            ui.write_err("manifest-debug exception: %s\n" % e)
            ui.log("pushrebase", "manifest-debug exception: %s\n" % e)

        return orig(self, nodeorrev, **kwargs)

    # internal config: pushrebase.debugprintmanifestreads
    if ui.configbool("pushrebase", "debugprintmanifestreads", False):
        extensions.wrapfunction(
            manifest.manifestrevlog, "revision", manifestlogrevision
        )

    if ui.configbool("pushrebase", "syncondispatch", True):
        wrapfunction(wireproto, "dispatch", _wireprodispatch)


def extsetup(ui):
    entry = wrapcommand(commands.table, "push", _push)
    # Don't add the 'to' arg if it already exists
    if not any(a for a in entry[1] if a[1] == "to"):
        entry[1].append(("", "to", "", _("server revision to rebase onto")))

    partorder = exchange.b2partsgenorder

    # rebase part must go before the changeset part, so we can mark the
    # changeset part as done first.
    partorder.insert(
        partorder.index("changeset"), partorder.pop(partorder.index(rebaseparttype))
    )

    # rebase pack part must go before rebase part so it can write to the pack to
    # disk for reading.
    partorder.insert(
        partorder.index(rebaseparttype),
        partorder.pop(partorder.index(rebasepackparttype)),
    )

    partorder.insert(0, partorder.pop(partorder.index(commonheadsparttype)))

    if "check-bookmarks" in partorder:
        # check-bookmarks is intended for non-pushrebase scenarios when
        # we can't push to a bookmark if it's changed in the meantime
        partorder.pop(partorder.index("check-bookmarks"))

    # we want to disable the heads check because in pushrebase repos, we
    # expect the heads to change during the push and we should not abort.

    origpushkeyhandler = bundle2.parthandlermapping["pushkey"]
    newpushkeyhandler = lambda *args, **kwargs: bundle2pushkey(
        origpushkeyhandler, *args, **kwargs
    )
    newpushkeyhandler.params = origpushkeyhandler.params
    bundle2.parthandlermapping["pushkey"] = newpushkeyhandler
    bundle2.parthandlermapping["b2x:pushkey"] = newpushkeyhandler

    origphaseheadshandler = bundle2.parthandlermapping["phase-heads"]
    newphaseheadshandler = lambda *args, **kwargs: bundle2phaseheads(
        origphaseheadshandler, *args, **kwargs
    )
    newphaseheadshandler.params = origphaseheadshandler.params
    bundle2.parthandlermapping["phase-heads"] = newphaseheadshandler

    wrapfunction(exchange, "unbundle", unbundle)

    wrapfunction(hg, "_peerorrepo", _peerorrepo)


def reposetup(ui, repo):
    if isnonpushrebaseblocked(repo):
        repo.ui.setconfig(
            "hooks", "prechangegroup.blocknonpushrebase", blocknonpushrebase
        )

    # https://www.mercurial-scm.org/repo/hg/rev/a1e70c1dbec0
    # and related commits added a new way to pushing bookmarks
    # Since pushrebase for now uses pushkey, we want to set this config
    # (T24314128 tracks this)
    legexc = repo.ui.configlist("devel", "legacy.exchange", [])
    if "bookmarks" not in legexc:
        legexc.append("bookmarks")
    repo.ui.setconfig("devel", "legacy.exchange", legexc, "pushrebase")


def isnonpushrebaseblocked(repo):
    return repo.ui.configbool("pushrebase", "blocknonpushrebase")


def blocknonpushrebase(ui, repo, **kwargs):
    if not repo.ui.configbool("pushrebase", pushrebasemarker):
        raise error.Abort(
            _(
                "this repository requires that you enable the "
                "pushrebase extension and push using "
                "'hg push --to'"
            )
        )


def _wireprodispatch(orig, repo, proto, command):
    if command == "batch":
        # Perform a full hgsql sync before negotiating the push with the client.
        #
        # This prevents cases where the client would send public commits that
        # the server was unaware of (but were in the database), causing the
        # push to fail ("cannot rebase public changesets").
        #
        # This can be caused if the synclimiter lock is held for a long time.
        syncifneeded(repo)

    return orig(repo, proto, command)


def _peerorrepo(orig, ui, path, create=False, **kwargs):
    # Force hooks to use a bundle repo
    bundlepath = encoding.environ.get("HG_HOOK_BUNDLEPATH")
    if bundlepath:
        packpaths = encoding.environ.get("HG_HOOK_PACKPATHS")
        if packpaths:
            # Temporarily set the overall setting, then set it directly on the
            # repository.
            with ui.configoverride({("treemanifest", "treeonly"): True}):
                repo = orig(ui, bundlepath, create=create, **kwargs)
            repo.ui.setconfig("treemanifest", "treeonly", True)
        else:
            repo = orig(ui, bundlepath, create=create, **kwargs)

        # Add hook pack paths to the store
        if packpaths:
            paths = packpaths.split(":")
            _addbundlepacks(ui, repo.manifestlog, paths)

        return repo

    return orig(ui, path, create, **kwargs)


def unbundle(orig, repo, cg, heads, source, url, replaydata=None, respondlightly=False):
    # Preload the manifests that the client says we'll need. This happens
    # outside the lock, thus cutting down on our lock time and increasing commit
    # throughput.
    if util.safehasattr(cg, "params"):
        preloadmfs = cg.params.get("preloadmanifests")
        if preloadmfs:
            for mfnode in preloadmfs.split(","):
                repo.manifestlog[bin(mfnode)].read()

    try:
        starttime = time.time()
        result = orig(
            repo,
            cg,
            heads,
            source,
            url,
            replaydata=replaydata,
            respondlightly=respondlightly,
        )
        recording.recordpushrebaserequest(
            repo, conflicts=None, pushrebaseerrmsg=None, starttime=starttime
        )
        return result
    except ConflictsError as ex:
        recording.recordpushrebaserequest(
            repo,
            conflicts="\n".join(sorted(ex.conflicts)),
            pushrebaseerrmsg=None,
            starttime=starttime,
        )
        raise
    except error.HookAbort as ex:
        if ex.reason:
            errmsg = "%s reason: %s" % (ex, ex.reason)
        else:
            errmsg = "%s" % ex
        recording.recordpushrebaserequest(
            repo, conflicts=None, pushrebaseerrmsg=errmsg, starttime=starttime
        )
        raise
    except Exception as ex:
        recording.recordpushrebaserequest(
            repo, conflicts=None, pushrebaseerrmsg="%s" % ex, starttime=starttime
        )
        raise


def validaterevset(repo, revset):
    "Abort if this is a rebasable revset, return None otherwise"
    if not repo.revs(revset):
        raise error.Abort(_("nothing to rebase"))

    revs = repo.revs("%r and public()", revset)
    if revs:
        nodes = []
        for count, rev in enumerate(revs):
            if count >= 3:
                nodes.append("...")
                break
            nodes.append(str(repo[rev]))
        revstring = ", ".join(nodes)
        raise error.Abort(_("cannot rebase public changesets: %s") % revstring)

    if repo.revs("%r and obsolete()", revset):
        raise error.Abort(_("cannot rebase obsolete changesets"))

    heads = repo.revs("heads(%r)", revset)
    if len(heads) > 1:
        raise error.Abort(_("cannot rebase divergent changesets"))

    repo.ui.note(_("validated revset for rebase\n"))


def getrebaseparts(repo, peer, outgoing, onto):
    parts = []
    if util.safehasattr(repo.manifestlog, "datastore"):
        try:
            treemod = extensions.find("treemanifest")
        except KeyError:
            pass
        else:
            sendtrees = shallowbundle.cansendtrees(repo, outgoing.missing)
            if sendtrees != shallowbundle.NoTrees:
                part = treemod.createtreepackpart(
                    repo, outgoing, rebasepackparttype, sendtrees=sendtrees
                )
                parts.append(part)

    parts.append(createrebasepart(repo, peer, outgoing, onto))
    return parts


def createrebasepart(repo, peer, outgoing, onto):
    if not outgoing.missing:
        raise error.Abort(_("no changesets to rebase"))

    if rebaseparttype not in bundle2.bundle2caps(peer):
        raise error.Abort(_("no server support for %r") % rebaseparttype)

    validaterevset(repo, revsetlang.formatspec("%ln", outgoing.missing))

    version = changegroup.safeversion(repo)
    cg = changegroup.makestream(repo, outgoing, version, "push")

    # Explicitly notify the server what obsmarker versions the client supports
    # so the client could receive marker from the server.
    #
    # The core mercurial logic will do the right thing (enable obsmarker
    # capabilities in the pushback bundle) if obsmarker exchange is enabled
    # client-side.
    #
    # But we want the marker without enabling marker exchange, and our server
    # could reply a marker without exchange or even obsstore enabled. So we
    # bypass the "standard" way of capabilities check by sending the supported
    # versions directly in our own part. Note: do not enable "exchange" because
    # it has an unwanted side effect: pushing markers from client to server.
    #
    # "createmarkers" is all we need to be able to write a new marker.
    if obsolete.isenabled(repo, obsolete.createmarkersopt) or mutation.enabled(repo):
        obsmarkerversions = "\0".join(str(v) for v in obsolete.formats)
    else:
        obsmarkerversions = ""

    # .upper() marks this as a mandatory part: server will abort if there's no
    #  handler
    return bundle2.bundlepart(
        rebaseparttype.upper(),
        mandatoryparams={"onto": onto}.items(),
        advisoryparams={
            # advisory: (old) server could ignore this without error
            "obsmarkerversions": obsmarkerversions,
            "cgversion": version,
        }.items(),
        data=cg,
    )


def _push(orig, ui, repo, *args, **opts):
    wnode = repo["."].node()
    onto = opts.get("to")
    if not onto and not opts.get("rev") and not opts.get("dest"):
        try:
            # If it's a tracking bookmark, remotenames will push there,
            # so let's set that up as our --to.
            remotenames = extensions.find("remotenames")
            active = repo._activebookmark
            tracking = remotenames._readtracking(repo)
            if active and active in tracking:
                track = tracking[active]
                path, book = remotenames.splitremotename(track)
                onto = book
        except KeyError:
            # No remotenames? No big deal.
            pass

    overrides = {
        (experimental, configonto): onto,
        ("remotenames", "allownonfastforward"): True,
    }
    if onto:
        overrides[(experimental, "bundle2.pushback")] = True
        tracker = replacementtracker()
    else:
        tracker = util.nullcontextmanager()

    with ui.configoverride(overrides, "pushrebase"), tracker:
        result = orig(ui, repo, *args, **opts)

    if onto and tracker.replacementsreceived:
        with repo.wlock(), repo.lock(), repo.transaction("push") as tr:
            # move working copy parent
            if wnode in tracker.mapping:
                hg.update(repo, tracker.mapping[wnode])
            # move bookmarks
            bmarks = repo._bookmarks
            bmarkchanges = []
            for oldnode, newnode in tracker.mapping.items():
                bmarkchanges.extend(
                    (name, newnode) for name in repo.nodebookmarks(oldnode)
                )
            if bmarkchanges:
                bmarks.applychanges(repo, tr, bmarkchanges)
            visibility.remove(repo, tracker.mapping.keys())
            if mutation.recording(repo):
                # Landed commits require a mutation record, even if the server
                # recorded the land within the commit, as the changelog isn't
                # currently indexed by mutation information.
                entries = []
                for pred, succ in tracker.mapping.items():
                    entries.append(
                        mutation.createsyntheticentry(
                            repo, mutation.ORIGIN_SYNTHETIC, [pred], succ, "pushrebase"
                        )
                    )
                mutation.recordentries(repo, entries, skipexisting=False)

    return result


class replacementtracker(object):
    """track replacements of commits during pushrebase"""

    def __init__(self):
        self.replacementsreceived = False
        self.mapping = {}
        self.pushnodes = set()

    def pushdiscovery(self, orig, pushop):
        ret = orig(pushop)
        self.pushnodes = set(pushop.outgoing.missing)
        return ret

    def processchangegroup(self, orig, op, cg, tr, source, url, **kwargs):
        """find replacements from commit mutation metadata

        Look through the commits that the server returned, looking for ones
        that replace the commits we just pushed.
        """
        self.replacementsreceived = True
        ret = orig(op, cg, tr, source, url, **kwargs)

        clnode = op.repo.changelog.node
        for rev in tr.changes["revs"]:
            node = clnode(rev)
            entry = mutation.createcommitentry(op.repo, node)
            if entry is not None:
                preds = entry.preds() or []
                for pred in preds:
                    if pred in self.pushnodes:
                        self.mapping[pred] = node
        return ret

    def mergemarkers(self, orig, obsstore, transaction, data):
        """find replacements from obsmarkers

        Look through the markers that the server returned, looking for ones
        that tell us the commits that replaced the ones we just pushed.
        """
        version, markers = obsolete._readmarkers(data)
        if version == obsolete._fm1version:
            # only support fm1 1:1 replacements for now, record prec -> sucs
            for prec, sucs, flags, meta, date, parents in markers:
                if len(sucs) == 1:
                    self.mapping[prec] = sucs[0]

        # We force retrieval of obsmarkers even if they are not enabled locally,
        # so that we can look at them to track replacements and maybe synthesize
        # mutation information for the commits.  However, if markers are not
        # enabled locally then the store is read-only and we shouldn't add the
        # markers to it.  In this case, just skip adding them - this is the same
        # as if we'd never requested them.
        if not obsstore._readonly:
            return orig(obsstore, transaction, data)
        return 0

    def phasemove(self, orig, pushop, nodes, phase=phasesmod.public):
        """prevent replaced changesets from being marked public

        When marking changesets as public, we need to mark the replacement nodes
        returned from the server instead. This is done by looking at the new
        obsmarker we received during "_mergemarkers" and map old nodes to new
        ones.

        See exchange.push for the order of this and bundle2 pushback:

            _pushdiscovery(pushop)
            _pushbundle2(pushop)
                # bundle2 pushback is processed here, but the client receiving
                # the pushback cannot affect pushop.*heads (which affects
                # phasemove), because it only gets "repo", and creates a
                # separate "op":
                bundle2.processbundle(pushop.repo, reply, trgetter)
            _pushchangeset(pushop)
            _pushsyncphase(pushop)
                _localphasemove(...) # this method always gets called
            _pushobsolete(pushop)
            _pushbookmark(pushop)

        The least hacky way to get things "right" seem to be:

            1. In core, allow bundle2 pushback handler to affect the original
               "pushop" somehow (so original pushop's (common|future)heads could
               be updated accordingly and phasemove logic is affected)
            2. In pushrebase extension, add a new bundle2 part handler to
               receive the new relationship, correct pushop.*headers, and write
               obsmarkers.
            3. Migrate the obsmarker part to the new bundle2 part added in step
               2, i.e. the server won't send obsmarkers directly.

        For now, we don't have "1" so things are done in a bit hacky way.
        """
        if self.replacementsreceived and phase == phasesmod.public:
            # a rebase occurred, so only allow new nodes to become public
            nodes = [self.mapping.get(n, n) for n in nodes]
            allowednodes = set(self.mapping.values())
            nodes = [n for n in nodes if n in allowednodes]
        orig(pushop, nodes, phase)

    def __enter__(self):
        wrapfunction(exchange, "_pushdiscovery", self.pushdiscovery)
        wrapfunction(bundle2, "_processchangegroup", self.processchangegroup)
        wrapfunction(exchange, "_localphasemove", self.phasemove)
        wrapfunction(obsolete.obsstore, "mergemarkers", self.mergemarkers)

    def __exit__(self, exctype, excvalue, traceback):
        unwrapfunction(exchange, "_pushdiscovery", self.pushdiscovery)
        unwrapfunction(bundle2, "_processchangegroup", self.processchangegroup)
        unwrapfunction(exchange, "_localphasemove", self.phasemove)
        unwrapfunction(obsolete.obsstore, "mergemarkers", self.mergemarkers)


@exchange.b2partsgenerator(commonheadsparttype)
def commonheadspartgen(pushop, bundler):
    if rebaseparttype not in bundle2.bundle2caps(pushop.remote):
        # Server doesn't support pushrebase, so just fallback to normal push.
        return

    if pushop.ui.configbool("experimental", "infinitepush-scratchpush"):
        # We are doing an infinitepush: it's not a pushrebase.
        return

    bundler.newpart(commonheadsparttype, data="".join(pushop.outgoing.commonheads))


@bundle2.parthandler(commonheadsparttype)
def commonheadshandler(op, inpart):
    nodeid = inpart.read(20)
    while len(nodeid) == 20:
        op.records.add(commonheadsparttype, nodeid)
        nodeid = inpart.read(20)
    assert not nodeid  # data should split evenly into blocks of 20 bytes


def checkremotenames():
    try:
        extensions.find("remotenames")
        return True
    except KeyError:
        return False


@exchange.b2partsgenerator(rebasepackparttype)
def packpartgen(pushop, bundler):
    # We generate this part manually during pushrebase pushes, so this is a
    # no-op. But it's required because bundle2 expects there to be a generator
    # for every handler.
    pass


@exchange.b2partsgenerator(rebaseparttype)
def rebasepartgen(pushop, bundler):
    onto = pushop.ui.config(experimental, configonto)
    if "changesets" in pushop.stepsdone or not onto:
        return

    if rebaseparttype not in bundle2.bundle2caps(pushop.remote) and checkremotenames():
        # Server doesn't support pushrebase, but --to is valid in remotenames as
        # well, so just let it through.
        return

    pushop.stepsdone.add("changesets")
    pushop.stepsdone.add("treepack")
    if not pushop.outgoing.missing:
        # It's important that this text match the text found in upstream
        # Mercurial, since some tools rely on this string to know if a push
        # succeeded despite not pushing commits.
        pushop.ui.status(_("no changes found\n"))
        pushop.cgresult = 0
        return

    # Force push means no rebasing, so let's just take the existing parent.
    if pushop.force:
        onto = donotrebasemarker

    rebaseparts = getrebaseparts(pushop.repo, pushop.remote, pushop.outgoing, onto)

    for part in rebaseparts:
        bundler.addpart(part)

    # Tell the server which manifests to load before taking the lock.
    # This helps shorten the duration of the lock, which increases our potential
    # commit rate.
    missing = pushop.outgoing.missing
    roots = pushop.repo.set("parents(%ln) - %ln", missing, missing)
    preloadnodes = [hex(r.manifestnode()) for r in roots]
    bundler.addparam("preloadmanifests", ",".join(preloadnodes))

    def handlereply(op):
        # server either succeeds or aborts; no code to read
        pushop.cgresult = 1

    return handlereply


bundle2.capabilities[rebaseparttype] = ()


def _makebundlefile(op, part, cgversion):
    """constructs a temporary bundle file

    part.data should be an uncompressed v1 changegroup"""

    fp = None
    fd, bundlefile = tempfile.mkstemp()
    try:  # guards bundlefile
        try:  # guards fp
            fp = os.fdopen(fd, "wb")
            if cgversion == "01":
                magic = "HG10UN"
                fp.write(magic)
                data = part.read(mmap.PAGESIZE - len(magic))
                while data:
                    fp.write(data)
                    data = part.read(mmap.PAGESIZE)
            elif cgversion in ["02", "03"]:
                bundle = bundle2.bundle20(op.repo.ui, {})
                cgpart = bundle.newpart("CHANGEGROUP", data=part.read())
                cgpart.addparam("version", cgversion)

                for chunk in bundle.getchunks():
                    fp.write(chunk)
            else:
                raise ValueError("unsupported changegroup version '%s'" % cgversion)
        finally:
            fp.close()
    except Exception:
        try:
            os.unlink(bundlefile)
        except Exception:
            # we would rather see the original exception
            pass
        raise

    return bundlefile


def _getrenamesrcs(op, rev):
    """get all rename sources in a revision"""
    srcs = set()
    revmf = _getmanifest(op, rev)
    for f in rev.files():
        if f in revmf:
            fctx = _getfilectx(rev, revmf, f)
            renamed = fctx.renamed()
            if renamed:
                srcs.add(renamed[0])
    return srcs


def _getrevs(op, bundle, onto, renamesrccache):
    "extracts and validates the revs to be imported"
    validaterevset(bundle, "bundle()")
    revs = [bundle[r] for r in bundle.revs("sort(bundle())")]
    onto = bundle[onto.hex()]
    # Fast forward update, no rebase needed
    if list(bundle.set("bundle() & %d::", onto.rev())):
        return revs, onto

    if revs:
        # We want to rebase the highest bundle root that is an ancestor of
        # `onto`.
        oldonto = list(
            bundle.set("max(parents(bundle()) - bundle() & ::%d)", onto.rev())
        )
        if not oldonto:
            # If there's no shared history, only allow the rebase if the
            # incoming changes are completely distinct.
            sharedparents = list(bundle.set("parents(bundle()) - bundle()"))
            if not sharedparents:
                return revs, bundle[nullid]
            raise error.Abort(
                _(
                    "pushed changesets do not branch from an "
                    "ancestor of the desired destination %s"
                )
                % onto.hex()
            )
        oldonto = oldonto[0]

        # Computes a list of all the incoming file changes
        bundlefiles = set()
        for bundlerev in revs:
            bundlefiles.update(bundlerev.files())

            # Also include sources of renames.
            bundlerevnode = bundlerev.node()
            if bundlerevnode in renamesrccache:
                bundlefiles.update(renamesrccache[bundlerevnode])
            else:
                bundlefiles.update(_getrenamesrcs(op, bundlerev))

        def findconflicts():
            # Returns all the files touched in the bundle that are also touched
            # between the old onto (ex: our old bookmark location) and the new
            # onto (ex: the server's actual bookmark location).
            filematcher = scmutil.matchfiles(bundle, bundlefiles)
            return onto.manifest().diff(oldonto.manifest(), filematcher).keys()

        def findconflictsfast():
            # Fast path for detecting conflicting files. Inspects the changelog
            # file list instead of loading manifests. This only works for
            # non-merge commits, since merge commit file lists do not include
            # all the files changed in the merged.
            ontofiles = set()
            for betweenctx in bundle.set("%d %% %d", onto.rev(), oldonto.rev()):
                ontofiles.update(betweenctx.files())

            return bundlefiles.intersection(ontofiles)

        if bundle.revs("(%d %% %d) - not merge()", onto.rev(), oldonto.rev()):
            # If anything between oldonto and newonto is a merge commit, use the
            # slower manifest diff path.
            conflicts = findconflicts()
        else:
            conflicts = findconflictsfast()

        if conflicts:
            raise ConflictsError(conflicts)

    return revs, oldonto


def _getmanifest(op, rev):
    repo = rev._repo
    if not op.records[treepackrecords] and not repo.ui.configbool(
        "pushrebase", "forcetreereceive"
    ):
        m = rev.manifest()
    else:
        store = repo.manifestlog.datastore
        from edenscmnative import cstore

        m = cstore.treemanifest(store, rev.manifestnode())
        if store.getmissing([("", rev.manifestnode())]):
            raise error.Abort(
                _(
                    "error: pushes must contain tree manifests "
                    "when the server has "
                    "pushrebase.forcetreereceive enabled"
                )
            )
    return m


def _getfilectx(rev, mf, path):
    fileid = mf.get(path)
    return context.filectx(rev._repo, path, fileid=fileid, changectx=rev)


def _graft(op, rev, mapping, lastdestnode, getcommitdate):
    '''duplicate changeset "rev" with parents from "mapping"'''
    repo = op.repo
    oldp1 = rev.p1().node()
    oldp2 = rev.p2().node()
    newp1 = mapping.get(oldp1, oldp1)
    newp2 = mapping.get(oldp2, oldp2)

    m = _getmanifest(op, rev)

    def getfilectx(repo, memctx, path):
        if path in m:
            # We can't use the normal rev[path] accessor here since it will try
            # to go through the flat manifest, which may not exist.
            # That is, fctx.flags() might fail. Therefore use m.flags.
            flags = m.flags(path)
            fctx = _getfilectx(rev, m, path)
            return context.overlayfilectx(fctx, ctx=memctx, flags=flags)
        else:
            return None

    # If the incoming commit has no parents, but requested a rebase,
    # allow it only for the first commit. The null/null commit will always
    # be the first commit since we only allow a nullid->nonnullid mapping if the
    # incoming commits are a completely distinct history (see `sharedparents` in
    # getrevs()), so there's no risk of commits with a single null parent
    # accidentally getting translated first.
    if oldp1 == nullid and oldp2 == nullid:
        if newp1 != nullid:
            newp2 = nullid
            del mapping[nullid]

    if oldp1 != nullid and oldp2 != nullid:
        # The way commits work is they copy p1, then apply the necessary changes
        # to get to the new state. In a pushrebase situation, we are applying
        # changes from the pre-rebase commit to a post-rebase commit, which
        # means we need to ensure that changes caused by the rebase are
        # preserved. In a merge commit, if p2 is the post-rebase commit that
        # contains all the files from the rebase destination, those changes will
        # be lost, since the newp1 doesn't have those changes, and
        # oldp1.diff(oldrev) doesn't have them either. The solution is to ensure
        # that the parent that contains all the original rebase destination
        # files is always p1. We do that by just swapping them here.
        if newp2 == lastdestnode:
            newtemp = newp1
            oldtemp = oldp1
            oldp1 = oldp2
            oldp2 = oldtemp
            newp1 = newp2
            newp2 = newtemp

        # If it's a merge commit, Mercurial's rev.files() only returns the files
        # that are different from both p1 and p2, so it would not capture all of
        # the incoming changes from p2 (for instance, new files in p2). The fix
        # is to manually diff the rev manifest and it's p1 to get the list of
        # files that have changed. We only need to diff against p1, and not p2,
        # because Mercurial constructs new commits by applying our specified
        # files on top of a copy of the p1 manifest, so we only need the diff
        # against p1.
        bundlerepo = rev._repo
        files = _getmanifest(op, rev).diff(_getmanifest(op, bundlerepo[oldp1])).keys()
    else:
        files = rev.files()

    date = getcommitdate(repo.ui, rev.hex(), rev.date())

    extra = rev.extra().copy()
    mutation.record(repo, extra, [rev.node()], "pushrebase")
    loginfo = {"predecessors": rev.hex(), "mutation": "pushrebase"}

    return _commit(
        repo,
        [newp1, newp2],
        rev.description(),
        files,
        getfilectx,
        rev.user(),
        date,
        extra,
        loginfo,
    )


def _commit(repo, parents, desc, files, filectx, user, date, extras, loginfo):
    """ Make a commit as defined by the passed in parameters in the repository.
    All the commits created by the pushrebase extension should ideally go
    through this method.

    This method exists independently so that it can be easily wrapped around by
    other extensions for modifying the commit metadata before the actual commit
    operation.
    """

    return context.memctx(
        repo, parents, desc, files, filectx, user, date, extras, loginfo=loginfo
    ).commit()


def _buildobsolete(replacements, oldrepo, newrepo, date):
    """return obsmarkers, add them locally (server-side) if obsstore enabled"""
    markers = [
        (
            oldrepo[oldrev],
            (newrepo[newrev],),
            {"operation": "push", "user": newrepo[newrev].user()},
        )
        for oldrev, newrev in replacements.items()
        if newrev != oldrev
    ]
    return markers


def _addpushbackchangegroup(repo, reply, outgoing):
    """adds changegroup part to reply containing revs from outgoing.missing"""
    cgversions = set(reply.capabilities.get("changegroup"))
    if not cgversions:
        cgversions.add("01")
    version = max(cgversions & set(changegroup.supportedoutgoingversions(repo)))

    cg = changegroup.makestream(
        repo, outgoing, version, "rebase:reply", b2caps=reply.capabilities
    )

    cgpart = reply.newpart("CHANGEGROUP", data=cg)
    if version != "01":
        cgpart.addparam("version", version)


def _addpushbackobsolete(repo, reply, markers, markerdate, clientobsmarkerversions):
    """adds obsmarkers to reply"""
    # experimental config: pushrebase.pushback.obsmarkers
    # if set to False, the server will not push back obsmarkers.
    if not repo.ui.configbool("pushrebase", "pushback.obsmarkers", True):
        return

    # _buildobsolete has hard-coded obsolete._fm1version raw markers, so client
    # needs to support it, and the reply needs to have the correct capabilities
    if obsolete._fm1version not in clientobsmarkerversions:
        return
    reply.capabilities["obsmarkers"] = ["V1"]

    flag = 0
    parents = None
    try:
        rawmarkers = [
            (
                pre.node(),
                tuple(s.node() for s in sucs),
                flag,
                tuple(sorted(meta.items())),
                markerdate,
                parents,
            )
            for pre, sucs, meta in markers
        ]
        bundle2.buildobsmarkerspart(reply, rawmarkers)
    except ValueError as exc:
        repo.ui.status(_("can't send obsolete markers: %s") % exc.message)


def _addpushbackparts(op, replacements, markers, markerdate, clientobsmarkerversions):
    """adds pushback to reply if supported by the client"""
    if (
        op.records[commonheadsparttype]
        and op.reply
        and "pushback" in op.reply.capabilities
        and not op.respondlightly
    ):
        outgoing = discovery.outgoing(
            op.repo,
            op.records[commonheadsparttype],
            [new for old, new in replacements.items() if old != new],
        )

        if outgoing.missing:
            op.repo.ui.warn(
                _n(
                    "%s new changeset from the server will be downloaded\n",
                    "%s new changesets from the server will be downloaded\n",
                    len(outgoing.missing),
                )
                % len(outgoing.missing)
            )
            _addpushbackchangegroup(op.repo, op.reply, outgoing)
            _addpushbackobsolete(
                op.repo, op.reply, markers, markerdate, clientobsmarkerversions
            )


def resolveonto(repo, ontoarg):
    try:
        if ontoarg != donotrebasemarker:
            return scmutil.revsingle(repo, ontoarg)
    except error.RepoLookupError:
        # Probably a new bookmark. Leave onto as None to not do any rebasing
        pass
    # onto is None means don't do rebasing
    return None


@bundle2.parthandler(rebasepackparttype, ("version", "cache", "category"))
def packparthandler(op, part):
    repo = op.repo

    versionstr = part.params.get("version")
    try:
        version = int(versionstr)
    except ValueError:
        version = 0

    if version < 1 or version > 2:
        raise error.Abort(_("unknown rebasepack bundle2 part version: %s") % versionstr)

    temppackpath = tempfile.mkdtemp()
    op.records.add("tempdirs", temppackpath)
    with mutablestores.mutabledatastore(repo, temppackpath) as dpack:
        with mutablestores.mutablehistorystore(repo, temppackpath) as hpack:
            wirepack.receivepack(repo.ui, part, dpack, hpack, version=version)
    op.records.add("temp%spackdir" % part.params.get("category", ""), temppackpath)
    # TODO: clean up


def _createpackstore(ui, packpath):
    datastore = datapack.datapackstore(ui, packpath)
    histstore = historypack.historypackstore(ui, packpath)
    return datastore, histstore


def _createbundlerepo(op, bundlepath):
    bundle = hg.repository(op.repo.ui, bundlepath)

    # Create stores for any received pack files
    if op.records[treepackrecords]:
        _addbundlepacks(op.repo.ui, bundle.manifestlog, op.records[treepackrecords])

    return bundle


def _addbundlepacks(ui, mfl, packpaths):
    bundledatastores = []
    bundlehiststores = []
    for path in packpaths:
        datastore, histstore = _createpackstore(ui, path)
        bundledatastores.append(datastore)
        bundlehiststores.append(histstore)

    # Point the bundle repo at the temp stores
    bundledatastores.append(mfl.datastore)
    mfl.datastore = contentstore.unioncontentstore(*bundledatastores)
    bundlehiststores.append(mfl.historystore)
    mfl.historystore = metadatastore.unionmetadatastore(*bundlehiststores)


@bundle2.parthandler(
    # "newhead" is not used, but exists for compatibility.
    rebaseparttype,
    ("onto", "newhead", "obsmarkerversions", "cgversion"),
)
def bundle2rebase(op, part):
    """unbundle a bundle2 containing a changegroup to rebase"""

    params = part.params

    bundlefile = None
    bundle = None
    markerdate = util.makedate()
    ui = op.repo.ui

    # Patch ctx._fileinfo so it can look into treemanifests. This covers more
    # code paths (ex. fctx.renamed -> _copied -> ctx.filenode -> ctx._fileinfo
    # -> "repo.manifestlog[self._changeset.manifest].find(path)")
    def _fileinfo(orig, self, path):
        try:
            return orig(self, path)
        except LookupError:
            # Try look up again
            mf = _getmanifest(op, self)
            try:
                return mf.find(path)
            except KeyError:
                raise error.ManifestLookupError(
                    self._node, path, _("not found in manifest")
                )

    with extensions.wrappedfunction(context.basectx, "_fileinfo", _fileinfo):
        ontoparam = params.get("onto", donotrebasemarker)
        try:  # guards bundlefile
            cgversion = params.get("cgversion", "01")
            bundlefile = _makebundlefile(op, part, cgversion)
            bundlepath = "bundle:%s+%s" % (op.repo.root, bundlefile)
            bundle = _createbundlerepo(op, bundlepath)

            def setrecordingparams(repo, ontoparam, ontoctx):
                repo.pushrebaserecordingparams = {
                    "onto": ontoparam,
                    "ontorev": ontoctx and ontoctx.hex(),
                }

            ontoctx = resolveonto(op.repo, ontoparam)
            setrecordingparams(op.repo, ontoparam, ontoctx)

            prepushrebasehooks(op, params, bundle, bundlefile)

            ui.setconfig("pushrebase", pushrebasemarker, True)
            verbose = ontoctx is not None and ui.configbool("pushrebase", "verbose")
            usestackpush = ontoctx is not None and ui.configbool(
                "pushrebase", "trystackpush", True
            )

            def log(msg, force=False):
                if verbose or force:
                    ui.write_err(msg)
                ui.log("pushrebase", msg)

            if usestackpush:
                try:
                    pushrequest = stackpush.pushrequest.fromrevset(bundle, "bundle()")
                except StackPushUnsupportedError as ex:
                    # stackpush is unsupported. Fallback to old code path.
                    if verbose:
                        ui.write_err(_("not using stackpush: %s\n") % ex)

                    usestackpush = False
            if usestackpush:
                # This can happen in the following (rare) case:
                #
                # Client:         Server:
                #
                #  C
                #  |
                #  B               B
                #  |               |
                #  A               A master
                #
                # Client runs "push -r C --to master". "bundle()" only contains
                # "C". The non-stackpush code path would fast-forward master to
                # "C". The stackpush code path will try rebasing "C" to "A".
                # Prevent that. An alternative fix is to pass "::bundle() % onto"
                # to pushrequest.fromrevset. But that's more expensive and adds
                # other complexities.
                if ontoctx.node() != pushrequest.stackparentnode and op.repo.changelog.isancestor(
                    ontoctx.node(), pushrequest.stackparentnode
                ):
                    if verbose:
                        ui.write_err(_("not using stackpush: not rebasing backwards\n"))
                    usestackpush = False

            if usestackpush:
                # stackpush code path - use "pushrequest" instead of "bundlerepo"

                # Check conflicts before entering the critical section. This is
                # optional since there is another check inside the critical
                # section.
                log(_("checking conflicts with %s\n") % (ontoctx,))

                setrecordingparams(op.repo, ontoparam, ontoctx)
                pushrequest.check(ontoctx)

                # Print and log what commits to push.
                log(
                    getpushmessage(
                        pushrequest.pushcommits,
                        lambda c: "%s  %s"
                        % (short(c.orignode), c.desc.split("\n", 1)[0][:50]),
                    ),
                    force=True,
                )

                # Enter the critical section! This triggers a hgsql sync.
                tr = op.gettransaction()
                hookargs = dict(tr.hookargs)
                op.repo.hook("prechangegroup", throw=True, **hookargs)

                # ontoctx could move. Fetch the new one.
                # Print rebase source and destination.
                ontoctx = resolveonto(op.repo, ontoparam)
                log(
                    _("rebasing stack from %s onto %s\n")
                    % (short(pushrequest.stackparentnode), ontoctx)
                )
                setrecordingparams(op.repo, ontoparam, ontoctx)
                added, replacements = pushrequest.pushonto(
                    ontoctx, getcommitdatefn=common.commitdategenerator(op)
                )
            else:
                # Old code path - use a bundlerepo

                # Create a cache of rename sources while we don't have the lock.
                renamesrccache = {
                    bundle[r].node(): _getrenamesrcs(op, bundle[r])
                    for r in bundle.revs("bundle()")
                }

                # Opening the transaction takes the lock, so do it after prepushrebase
                # and after we've fetched all the cache information we'll need.
                tr = op.gettransaction()
                hookargs = dict(tr.hookargs)

                # Recreate the bundle repo, since taking the lock in gettransaction()
                # may have caused it to become out of date.
                # (but grab a copy of the cache first)
                bundle.close()
                bundle = _createbundlerepo(op, bundlepath)

                onto = getontotarget(op, params, bundle)

                setrecordingparams(op.repo, ontoparam, onto)
                revs, oldonto = _getrevs(op, bundle, onto, renamesrccache)

                op.repo.hook("prechangegroup", throw=True, **hookargs)

                log(
                    getpushmessage(
                        revs,
                        lambda r: "%s  %s"
                        % (r, bundle[r].description().split("\n", 1)[0][:50]),
                    ),
                    force=True,
                )

                # Prepopulate the revlog _cache with the original onto's fulltext. This
                # means reading the new onto's manifest will likely have a much shorter
                # delta chain to traverse.
                log(_("rebasing onto %s\n") % (short(onto.node()),))

                # Perform the rebase + commit to the main repo
                added, replacements = runrebase(op, revs, oldonto, onto)

                # revs is modified by runrebase to ensure garbage collection of
                # manifests, so don't use it from here on.
                revs = None

            op.repo.pushrebaseaddedchangesets = added
            op.repo.pushrebasereplacements = replacements

            markers = _buildobsolete(replacements, bundle, op.repo, markerdate)
        finally:
            pushrebaserecordingparams = getattr(
                op.repo, "pushrebaserecordingparams", None
            )
            if pushrebaserecordingparams is not None:
                rebasedctx = resolveonto(op.repo, ontoparam)
                if rebasedctx:
                    pushrebaserecordingparams["onto_rebased_rev"] = rebasedctx.hex()
                pushrebaserecordingparams.update(
                    {
                        "replacements_revs": json.dumps(
                            {
                                hex(k): hex(v)
                                for k, v in getattr(
                                    op.repo, "pushrebasereplacements", {}
                                ).items()
                            }
                        ),
                        "ordered_added_revs": json.dumps(
                            [
                                hex(v)
                                for v in getattr(
                                    op.repo, "pushrebaseaddedchangesets", []
                                )
                            ]
                        ),
                    }
                )

            try:
                if bundlefile:
                    os.unlink(bundlefile)
            except OSError as e:
                if e.errno != errno.ENOENT:
                    raise
            if bundle:
                bundle.close()

    # Move public phase forward
    publishing = op.repo.ui.configbool("phases", "publish", untrusted=True)
    if publishing:
        phasesmod.advanceboundary(op.repo, tr, phasesmod.public, [added[-1]])

    addfinalhooks(op, tr, hookargs, added)

    # Send new commits back to the client
    clientobsmarkerversions = [
        int(v) for v in params.get("obsmarkerversions", "").split("\0") if v
    ]
    _addpushbackparts(op, replacements, markers, markerdate, clientobsmarkerversions)

    for k in replacements.keys():
        replacements[hex(k)] = hex(replacements[k])
    op.records.add(rebaseparttype, replacements)

    return 1


def prepushrebasehooks(op, params, bundle, bundlefile):
    onto = params.get("onto")
    prelockonto = resolveonto(op.repo, onto or donotrebasemarker)
    prelockontonode = prelockonto.hex() if prelockonto else None

    # Allow running hooks on the new commits before we take the lock
    if op.hookargs is None:
        # Usually pushrebase prepushrebasehooks are called outside of
        # transaction. If that's the case then op.hookargs is not None and
        # it contains hook arguments.
        # However Mononoke -> hg sync job might replay two bundles under
        # the same transaction. In that case hookargs are stored in transaction
        # object (see bundle2operation:gettransaction).
        #
        # For reference: Mononoke -> hg sync job uses wireproto.py:unbundlereplay
        # function as it's entry point
        tr = op.repo.currenttransaction()
        if tr is not None:
            prelockrebaseargs = tr.hookargs.copy()
        else:
            raise error.ProgrammingError("internal error: hookargs are not set")
    else:
        prelockrebaseargs = op.hookargs.copy()
    prelockrebaseargs["source"] = "push"
    prelockrebaseargs["bundle2"] = "1"
    prelockrebaseargs["node"] = scmutil.revsingle(bundle, "min(bundle())").hex()
    prelockrebaseargs["node_onto"] = prelockontonode
    if onto:
        prelockrebaseargs["onto"] = onto
    prelockrebaseargs["hook_bundlepath"] = bundlefile

    for path in op.records[treepackrecords]:
        if ":" in path:
            raise RuntimeError(_("tree pack path may not contain colon (%s)") % path)
    packpaths = ":".join(op.records[treepackrecords])
    prelockrebaseargs["hook_packpaths"] = packpaths

    if op.records[treepackrecords]:
        # If we received trees, force python hooks to operate in treeonly mode
        # so they load only trees.
        repo = op.repo
        with repo.baseui.configoverride(
            {("treemanifest", "treeonly"): True}
        ), util.environoverride("HG_HOOK_BUNDLEPATH", bundlefile), util.environoverride(
            "HG_HOOK_PACKPATHS", packpaths
        ):
            brepo = hg.repository(repo.ui, repo.root)
            brepo.hook("prepushrebase", throw=True, **prelockrebaseargs)
    else:
        op.repo.hook("prepushrebase", throw=True, **prelockrebaseargs)

    revs = list(bundle.revs("bundle()"))
    changegroup.checkrevs(bundle, revs)


def syncifneeded(repo):
    """Performs a hgsql sync if enabled"""
    # internal config: pushrebase.runhgsqlsync
    if not repo.ui.configbool("pushrebase", "runhgsqlsync", False):
        return

    if hgsql.issqlrepo(repo):
        oldrevcount = len(repo)
        hgsql.executewithsql(repo, lambda: None, enforcepullfromdb=True)
        newrevcount = len(repo)
        if oldrevcount != newrevcount:
            msg = "pushrebase: tip moved %d -> %d\n" % (oldrevcount, newrevcount)
        else:
            msg = "pushrebase: tip not moved\n"
        repo.ui.log("pushrebase", msg)

        # internal config: pushrebase.runhgsqlsync.debug
        if repo.ui.configbool("pushrebase", "runhgsqlsync.debug", False):
            repo.ui.write_err(msg)


def getontotarget(op, params, bundle):
    onto = resolveonto(op.repo, params.get("onto", donotrebasemarker))

    if onto is None:
        maxcommonanc = list(bundle.set("max(parents(bundle()) - bundle())"))
        if not maxcommonanc:
            onto = op.repo[nullid]
        else:
            onto = maxcommonanc[0]
    return onto


def getpushmessage(revs, getmessage):
    # Notify the user of what is being pushed
    io = util.stringio()
    io.write(
        _n("pushing %s changeset:\n", "pushing %s changesets:\n", len(revs)) % len(revs)
    )
    maxoutput = 10
    for i in range(0, min(len(revs), maxoutput)):
        io.write(("    %s\n") % (getmessage(revs[i])))

    if len(revs) > maxoutput + 1:
        io.write(("    ...\n"))
        io.write(("    %s\n") % (getmessage(revs[-1])))
    return io.getvalue()


def runrebase(op, revs, oldonto, onto):
    mapping = {}
    replacements = {}
    added = []

    # Seed the mapping with oldonto->onto
    mapping[oldonto.node()] = onto.node()

    lastdestnode = onto.node()

    # Pop rev contexts from the list as we iterate, so we garbage collect the
    # manifests we're creating.
    revs.reverse()

    while revs:
        rev = revs.pop()
        getcommitdate = common.commitdategenerator(op)
        newrev = _graft(op, rev, mapping, lastdestnode, getcommitdate)

        new = op.repo[newrev]
        oldnode = rev.node()
        newnode = new.node()
        replacements[oldnode] = newnode
        mapping[oldnode] = newnode
        added.append(newnode)

        # Track which commit contains the original rebase destination
        # contents, so we can preserve the appropriate side's content during
        # merges.
        if lastdestnode == new.p1().node():
            lastdestnode = newnode

    return added, replacements


def addfinalhooks(op, tr, hookargs, added):
    hookargs["node"] = tr.hookargs["node"] = hex(added[0])
    hookargs["node_last"] = hex(added[-1])

    p = lambda: tr.writepending() and op.repo.root or ""
    op.repo.hook("pretxnchangegroup", throw=True, pending=p, **hookargs)

    def runhooks():
        args = hookargs.copy()
        op.repo.hook("changegroup", **hookargs)
        args.pop("node_last")

    tr.addpostclose("serverrebase-cg-hooks", lambda tr: op.repo._afterlock(runhooks))


def bundle2pushkey(orig, op, part):
    # Merges many dicts into one. First it converts them to list of pairs,
    # then concatenates them (using sum), and then creates a diff out of them.
    replacements = dict(
        sum([record.items() for record in op.records[rebaseparttype]], [])
    )

    namespace = pushkey.decode(part.params["namespace"])
    if namespace == "phases":
        key = pushkey.decode(part.params["key"])
        part.params["key"] = pushkey.encode(replacements.get(key, key))
    if namespace == "bookmarks":
        new = pushkey.decode(part.params["new"])
        part.params["new"] = pushkey.encode(replacements.get(new, new))
        serverbin = op.repo._bookmarks.get(part.params["key"])
        clienthex = pushkey.decode(part.params["old"])

        if serverbin and clienthex:
            cl = op.repo.changelog
            revserver = cl.rev(serverbin)
            revclient = cl.rev(bin(clienthex))
            if revclient in cl.ancestors([revserver]):
                # if the client's bookmark origin is an lagging behind the
                # server's location for that bookmark (usual for pushrebase)
                # then update the old location to match the real location
                #
                # TODO: We would prefer to only do this for pushrebase pushes
                # but that isn't straightforward so we just do it always here.
                # This forbids moving bookmarks backwards from clients.
                part.params["old"] = pushkey.encode(hex(serverbin))

    return orig(op, part)


def bundle2phaseheads(orig, op, part):
    # Merges many dicts into one. First it converts them to list of pairs,
    # then concatenates them (using sum), and then creates a diff out of them.
    replacements = dict(
        sum([record.items() for record in op.records[rebaseparttype]], [])
    )

    decodedphases = phasesmod.binarydecode(part)

    replacedphases = []
    for phasetype in decodedphases:
        replacedphases.append([replacements.get(node, node) for node in phasetype])
    # Since we've just read the bundle part, then `orig()` won't be able to
    # read it again. Let's replace payload stream with new stream of replaced
    # nodes.
    part._payloadstream = util.chunkbuffer([phasesmod.binaryencode(replacedphases)])
    return orig(op, part)
