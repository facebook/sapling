# Infinite push
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
""" store draft commits in the cloud

Configs::

    [infinitepush]
    # Server-side and client-side option. Pattern of the infinitepush bookmark
    branchpattern = PATTERN

    # Server or client
    server = False

    # Server-side option. Possible values: 'disk' or 'sql'. Fails if not set
    indextype = disk

    # Server-side option. Used only if indextype=sql.
    # Format: 'IP:PORT:DB_NAME:USER:PASSWORD'
    sqlhost = IP:PORT:DB_NAME:USER:PASSWORD

    # Server-side option. Used only if indextype=disk.
    # Filesystem path to the index store
    indexpath = PATH

    # Server-side option. Possible values: 'disk' or 'external'
    # Fails if not set
    storetype = disk

    # Server-side option.
    # Path to the binary that will save bundle to the bundlestore
    # Formatted cmd line will be passed to it (see `put_args`)
    put_binary = put

    # Server-side option. Used only if storetype=external.
    # Format cmd-line string for put binary. Placeholder: {filename}
    put_args = {filename}

    # Server-side option.
    # Path to the binary that get bundle from the bundlestore.
    # Formatted cmd line will be passed to it (see `get_args`)
    get_binary = get

    # Server-side option. Used only if storetype=external.
    # Format cmd-line string for get binary. Placeholders: {filename} {handle}
    get_args = {filename} {handle}

    # Server-side option
    logfile = FIlE

    # Server-side option
    loglevel = DEBUG

    # Server-side option. Used only if indextype=sql.
    # Sets mysql wait_timeout option.
    waittimeout = 300

    # Server-side option. Used only if indextype=sql.
    # Sets mysql innodb_lock_wait_timeout option.
    locktimeout = 120

    # Server-side option. Used only if indextype=sql.
    # limit number of days to generate warning on trying to
    # fetch too old commit for hg up / hg pull with short hash rev
    shorthasholdrevthreshold = 31

    # Server-side option. Used only if indextype=sql.
    # Name of the repository
    reponame = ''

    # Client-side option. Used by --list-remote option. List of remote scratch
    # patterns to list if no patterns are specified.
    defaultremotepatterns = ['*']

    # Server-side option. If bookmark that was pushed matches
    # `fillmetadatabranchpattern` then background
    # `hg debugfillinfinitepushmetadata` process will save metadata
    # in infinitepush index for nodes that are ancestor of the bookmark.
    fillmetadatabranchpattern = ''

    # Instructs infinitepush to forward all received bundle2 parts to the
    # bundle for storage. Defaults to False.
    storeallparts = True

    # Server-side option.  Maximum acceptable bundle size in megabytes.
    maxbundlesize = 500

    # Which compression algorithm to use for infinitepush bundles.
    bundlecompression = ZS

    [remotenames]
    # Client-side option
    # This option should be set only if remotenames extension is enabled.
    # Whether remote bookmarks are tracked by remotenames extension.
    bookmarks = True
"""

from __future__ import absolute_import

import errno
import json
import os
import struct
import tempfile

from edenscm.mercurial import (
    bundle2,
    changegroup,
    discovery,
    encoding,
    error,
    exchange,
    extensions,
    i18n,
    mutation,
    node as nodemod,
    pushkey,
    util,
    wireproto,
)
from edenscm.mercurial.commands import debug as debugcommands
from edenscm.mercurial.i18n import _

from . import bundleparts, bundlestore, client, common, infinitepushcommands, server


cmdtable = infinitepushcommands.cmdtable

colortable = {
    "commitcloud.changeset": "green",
    "commitcloud.meta": "bold",
    "commitcloud.commitcloud": "yellow",
}


def _debugbundle2part(orig, ui, part, all, **opts):
    if part.type == bundleparts.scratchmutationparttype:
        entries = mutation.mutationstore.unbundle(part.read())
        ui.write(("    %s entries\n") % len(entries))
        for entry in entries:
            pred = ",".join([nodemod.hex(p) for p in entry.preds()])
            succ = nodemod.hex(entry.succ())
            split = entry.split()
            if split:
                succ = ",".join([nodemod.hex(s) for s in split] + [succ])
            ui.write(
                ("      %s -> %s (%s by %s at %s)\n")
                % (pred, succ, entry.op(), entry.user(), entry.time())
            )

    orig(ui, part, all, **opts)


def reposetup(ui, repo):
    common.reposetup(ui, repo)
    if common.isserver(ui) and repo.local():
        repo.bundlestore = bundlestore.bundlestore(repo)


def uisetup(ui):
    # remotenames circumvents the default push implementation entirely, so make
    # sure we load after it so that we wrap it.
    order = extensions._order
    order.remove("infinitepush")
    order.append("infinitepush")
    extensions._order = order


def extsetup(ui):
    commonextsetup(ui)
    if common.isserver(ui):
        server.extsetup(ui)
    else:
        client.extsetup(ui)


def commonextsetup(ui):
    wireproto.commands["listkeyspatterns"] = (
        wireprotolistkeyspatterns,
        "namespace patterns",
    )
    wireproto.commands["knownnodes"] = (wireprotoknownnodes, "nodes *")
    extensions.wrapfunction(debugcommands, "_debugbundle2part", _debugbundle2part)


def wireprotolistkeyspatterns(repo, proto, namespace, patterns):
    patterns = wireproto.decodelist(patterns)
    d = repo.listkeys(encoding.tolocal(namespace), patterns).iteritems()
    return pushkey.encodekeys(d)


def wireprotoknownnodes(repo, proto, nodes, others):
    """similar to 'known' but also check in infinitepush storage"""
    nodes = wireproto.decodelist(nodes)
    knownlocally = repo.known(nodes)
    for index, known in enumerate(knownlocally):
        # TODO: make a single query to the bundlestore.index
        if not known and repo.bundlestore.index.getnodebyprefix(
            nodemod.hex(nodes[index])
        ):
            knownlocally[index] = True
    return "".join(b and "1" or "0" for b in knownlocally)


def _decodebookmarks(stream):
    sizeofjsonsize = struct.calcsize(">i")
    size = struct.unpack(">i", stream.read(sizeofjsonsize))[0]
    unicodedict = json.loads(stream.read(size))
    # python json module always returns unicode strings. We need to convert
    # it back to bytes string
    result = {}
    for bookmark, node in unicodedict.iteritems():
        bookmark = bookmark.encode("ascii")
        node = node.encode("ascii")
        result[bookmark] = node
    return result


bundle2.capabilities[bundleparts.scratchbranchparttype] = ()
bundle2.capabilities[bundleparts.scratchbookmarksparttype] = ()
bundle2.capabilities[bundleparts.scratchmutationparttype] = ()


@bundle2.b2streamparamhandler("infinitepush")
def processinfinitepush(unbundler, param, value):
    """ process the bundle2 stream level parameter containing whether this push
    is an infinitepush or not. """
    if value and unbundler.ui.configbool("infinitepush", "bundle-stream", False):
        pass


@bundle2.parthandler(
    bundleparts.scratchbranchparttype, ("bookmark", "create", "force", "cgversion")
)
def bundle2scratchbranch(op, part):
    """unbundle a bundle2 part containing a changegroup to store"""

    bundler = bundle2.bundle20(op.repo.ui)
    cgversion = part.params.get("cgversion", "01")
    cgpart = bundle2.bundlepart("changegroup", data=part.read())
    cgpart.addparam("version", cgversion)
    bundler.addpart(cgpart)
    buf = util.chunkbuffer(bundler.getchunks())

    fd, bundlefile = tempfile.mkstemp()
    try:
        try:
            fp = os.fdopen(fd, "wb")
            fp.write(buf.read())
        finally:
            fp.close()
        server.storebundle(op, part.params, bundlefile)
    finally:
        try:
            os.unlink(bundlefile)
        except OSError as e:
            if e.errno != errno.ENOENT:
                raise

    return 1


@bundle2.parthandler(bundleparts.scratchbookmarksparttype)
def bundle2scratchbookmarks(op, part):
    """Handler deletes bookmarks first then adds new bookmarks.
    """
    index = op.repo.bundlestore.index
    decodedbookmarks = _decodebookmarks(part)
    toinsert = {}
    todelete = []
    for bookmark, node in decodedbookmarks.iteritems():
        if node:
            toinsert[bookmark] = node
        else:
            todelete.append(bookmark)
    log = server._getorcreateinfinitepushlogger(op)
    with server.logservicecall(log, bundleparts.scratchbookmarksparttype), index:
        if todelete:
            index.deletebookmarks(todelete)
        if toinsert:
            index.addmanybookmarks(toinsert)


@bundle2.parthandler(bundleparts.scratchmutationparttype)
def bundle2scratchmutation(op, part):
    mutation.unbundle(op.repo, part.read())


def _deltaparent(orig, self, revlog, rev, p1, p2, prev):
    # This version of deltaparent prefers p1 over prev to use less space
    dp = revlog.deltaparent(rev)
    if dp == nodemod.nullrev and not revlog.storedeltachains:
        # send full snapshot only if revlog configured to do so
        return nodemod.nullrev
    return p1


def _createbundler(ui, repo, other):
    bundler = bundle2.bundle20(ui, bundle2.bundle2caps(other))
    compress = ui.config("infinitepush", "bundlecompression", "UN")
    bundler.setcompression(compress)
    # Disallow pushback because we want to avoid taking repo locks.
    # And we don't need pushback anyway
    capsblob = bundle2.encodecaps(bundle2.getrepocaps(repo, allowpushback=False))
    bundler.newpart("replycaps", data=capsblob)
    return bundler


def _sendbundle(bundler, other):
    stream = util.chunkbuffer(bundler.getchunks())
    try:
        reply = other.unbundle(stream, ["force"], other.url())
        # Look for an error part in the response.  Note that we don't apply
        # the reply bundle, as we're not expecting any response, except maybe
        # an error.  If we receive any extra parts, that is an error.
        for part in reply.iterparts():
            if part.type == "error:abort":
                raise bundle2.AbortFromPart(
                    part.params["message"], hint=part.params.get("hint")
                )
            elif part.type == "reply:changegroup":
                pass
            else:
                raise error.Abort(_("unexpected part in reply: %s") % part.type)
    except error.BundleValueError as exc:
        raise error.Abort(_("missing support for %s") % exc)


def pushbackupbundle(ui, repo, other, outgoing, bookmarks):
    """
    push a backup bundle to the server

    Pushes an infinitepush bundle containing the commits described in `outgoing`
    and the bookmarks described in `bookmarks` to the `other` server.
    """
    # Wrap deltaparent function to make sure that bundle takes less space
    # See _deltaparent comments for details
    extensions.wrapfunction(changegroup.cg2packer, "deltaparent", _deltaparent)
    try:
        bundler = _createbundler(ui, repo, other)
        bundler.addparam("infinitepush", "True")
        pushvarspart = bundler.newpart("pushvars")
        pushvarspart.addparam("BYPASS_READONLY", "True", mandatory=False)

        backup = False

        if outgoing and not outgoing.missing and not bookmarks:
            ui.status(_("nothing to back up\n"))
            return True

        if outgoing and outgoing.missing:
            backup = True
            parts = bundleparts.getscratchbranchparts(
                repo,
                other,
                outgoing,
                confignonforwardmove=False,
                ui=ui,
                bookmark=None,
                create=False,
                bookmarknode=None,
            )
            for part in parts:
                bundler.addpart(part)

        if bookmarks:
            backup = True
            bundler.addpart(bundleparts.getscratchbookmarkspart(other, bookmarks))

        if backup:
            _sendbundle(bundler, other)
        return backup
    finally:
        extensions.unwrapfunction(changegroup.cg2packer, "deltaparent", _deltaparent)


def pushbackupbundlewithdiscovery(ui, repo, other, heads, bookmarks):

    if heads:
        with ui.configoverride({("remotenames", "fastheaddiscovery"): False}):
            outgoing = discovery.findcommonoutgoing(repo, other, onlyheads=heads)
    else:
        outgoing = None

    return pushbackupbundle(ui, repo, other, outgoing, bookmarks)


def isbackedupnodes(getconnection, nodes):
    """
    check on the server side if the nodes are backed up using 'known' or 'knownnodes' commands
    """
    with getconnection() as conn:
        if "knownnodes" in conn.peer.capabilities():
            return conn.peer.knownnodes([nodemod.bin(n) for n in nodes])
        else:
            return conn.peer.known([nodemod.bin(n) for n in nodes])


def pushbackupbundledraftheads(ui, repo, getconnection, heads):
    """
    push a backup bundle containing draft heads to the server

    Pushes an infinitepush bundle containing the commits that are draft
    ancestors of `heads`, to the `other` server.
    """
    if heads:
        # Calculate the commits to back-up.  The bundle needs to cleanly
        # apply to the server, so we need to include the whole draft stack.
        commitstobackup = [ctx.node() for ctx in repo.set("draft() & ::%ln", heads)]

        # Calculate the parent commits of the commits we are backing up.
        # These are the public commits that should be on the server.
        parentcommits = [
            ctx.node() for ctx in repo.set("parents(roots(%ln))", commitstobackup)
        ]

        # Build a discovery object encapsulating the commits to backup.
        # Skip the actual discovery process, as we know exactly which
        # commits are missing.  For the common commits, include all the
        # parents of the commits we are sending.  In the unlikely event that
        # the server is missing public commits, we will try again with
        # discovery enabled.
        og = discovery.outgoing(repo, commonheads=parentcommits, missingheads=heads)
        og._missing = commitstobackup
        og._common = parentcommits
    else:
        og = None

    try:
        with getconnection() as conn:
            return pushbackupbundle(ui, repo, conn.peer, og, None)
    except Exception as e:
        ui.warn(_("push failed: %s\n") % e)
        ui.warn(_("retrying push with discovery\n"))
    with getconnection() as conn:
        return pushbackupbundlewithdiscovery(ui, repo, conn.peer, heads, None)


def pushbackupbundlestacks(ui, repo, getconnection, heads):
    # Push bundles containing the commits.  Initially attempt to push one
    # bundle for each stack (commits that share a single root).  If a stack is
    # too large, or if the push fails, and the stack has multiple heads, push
    # head-by-head.
    roots = repo.set("roots(draft() & ::%ls)", heads)
    newheads = set()
    failedheads = set()
    for root in roots:
        ui.status(_("backing up stack rooted at %s\n") % root)
        stack = [ctx.hex() for ctx in repo.set("(%n::%ls)", root.node(), heads)]
        if len(stack) == 0:
            continue

        stackheads = [ctx.hex() for ctx in repo.set("heads(%ls)", stack)]
        if len(stack) > 1000:
            # This stack is too large, something must have gone wrong
            ui.warn(
                _("not backing up excessively large stack rooted at %s (%d commits)")
                % (root, len(stack))
            )
            failedheads |= set(stackheads)
            continue

        if len(stack) < 20 and len(stackheads) > 1:
            # Attempt to push the whole stack.  This makes it easier on the
            # server when accessing one of the head commits, as the ancestors
            # will always be in the same bundle.
            try:
                if pushbackupbundledraftheads(
                    ui, repo, getconnection, [nodemod.bin(h) for h in stackheads]
                ):
                    newheads |= set(stackheads)
                    continue
                else:
                    ui.warn(_("failed to push stack bundle rooted at %s\n") % root)
            except Exception as e:
                ui.warn(_("push of stack %s failed: %s\n") % (root, e))
            ui.warn(_("retrying each head individually\n"))

        # The stack only has one head, is large, or pushing the whole stack
        # failed, push each head in turn.
        for head in stackheads:
            try:
                if pushbackupbundledraftheads(
                    ui, repo, getconnection, [nodemod.bin(head)]
                ):
                    newheads.add(head)
                    continue
                else:
                    ui.warn(
                        _("failed to push stack bundle with head %s\n")
                        % nodemod.short(nodemod.bin(head))
                    )
            except Exception as e:
                ui.warn(
                    _("push of head %s failed: %s\n")
                    % (nodemod.short(nodemod.bin(head)), e)
                )
            failedheads.add(head)

    return newheads, failedheads
