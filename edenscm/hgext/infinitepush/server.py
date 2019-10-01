# Infinite push
#
# Copyright 2016-2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import contextlib
import functools
import os
import random
import socket
import subprocess
import tempfile
import time

from edenscm.mercurial import (
    bundle2,
    changegroup,
    discovery,
    encoding,
    error,
    exchange,
    extensions,
    hg,
    localrepo,
    mutation,
    node as nodemod,
    phases,
    util,
    wireproto,
)
from edenscm.mercurial.i18n import _, _n

from . import constants


def extsetup(ui):
    origpushkeyhandler = bundle2.parthandlermapping["pushkey"]

    def newpushkeyhandler(*args, **kwargs):
        bundle2pushkey(origpushkeyhandler, *args, **kwargs)

    newpushkeyhandler.params = origpushkeyhandler.params
    bundle2.parthandlermapping["pushkey"] = newpushkeyhandler

    orighandlephasehandler = bundle2.parthandlermapping["phase-heads"]
    newphaseheadshandler = lambda *args, **kwargs: bundle2handlephases(
        orighandlephasehandler, *args, **kwargs
    )
    newphaseheadshandler.params = orighandlephasehandler.params
    bundle2.parthandlermapping["phase-heads"] = newphaseheadshandler

    extensions.wrapfunction(localrepo.localrepository, "listkeys", localrepolistkeys)
    wireproto.commands["lookup"] = (
        _makelookupwrap(wireproto.commands["lookup"][0]),
        "key",
    )
    extensions.wrapfunction(exchange, "getbundlechunks", getbundlechunks)
    extensions.wrapfunction(bundle2, "processparts", processparts)

    if util.safehasattr(wireproto, "_capabilities"):
        extensions.wrapfunction(wireproto, "_capabilities", _capabilities)
    else:
        extensions.wrapfunction(wireproto, "capabilities", _capabilities)


def _capabilities(orig, repo, proto):
    caps = orig(repo, proto)
    caps.append("listkeyspatterns")
    caps.append("knownnodes")
    return caps


def bundle2pushkey(orig, op, part):
    """Wrapper of bundle2.handlepushkey()

    The only goal is to skip calling the original function if flag is set.
    It's set if infinitepush push is happening.
    """
    if op.records[constants.scratchbranchparttype + "_skippushkey"]:
        if op.reply is not None:
            rpart = op.reply.newpart("reply:pushkey")
            rpart.addparam("in-reply-to", str(part.id), mandatory=False)
            rpart.addparam("return", "1", mandatory=False)
        return 1

    return orig(op, part)


def bundle2handlephases(orig, op, part):
    """Wrapper of bundle2.handlephases()

    The only goal is to skip calling the original function if flag is set.
    It's set if infinitepush push is happening.
    """

    if op.records[constants.scratchbranchparttype + "_skipphaseheads"]:
        return

    return orig(op, part)


def localrepolistkeys(orig, self, namespace, patterns=None):
    """Wrapper of localrepo.listkeys()"""

    if namespace == "bookmarks" and patterns:
        index = self.bundlestore.index
        # Using sortdict instead of a dictionary to ensure that bookmaks are
        # restored in the same order after a pullbackup. See T24417531
        results = util.sortdict()
        bookmarks = orig(self, namespace)
        for pattern in patterns:
            results.update(index.getbookmarks(pattern))
            if pattern.endswith("*"):
                pattern = "re:^" + pattern[:-1] + ".*"
            kind, pat, matcher = util.stringmatcher(pattern)
            for bookmark, node in bookmarks.iteritems():
                if matcher(bookmark):
                    results[bookmark] = node
        return results
    else:
        return orig(self, namespace)


def _makelookupwrap(orig):
    """Create wrapper for wireproto lookup command."""

    def _lookup(repo, proto, key):
        localkey = encoding.tolocal(key)

        if isinstance(localkey, str) and repo._scratchbranchmatcher.match(localkey):
            scratchnode = repo.bundlestore.index.getnode(localkey)
            if scratchnode:
                return "%s %s\n" % (1, scratchnode)
            else:
                return "%s %s\n" % (0, "scratch branch %s not found" % localkey)
        else:
            try:
                r = nodemod.hex(repo.lookup(localkey))
                return "%s %s\n" % (1, r)
            except Exception as inst:
                try:
                    node = repo.bundlestore.index.getnodebyprefix(localkey)
                    if node:
                        return "%s %s\n" % (1, node)
                    else:
                        return "%s %s\n" % (0, str(inst))
                except Exception as inst:
                    return "%s %s\n" % (0, str(inst))

    return _lookup


def getbundlechunks(orig, repo, source, heads=None, bundlecaps=None, **kwargs):
    heads = heads or []
    # newheads are parents of roots of scratch bundles that were requested
    newphases = {}
    scratchbundles = []
    newheads = []
    scratchheads = []
    nodestobundle = {}
    allbundlestocleanup = []

    cgversion = _getsupportedcgversion(repo, bundlecaps or [])
    try:
        for head in heads:
            if head not in repo.changelog.nodemap:
                if head not in nodestobundle:
                    newbundlefile = downloadbundle(repo, head)
                    bundlepath = "bundle:%s+%s" % (repo.root, newbundlefile)
                    bundlerepo = hg.repository(repo.ui, bundlepath)

                    allbundlestocleanup.append((bundlerepo, newbundlefile))
                    bundlerevs = set(bundlerepo.revs("bundle()"))
                    bundlecaps = _includefilelogstobundle(
                        bundlecaps, bundlerepo, bundlerevs, repo.ui
                    )
                    cl = bundlerepo.changelog
                    bundleroots = _getbundleroots(repo, bundlerepo, bundlerevs)
                    draftcommits = set()
                    bundleheads = set([head])
                    for rev in bundlerevs:
                        node = cl.node(rev)
                        draftcommits.add(node)
                        if node in heads:
                            bundleheads.add(node)
                            nodestobundle[node] = (
                                bundlerepo,
                                bundleroots,
                                newbundlefile,
                            )

                    if draftcommits:
                        # Filter down to roots of this head, so we don't report
                        # non-roots as phase roots and we don't report commits
                        # that aren't related to the requested head.
                        for rev in bundlerepo.revs(
                            "roots((%ln) & ::%ln)", draftcommits, bundleheads
                        ):
                            newphases[bundlerepo[rev].hex()] = str(phases.draft)

                scratchbundles.append(
                    _generateoutputparts(
                        head, cgversion, bundlecaps, *nodestobundle[head]
                    )
                )
                newheads.extend(bundleroots)
                scratchheads.append(head)
    finally:
        for bundlerepo, bundlefile in allbundlestocleanup:
            bundlerepo.close()
            try:
                os.unlink(bundlefile)
            except (IOError, OSError):
                # if we can't cleanup the file then just ignore the error,
                # no need to fail
                pass

    pullfrombundlestore = bool(scratchbundles)
    wrappedchangegrouppart = False
    wrappedlistkeys = False
    oldchangegrouppart = exchange.getbundle2partsmapping["changegroup"]
    try:

        def _changegrouppart(bundler, *args, **kwargs):
            # Order is important here. First add non-scratch part
            # and only then add parts with scratch bundles because
            # non-scratch part contains parents of roots of scratch bundles.
            result = oldchangegrouppart(bundler, *args, **kwargs)
            for bundle in scratchbundles:
                for part in bundle:
                    bundler.addpart(part)
            return result

        exchange.getbundle2partsmapping["changegroup"] = _changegrouppart
        wrappedchangegrouppart = True

        def _listkeys(orig, self, namespace):
            origvalues = orig(self, namespace)
            if namespace == "phases" and pullfrombundlestore:
                if origvalues.get("publishing") == "True":
                    # Make repo non-publishing to preserve draft phase
                    del origvalues["publishing"]
                origvalues.update(newphases)
            return origvalues

        extensions.wrapfunction(localrepo.localrepository, "listkeys", _listkeys)
        wrappedlistkeys = True
        heads = list((set(newheads) | set(heads)) - set(scratchheads))
        result = orig(repo, source, heads=heads, bundlecaps=bundlecaps, **kwargs)
    finally:
        if wrappedchangegrouppart:
            exchange.getbundle2partsmapping["changegroup"] = oldchangegrouppart
        if wrappedlistkeys:
            extensions.unwrapfunction(localrepo.localrepository, "listkeys", _listkeys)
    return result


def _getsupportedcgversion(repo, bundlecaps):
    b2caps = _decodebundle2caps(bundlecaps)

    cgversion = "01"
    cgversions = b2caps.get("changegroup")
    if cgversions:  # 3.1 and 3.2 ship with an empty value
        cgversions = [
            v for v in cgversions if v in changegroup.supportedoutgoingversions(repo)
        ]
        if not cgversions:
            raise ValueError(_("no common changegroup version"))
        cgversion = max(cgversions)
    return cgversion


# TODO(stash): remove copy-paste from upstream hg
def _decodebundle2caps(bundlecaps):
    b2caps = {}
    for bcaps in bundlecaps:
        if bcaps.startswith("bundle2="):
            blob = util.urlreq.unquote(bcaps[len("bundle2=") :])
            b2caps.update(bundle2.decodecaps(blob))
    return b2caps


def downloadbundle(repo, unknownbinhead):
    index = repo.bundlestore.index
    store = repo.bundlestore.store
    bundleid = index.getbundle(nodemod.hex(unknownbinhead))
    if bundleid is None:
        raise error.Abort("%s head is not known" % nodemod.hex(unknownbinhead))
    data = store.read(bundleid)
    fp = None
    fd, bundlefile = tempfile.mkstemp()
    try:  # guards bundlefile
        try:  # guards fp
            fp = util.fdopen(fd, "wb")
            fp.write(data)
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


def _includefilelogstobundle(bundlecaps, bundlerepo, bundlerevs, ui):
    """Tells remotefilelog to include all changed files to the changegroup

    By default remotefilelog doesn't include file content to the changegroup.
    But we need to include it if we are fetching from bundlestore.
    """
    changedfiles = set()
    cl = bundlerepo.changelog
    for r in bundlerevs:
        # [3] means changed files
        changedfiles.update(cl.read(r)[3])
    if not changedfiles:
        return bundlecaps

    changedfiles = "\0".join("path:%s" % p for p in changedfiles)
    newcaps = []
    appended = False
    for cap in bundlecaps or []:
        if cap.startswith("excludepattern="):
            newcaps.append("\0".join((cap, changedfiles)))
            appended = True
        else:
            newcaps.append(cap)
    if not appended:
        # Not found excludepattern cap. Just append it
        newcaps.append("excludepattern=" + changedfiles)

    return newcaps


def _getbundleroots(oldrepo, bundlerepo, bundlerevs):
    cl = bundlerepo.changelog
    bundleroots = []
    for rev in bundlerevs:
        node = cl.node(rev)
        parents = cl.parents(node)
        for parent in parents:
            # include all revs that exist in the main repo
            # to make sure that bundle may apply client-side
            if parent != nodemod.nullid and parent in oldrepo:
                bundleroots.append(parent)
    return bundleroots


def _needsrebundling(head, bundlerepo):
    bundleheads = list(bundlerepo.revs("heads(bundle())"))
    return not (len(bundleheads) == 1 and bundlerepo[bundleheads[0]].node() == head)


def _generateoutputparts(
    head, cgversion, bundlecaps, bundlerepo, bundleroots, bundlefile
):
    """generates bundle that will be send to the user

    returns tuple with raw bundle string and bundle type
    """
    parts = []
    if not _needsrebundling(head, bundlerepo):
        with util.posixfile(bundlefile, "rb") as f:
            unbundler = exchange.readbundle(bundlerepo.ui, f, bundlefile)
            if isinstance(unbundler, changegroup.cg1unpacker):
                part = bundle2.bundlepart("changegroup", data=unbundler._stream.read())
                part.addparam("version", "01")
                parts.append(part)
            elif isinstance(unbundler, bundle2.unbundle20):
                haschangegroup = False
                for part in unbundler.iterparts():
                    if part.type == "changegroup":
                        haschangegroup = True
                    newpart = bundle2.bundlepart(part.type, data=part.read())
                    for key, value in part.params.iteritems():
                        newpart.addparam(key, value)
                    parts.append(newpart)

                if not haschangegroup:
                    raise error.Abort(
                        "unexpected bundle without changegroup part, "
                        + "head: %s" % hex(head),
                        hint="report to administrator",
                    )
            else:
                raise error.Abort("unknown bundle type")
    else:
        parts = _rebundle(bundlerepo, bundleroots, head, cgversion, bundlecaps)

    return parts


def _rebundle(bundlerepo, bundleroots, unknownhead, cgversion, bundlecaps):
    """
    Bundle may include more revision then user requested. For example,
    if user asks for revision but bundle also consists its descendants.
    This function will filter out all revision that user is not requested.
    """
    parts = []

    outgoing = discovery.outgoing(
        bundlerepo, commonheads=bundleroots, missingheads=[unknownhead]
    )
    cgstream = changegroup.makestream(
        bundlerepo, outgoing, cgversion, "pull", bundlecaps=bundlecaps
    )
    cgstream = util.chunkbuffer(cgstream).read()
    cgpart = bundle2.bundlepart("changegroup", data=cgstream)
    cgpart.addparam("version", cgversion)
    parts.append(cgpart)

    # This parsing should be refactored to be shared with
    # exchange.getbundlechunks. But I'll do that in a separate diff.
    if bundlecaps is None:
        bundlecaps = set()
    b2caps = {}
    for bcaps in bundlecaps:
        if bcaps.startswith("bundle2="):
            blob = util.urlreq.unquote(bcaps[len("bundle2=") :])
            b2caps.update(bundle2.decodecaps(blob))

    if constants.scratchmutationparttype in b2caps:
        mutdata = mutation.bundle(bundlerepo, outgoing.missing)
        parts.append(
            bundle2.bundlepart(constants.scratchmutationparttype, data=mutdata)
        )

    try:
        treemod = extensions.find("treemanifest")
        remotefilelog = extensions.find("remotefilelog")
    except KeyError:
        pass
    else:
        missing = outgoing.missing
        if remotefilelog.shallowbundle.cansendtrees(
            bundlerepo, missing, source="pull", bundlecaps=bundlecaps, b2caps=b2caps
        ):
            try:
                treepart = treemod.createtreepackpart(
                    bundlerepo, outgoing, treemod.TREEGROUP_PARTTYPE2
                )
                parts.append(treepart)
            except BaseException as ex:
                parts.append(bundle2.createerrorpart(str(ex)))

    try:
        snapshot = extensions.find("snapshot")
    except KeyError:
        pass
    else:
        snapshot.bundleparts.appendsnapshotmetadatabundlepart(
            bundlerepo, outgoing.missing, parts
        )

    return parts


def processparts(orig, repo, op, unbundler):
    if unbundler.params.get("infinitepush") != "True":
        return orig(repo, op, unbundler)

    handleallparts = repo.ui.configbool("infinitepush", "storeallparts")

    partforwardingwhitelist = [constants.scratchmutationparttype]
    try:
        treemfmod = extensions.find("treemanifest")
        partforwardingwhitelist.append(treemfmod.TREEGROUP_PARTTYPE2)
    except KeyError:
        pass

    try:
        snapshot = extensions.find("snapshot")
        partforwardingwhitelist.append(snapshot.bundleparts.snapshotmetadataparttype)
    except KeyError:
        pass

    bundler = bundle2.bundle20(repo.ui)
    compress = repo.ui.config("infinitepush", "bundlecompression", "UN")
    bundler.setcompression(compress)
    cgparams = None
    scratchbookpart = None
    with bundle2.partiterator(repo, op, unbundler) as parts:
        for part in parts:
            bundlepart = None
            if part.type == "replycaps":
                # This configures the current operation to allow reply parts.
                bundle2._processpart(op, part)
            elif part.type == constants.scratchbranchparttype:
                # Scratch branch parts need to be converted to normal
                # changegroup parts, and the extra parameters stored for later
                # when we upload to the store. Eventually those parameters will
                # be put on the actual bundle instead of this part, then we can
                # send a vanilla changegroup instead of the scratchbranch part.
                cgversion = part.params.get("cgversion", "01")
                bundlepart = bundle2.bundlepart("changegroup", data=part.read())
                bundlepart.addparam("version", cgversion)
                cgparams = part.params

                # If we're not dumping all parts into the new bundle, we need to
                # alert the future pushkey and phase-heads handler to skip
                # the part.
                if not handleallparts:
                    op.records.add(
                        constants.scratchbranchparttype + "_skippushkey", True
                    )
                    op.records.add(
                        constants.scratchbranchparttype + "_skipphaseheads", True
                    )
            elif part.type == constants.scratchbookmarksparttype:
                # Save this for later processing. Details below.
                #
                # Upstream https://phab.mercurial-scm.org/D1389 and its
                # follow-ups stop part.seek support to reduce memory usage
                # (https://bz.mercurial-scm.org/5691). So we need to copy
                # the part so it can be consumed later.
                scratchbookpart = copiedpart(part)
            else:
                if handleallparts or part.type in partforwardingwhitelist:
                    # Ideally we would not process any parts, and instead just
                    # forward them to the bundle for storage, but since this
                    # differs from previous behavior, we need to put it behind a
                    # config flag for incremental rollout.
                    bundlepart = bundle2.bundlepart(part.type, data=part.read())
                    for key, value in part.params.iteritems():
                        bundlepart.addparam(key, value)

                    # Certain parts require a response
                    if part.type == "pushkey":
                        if op.reply is not None:
                            rpart = op.reply.newpart("reply:pushkey")
                            rpart.addparam("in-reply-to", str(part.id), mandatory=False)
                            rpart.addparam("return", "1", mandatory=False)
                else:
                    bundle2._processpart(op, part)

            if handleallparts:
                op.records.add(part.type, {"return": 1})
            if bundlepart:
                bundler.addpart(bundlepart)

    # If commits were sent, store them
    if cgparams:
        buf = util.chunkbuffer(bundler.getchunks())
        fd, bundlefile = tempfile.mkstemp()
        try:
            try:
                fp = util.fdopen(fd, "wb")
                fp.write(buf.read())
            finally:
                fp.close()
            storebundle(op, cgparams, bundlefile)
        finally:
            try:
                os.unlink(bundlefile)
            except Exception:
                # we would rather see the original exception
                pass

    # The scratch bookmark part is sent as part of a push backup. It needs to be
    # processed after the main bundle has been stored, so that any commits it
    # references are available in the store.
    if scratchbookpart:
        bundle2._processpart(op, scratchbookpart)


class copiedpart(object):
    """a copy of unbundlepart content that can be consumed later"""

    def __init__(self, part):
        # copy "public properties"
        self.type = part.type
        self.id = part.id
        self.mandatory = part.mandatory
        self.mandatoryparams = part.mandatoryparams
        self.advisoryparams = part.advisoryparams
        self.params = part.params
        self.mandatorykeys = part.mandatorykeys
        # copy the buffer
        self._io = util.stringio(part.read())

    def consume(self):
        return

    def read(self, size=None):
        if size is None:
            return self._io.read()
        else:
            return self._io.read(size)


def _getorcreateinfinitepushlogger(op):
    logger = op.records["infinitepushlogger"]
    if not logger:
        ui = op.repo.ui
        try:
            username = util.getuser()
        except Exception:
            username = "unknown"
        # Generate random request id to be able to find all logged entries
        # for the same request. Since requestid is pseudo-generated it may
        # not be unique, but we assume that (hostname, username, requestid)
        # is unique.
        random.seed()
        requestid = random.randint(0, 2000000000)
        hostname = socket.gethostname()
        logger = functools.partial(
            ui.log,
            "infinitepush",
            user=username,
            requestid=requestid,
            hostname=hostname,
            reponame=ui.config("infinitepush", "reponame"),
        )
        op.records.add("infinitepushlogger", logger)
    else:
        logger = logger[0]
    return logger


@contextlib.contextmanager
def logservicecall(logger, service, **kwargs):
    start = time.time()
    logger(service, eventtype="start", **kwargs)
    try:
        yield
        logger(
            service,
            eventtype="success",
            elapsedms=(time.time() - start) * 1000,
            **kwargs
        )
    except Exception as e:
        logger(
            service,
            eventtype="failure",
            elapsedms=(time.time() - start) * 1000,
            errormsg=str(e),
            **kwargs
        )
        raise


def storebundle(op, params, bundlefile):
    log = _getorcreateinfinitepushlogger(op)
    parthandlerstart = time.time()
    log(constants.scratchbranchparttype, eventtype="start")
    index = op.repo.bundlestore.index
    store = op.repo.bundlestore.store
    op.records.add(constants.scratchbranchparttype + "_skippushkey", True)

    bundle = None
    try:  # guards bundle
        bundlepath = "bundle:%s+%s" % (op.repo.root, bundlefile)
        bundle = hg.repository(op.repo.ui, bundlepath)

        bookmark = params.get("bookmark")
        create = params.get("create")
        force = params.get("force")

        if bookmark:
            oldnode = index.getnode(bookmark)

            if not oldnode and not create:
                raise error.Abort(
                    "unknown bookmark %s" % bookmark,
                    hint="use --create if you want to create one",
                )
        else:
            oldnode = None
        bundleheads = bundle.revs("heads(bundle())")
        if bookmark and len(bundleheads) > 1:
            raise error.Abort(_("cannot push more than one head to a scratch branch"))

        revs = _getrevs(bundle, oldnode, force, bookmark)

        # Notify the user of what is being pushed
        op.repo.ui.warn(
            _n("pushing %s commit:\n", "pushing %s commits:\n", len(revs)) % len(revs)
        )
        maxoutput = 10
        for i in range(0, min(len(revs), maxoutput)):
            firstline = bundle[revs[i]].description().split("\n")[0][:50]
            op.repo.ui.warn(("    %s  %s\n") % (revs[i], firstline))

        if len(revs) > maxoutput + 1:
            op.repo.ui.warn(("    ...\n"))
            firstline = bundle[revs[-1]].description().split("\n")[0][:50]
            op.repo.ui.warn(("    %s  %s\n") % (revs[-1], firstline))

        nodesctx = [bundle[rev] for rev in revs]
        inindex = lambda rev: bool(index.getbundle(bundle[rev].hex()))
        if bundleheads:
            newheadscount = sum(not inindex(rev) for rev in bundleheads)
        else:
            newheadscount = 0
        # If there's a bookmark specified, the bookmarked node should also be
        # provided.  Older clients may omit this, in which case there should be
        # only one head, so we choose the last node, which will be that head.
        # If a bug or malicious client allows there to be a bookmark
        # with multiple heads, we will place the bookmark on the last head.
        bookmarknode = params.get(
            "bookmarknode", nodesctx[-1].hex() if nodesctx else None
        )
        key = None
        if newheadscount:
            with open(bundlefile, "r") as f:
                bundledata = f.read()
                with logservicecall(log, "bundlestore", bundlesize=len(bundledata)):
                    bundlesizelimitmb = op.repo.ui.configint(
                        "infinitepush", "maxbundlesize", 100
                    )
                    if len(bundledata) > bundlesizelimitmb * 1024 * 1024:
                        error_msg = (
                            "bundle is too big: %d bytes. "
                            + "max allowed size is %s MB" % bundlesizelimitmb
                        )
                        raise error.Abort(error_msg % (len(bundledata),))
                    key = store.write(bundledata)

        with logservicecall(log, "index", newheadscount=newheadscount), index:
            if key:
                index.addbundle(key, nodesctx)
            if bookmark and bookmarknode:
                index.addbookmark(bookmark, bookmarknode, False)
        log(
            constants.scratchbranchparttype,
            eventtype="success",
            elapsedms=(time.time() - parthandlerstart) * 1000,
        )

        fillmetadatabranchpattern = op.repo.ui.config(
            "infinitepush", "fillmetadatabranchpattern", ""
        )
        if bookmark and fillmetadatabranchpattern:
            __, __, matcher = util.stringmatcher(fillmetadatabranchpattern)
            if matcher(bookmark):
                _asyncsavemetadata(op.repo.root, [ctx.hex() for ctx in nodesctx])
    except Exception as e:
        log(
            constants.scratchbranchparttype,
            eventtype="failure",
            elapsedms=(time.time() - parthandlerstart) * 1000,
            errormsg=str(e),
        )
        raise
    finally:
        if bundle:
            bundle.close()


def _getrevs(bundle, oldnode, force, bookmark):
    "extracts and validates the revs to be imported"
    revs = [bundle[r] for r in bundle.revs("sort(bundle())")]

    # new bookmark
    if oldnode is None:
        return revs

    # Fast forward update
    if oldnode in bundle and list(bundle.set("bundle() & %s::", oldnode)):
        return revs

    # Forced non-fast forward update
    if force:
        return revs
    else:
        raise error.Abort(
            _("non-forward push"), hint=_("use --non-forward-move to override")
        )


def _asyncsavemetadata(root, nodes):
    """starts a separate process that fills metadata for the nodes

    This function creates a separate process and doesn't wait for it's
    completion. This was done to avoid slowing down pushes
    """

    maxnodes = 50
    if len(nodes) > maxnodes:
        return
    nodesargs = []
    for node in nodes:
        nodesargs.append("--node")
        nodesargs.append(node)
    with open(os.devnull, "w+b") as devnull:
        cmdline = [
            util.hgexecutable(),
            "debugfillinfinitepushmetadata",
            "-R",
            root,
        ] + nodesargs
        # Process will run in background. We don't care about the return code
        subprocess.Popen(
            cmdline,
            close_fds=True,
            shell=False,
            stdin=devnull,
            stdout=devnull,
            stderr=devnull,
        )
