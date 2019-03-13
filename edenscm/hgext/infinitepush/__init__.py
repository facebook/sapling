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

import collections
import contextlib
import errno
import functools
import json
import logging
import os
import random
import re
import socket
import struct
import subprocess
import sys
import tempfile
import time

from edenscm.mercurial import (
    bundle2,
    changegroup,
    cmdutil,
    commands,
    discovery,
    encoding,
    error,
    exchange,
    extensions,
    hg,
    hintutil,
    i18n,
    localrepo,
    mutation,
    node as nodemod,
    obsolete,
    peer,
    phases,
    pushkey,
    ui as uimod,
    util,
    visibility,
    wireproto,
)
from edenscm.mercurial.commands import debug as debugcommands

from . import bundleparts, common, infinitepushcommands


copiedpart = bundleparts.copiedpart
getscratchbranchparts = bundleparts.getscratchbranchparts
scratchbookmarksparttype = bundleparts.scratchbookmarksparttype
scratchbranchparttype = bundleparts.scratchbranchparttype
scratchmutationparttype = bundleparts.scratchmutationparttype

batchable = peer.batchable
bin = nodemod.bin
decodelist = wireproto.decodelist
encodelist = wireproto.encodelist
future = peer.future
hex = nodemod.hex
_ = i18n._
wrapcommand = extensions.wrapcommand
wrapfunction = extensions.wrapfunction
unwrapfunction = extensions.unwrapfunction
repository = hg.repository

pushrebaseparttype = "b2x:rebase"
experimental = "experimental"
configbookmark = "server-bundlestore-bookmark"
configcreate = "server-bundlestore-create"
configscratchpush = "infinitepush-scratchpush"
confignonforwardmove = "non-forward-move"

cmdtable = infinitepushcommands.cmdtable
_scratchbranchmatcher = lambda x: False
_maybehash = re.compile(r"^[a-f0-9]+$").search

colortable = {
    "commitcloud.changeset": "green",
    "commitcloud.meta": "bold",
    "commitcloud.commitcloud": "yellow",
}


def _buildexternalbundlestore(ui):
    put_args = ui.configlist("infinitepush", "put_args", [])
    put_binary = ui.config("infinitepush", "put_binary")
    if not put_binary:
        raise error.Abort("put binary is not specified")
    get_args = ui.configlist("infinitepush", "get_args", [])
    get_binary = ui.config("infinitepush", "get_binary")
    if not get_binary:
        raise error.Abort("get binary is not specified")
    from . import store

    return store.externalbundlestore(put_binary, put_args, get_binary, get_args)


def _buildsqlindex(ui):
    sqlhost = ui.config("infinitepush", "sqlhost")
    if not sqlhost:
        raise error.Abort(_("please set infinitepush.sqlhost"))
    host, port, db, user, password = sqlhost.split(":")
    reponame = ui.config("infinitepush", "reponame")
    if not reponame:
        raise error.Abort(_("please set infinitepush.reponame"))

    logfile = ui.config("infinitepush", "logfile", "")
    waittimeout = ui.configint("infinitepush", "waittimeout", 300)
    locktimeout = ui.configint("infinitepush", "locktimeout", 120)
    shorthasholdrevthreshold = ui.configint(
        "infinitepush", "shorthasholdrevthreshold", 60
    )
    from . import sqlindexapi

    return sqlindexapi.sqlindexapi(
        reponame,
        host,
        port,
        db,
        user,
        password,
        logfile,
        _getloglevel(ui),
        shorthasholdrevthreshold=shorthasholdrevthreshold,
        waittimeout=waittimeout,
        locktimeout=locktimeout,
    )


def _getloglevel(ui):
    loglevel = ui.config("infinitepush", "loglevel", "DEBUG")
    numeric_loglevel = getattr(logging, loglevel.upper(), None)
    if not isinstance(numeric_loglevel, int):
        raise error.Abort(_("invalid log level %s") % loglevel)
    return numeric_loglevel


def _tryhoist(ui, remotebookmark):
    """returns a bookmarks with hoisted part removed

    Remotenames extension has a 'hoist' config that allows to use remote
    bookmarks without specifying remote path. For example, 'hg update master'
    works as well as 'hg update remote/master'. We want to allow the same in
    infinitepush.
    """

    if common.isremotebooksenabled(ui):
        hoist = ui.config("remotenames", "hoist") + "/"
        if remotebookmark.startswith(hoist):
            return remotebookmark[len(hoist) :]
    return remotebookmark


def _debugbundle2part(orig, ui, part, all, **opts):
    if part.type == scratchmutationparttype:
        entries = mutation.mutationstore.unbundle(part.read())
        ui.write(("    %s entries\n") % len(entries))
        for entry in entries:
            fold = entry.fold()
            if fold:
                pred = ",".join([hex(f) for f in fold])
            else:
                pred = hex(entry.pred())
            succ = hex(entry.succ())
            split = entry.split()
            if split:
                succ = ",".join([hex(s) for s in split] + succ)
            ui.write(
                ("      %s -> %s (%s by %s at %s)\n")
                % (pred, succ, entry.op(), entry.user(), entry.time())
            )

    orig(ui, part, all, **opts)


class bundlestore(object):
    def __init__(self, repo):
        self._repo = repo
        storetype = self._repo.ui.config("infinitepush", "storetype", "")
        if storetype == "disk":
            from . import store

            self.store = store.filebundlestore(self._repo.ui, self._repo)
        elif storetype == "external":
            self.store = _buildexternalbundlestore(self._repo.ui)
        else:
            raise error.Abort(
                _("unknown infinitepush store type specified %s") % storetype
            )

        indextype = self._repo.ui.config("infinitepush", "indextype", "")
        if indextype == "disk":
            from . import fileindexapi

            self.index = fileindexapi.fileindexapi(self._repo)
        elif indextype == "sql":
            self.index = _buildsqlindex(self._repo.ui)
        else:
            raise error.Abort(
                _("unknown infinitepush index type specified %s") % indextype
            )


def _isserver(ui):
    return ui.configbool("infinitepush", "server")


def reposetup(ui, repo):
    if _isserver(ui) and repo.local():
        repo.bundlestore = bundlestore(repo)


def uisetup(ui):
    # remotenames circumvents the default push implementation entirely, so make
    # sure we load after it so that we wrap it.
    order = extensions._order
    order.remove("infinitepush")
    order.append("infinitepush")
    extensions._order = order


def extsetup(ui):
    commonsetup(ui)
    if _isserver(ui):
        serverextsetup(ui)
    else:
        clientextsetup(ui)


def commonsetup(ui):
    wireproto.commands["listkeyspatterns"] = (
        wireprotolistkeyspatterns,
        "namespace patterns",
    )
    wireproto.commands["knownnodes"] = (wireprotoknownnodes, "nodes *")
    scratchbranchpat = ui.config("infinitepush", "branchpattern")
    if scratchbranchpat:
        global _scratchbranchmatcher
        kind, pat, _scratchbranchmatcher = util.stringmatcher(scratchbranchpat)
    extensions.wrapfunction(debugcommands, "_debugbundle2part", _debugbundle2part)


def serverextsetup(ui):
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

    wrapfunction(localrepo.localrepository, "listkeys", localrepolistkeys)
    wireproto.commands["lookup"] = (_lookupwrap(wireproto.commands["lookup"][0]), "key")
    wrapfunction(exchange, "getbundlechunks", getbundlechunks)
    wrapfunction(bundle2, "processparts", processparts)

    if util.safehasattr(wireproto, "_capabilities"):
        extensions.wrapfunction(wireproto, "_capabilities", _capabilities)
    else:
        extensions.wrapfunction(wireproto, "capabilities", _capabilities)


def _capabilities(orig, repo, proto):
    caps = orig(repo, proto)
    caps.append("listkeyspatterns")
    caps.append("knownnodes")
    return caps


def clientextsetup(ui):
    entry = wrapcommand(commands.table, "push", _push)
    # Don't add the 'to' arg if it already exists
    if not any(a for a in entry[1] if a[1] == "to"):
        entry[1].append(("", "to", "", _("push revs to this bookmark")))

    if not any(a for a in entry[1] if a[1] == "non-forward-move"):
        entry[1].append(
            (
                "",
                "non-forward-move",
                None,
                _("allows moving a remote bookmark to an " "arbitrary place"),
            )
        )

    if not any(a for a in entry[1] if a[1] == "create"):
        entry[1].append(("", "create", None, _("create a new remote bookmark")))

    entry[1].append(
        ("", "bundle-store", None, _("force push to go to bundle store (EXPERIMENTAL)"))
    )

    bookcmd = extensions.wrapcommand(commands.table, "bookmarks", exbookmarks)
    bookcmd[1].append(
        (
            "",
            "list-remote",
            None,
            "list remote bookmarks. "
            "Positional arguments are interpreted as wildcard patterns. "
            "Only allowed wildcard is '*' in the end of the pattern. "
            "If no positional arguments are specified then it will list "
            'the most "important" remote bookmarks. '
            "Otherwise it will list remote bookmarks "
            "that match at least one pattern "
            "",
        )
    )
    bookcmd[1].append(
        ("", "remote-path", "", "name of the remote path to list the bookmarks")
    )

    wrapcommand(commands.table, "pull", _pull)
    wrapcommand(commands.table, "update", _update)

    wrapfunction(bundle2, "_addpartsfromopts", _addpartsfromopts)

    wireproto.wirepeer.listkeyspatterns = listkeyspatterns
    wireproto.wirepeer.knownnodes = knownnodes

    # Move infinitepush part before pushrebase part
    # to avoid generation of both parts.
    partorder = exchange.b2partsgenorder
    index = partorder.index("changeset")
    if pushrebaseparttype in partorder:
        index = min(index, partorder.index(pushrebaseparttype))
    partorder.insert(index, partorder.pop(partorder.index(scratchbranchparttype)))


def _showbookmarks(ui, bookmarks, **opts):
    # Copy-paste from commands.py
    fm = ui.formatter("bookmarks", opts)
    for bmark, n in sorted(bookmarks.iteritems()):
        fm.startitem()
        if not ui.quiet:
            fm.plain("   ")
        fm.write("bookmark", "%s", bmark)
        pad = " " * (25 - encoding.colwidth(bmark))
        fm.condwrite(not ui.quiet, "node", pad + " %s", n)
        fm.plain("\n")
    fm.end()


def exbookmarks(orig, ui, repo, *names, **opts):
    pattern = opts.get("list_remote")
    delete = opts.get("delete")
    remotepath = opts.get("remote_path")
    path = ui.paths.getpath(remotepath or None, default=("default"))
    if pattern:
        destpath = path.pushloc or path.loc
        other = hg.peer(repo, opts, destpath)
        if not names:
            raise error.Abort(
                "--list-remote requires a bookmark pattern",
                hint='use "hg book" to get a list of your local bookmarks',
            )
        else:
            fetchedbookmarks = other.listkeyspatterns("bookmarks", patterns=names)
        _showbookmarks(ui, fetchedbookmarks, **opts)
        return
    elif delete and "remotenames" in extensions._extensions:
        existing_local_bms = set(repo._bookmarks.keys())
        scratch_bms = []
        other_bms = []
        for name in names:
            if _scratchbranchmatcher(name) and name not in existing_local_bms:
                scratch_bms.append(name)
            else:
                other_bms.append(name)

        if len(scratch_bms) > 0:
            if remotepath == "":
                remotepath = "default"
            _deleteinfinitepushbookmarks(ui, repo, remotepath, scratch_bms)

        if len(other_bms) > 0 or len(scratch_bms) == 0:
            return orig(ui, repo, *other_bms, **opts)
    else:
        return orig(ui, repo, *names, **opts)


def _addpartsfromopts(orig, ui, repo, bundler, *args, **kwargs):
    """ adds a stream level part to bundle2 storing whether this is an
    infinitepush bundle or not """
    if ui.configbool("infinitepush", "bundle-stream", False):
        bundler.addparam("infinitepush", True)
    return orig(ui, repo, bundler, *args, **kwargs)


def wireprotolistkeyspatterns(repo, proto, namespace, patterns):
    patterns = decodelist(patterns)
    d = repo.listkeys(encoding.tolocal(namespace), patterns).iteritems()
    return pushkey.encodekeys(d)


def localrepolistkeys(orig, self, namespace, patterns=None):
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


@batchable
def listkeyspatterns(self, namespace, patterns):
    if not self.capable("pushkey"):
        yield {}, None
    f = future()
    self.ui.debug(
        'preparing listkeys for "%s" with pattern "%s"\n' % (namespace, patterns)
    )
    yield {
        "namespace": encoding.fromlocal(namespace),
        "patterns": encodelist(patterns),
    }, f
    d = f.value
    self.ui.debug('received listkey for "%s": %i bytes\n' % (namespace, len(d)))
    yield pushkey.decodekeys(d)


@batchable
def knownnodes(self, nodes):
    f = future()
    yield {"nodes": encodelist(nodes)}, f
    d = f.value
    try:
        yield [bool(int(b)) for b in d]
    except ValueError:
        error.Abort(error.ResponseError(_("unexpected response:"), d))


def _readbundlerevs(bundlerepo):
    return list(bundlerepo.revs("bundle()"))


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

    try:
        treemod = extensions.find("treemanifest")
        remotefilelog = extensions.find("remotefilelog")
    except KeyError:
        pass
    else:
        # This parsing should be refactored to be shared with
        # exchange.getbundlechunks. But I'll do that in a separate diff.
        if bundlecaps is None:
            bundlecaps = set()
        b2caps = {}
        for bcaps in bundlecaps:
            if bcaps.startswith("bundle2="):
                blob = util.urlreq.unquote(bcaps[len("bundle2=") :])
                b2caps.update(bundle2.decodecaps(blob))

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

    return parts


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


# TODO(stash): remove copy-paste from upstream hg
def _decodebundle2caps(bundlecaps):
    b2caps = {}
    for bcaps in bundlecaps:
        if bcaps.startswith("bundle2="):
            blob = util.urlreq.unquote(bcaps[len("bundle2=") :])
            b2caps.update(bundle2.decodecaps(blob))
    return b2caps


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
                    newbundlefile = common.downloadbundle(repo, head)
                    bundlepath = "bundle:%s+%s" % (repo.root, newbundlefile)
                    bundlerepo = repository(repo.ui, bundlepath)

                    allbundlestocleanup.append((bundlerepo, newbundlefile))
                    bundlerevs = set(_readbundlerevs(bundlerepo))
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

        wrapfunction(localrepo.localrepository, "listkeys", _listkeys)
        wrappedlistkeys = True
        heads = list((set(newheads) | set(heads)) - set(scratchheads))
        result = orig(repo, source, heads=heads, bundlecaps=bundlecaps, **kwargs)
    finally:
        if wrappedchangegrouppart:
            exchange.getbundle2partsmapping["changegroup"] = oldchangegrouppart
        if wrappedlistkeys:
            unwrapfunction(localrepo.localrepository, "listkeys", _listkeys)
    return result


def _lookupwrap(orig):
    def _lookup(repo, proto, key):
        localkey = encoding.tolocal(key)

        if isinstance(localkey, str) and _scratchbranchmatcher(localkey):
            scratchnode = repo.bundlestore.index.getnode(localkey)
            if scratchnode:
                return "%s %s\n" % (1, scratchnode)
            else:
                return "%s %s\n" % (0, "scratch branch %s not found" % localkey)
        else:
            try:
                r = hex(repo.lookup(localkey))
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


def wireprotoknownnodes(repo, proto, nodes, others):
    """similar to 'known' but also check in infinitepush storage"""
    nodes = decodelist(nodes)
    knownlocally = repo.known(nodes)
    for index, known in enumerate(knownlocally):
        # TODO: make a single query to the bundlestore.index
        if not known and repo.bundlestore.index.getnodebyprefix(hex(nodes[index])):
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


def _update(orig, ui, repo, node=None, rev=None, **opts):
    """commit cloud (infinitepush) extension for hg up
    `hg up` will access:
    * local repo
    * hidden commits
    * remote commits
    * commit cloud (infinitepush) storage
    """
    if rev and node:
        raise error.Abort(_("please specify just one revision"))

    unfi = repo.unfiltered()
    if not opts.get("date") and (rev or node) not in unfi:
        mayberemote = rev or node
        mayberemote = _tryhoist(ui, mayberemote)
        dopull = False
        kwargs = {}
        if _scratchbranchmatcher(mayberemote):
            dopull = True
            kwargs["bookmark"] = [mayberemote]
        elif _maybehash(mayberemote):
            dopull = True
            kwargs["rev"] = [mayberemote]

        if dopull:
            ui.warn(
                _("'%s' does not exist locally - looking for it remotely...\n")
                % mayberemote
            )
            # Try pulling node from remote repo
            pullstarttime = time.time()

            try:
                (pullcmd, pullopts) = cmdutil.getcmdanddefaultopts(
                    "pull", commands.table
                )
                pullopts.update(kwargs)
                # Prefer to pull from 'infinitepush' path if it exists.
                # 'infinitepush' path has both infinitepush and non-infinitepush
                # revisions, so pulling from it is safer.
                # This is useful for dogfooding other hg backend that stores
                # only public commits (e.g. Mononoke)
                with _resetinfinitepushpath(ui):
                    pullcmd(ui, unfi, **pullopts)
            except Exception:
                remoteerror = str(sys.exc_info()[1])
                replacements = {
                    "commitcloud.changeset": ("changeset:",),
                    "commitcloud.meta": ("date:", "summary:", "author:"),
                    "commitcloud.commitcloud": ("#commitcloud",),
                }
                for label, keywords in replacements.iteritems():
                    for kw in keywords:
                        remoteerror = remoteerror.replace(kw, ui.label(kw, label))

                ui.warn(_("pull failed: %s\n") % remoteerror)

                # User updates to own commit from Commit Cloud
                if ui.username() in remoteerror:
                    hintutil.trigger("commitcloud-sync-education", ui)
            else:
                ui.warn(_("'%s' found remotely\n") % mayberemote)
                pulltime = time.time() - pullstarttime
                ui.warn(_("pull finished in %.3f sec\n") % pulltime)

    try:
        return orig(ui, repo, node, rev, **opts)
    except Exception:
        # Show the triggered hints anyway
        hintutil.show(ui)
        raise


@contextlib.contextmanager
def _resetinfinitepushpath(ui):
    """
    Sets "default" path to "infinitepush" path and deletes "infinitepush" path.
    In some cases (e.g. when testing new hg backend which doesn't have commit cloud
    commits) we want to do normal `hg pull` from "default" path but `hg pull -r HASH`
    from "infinitepush" path if it's present. This is better than just setting
    another path because of "remotenames" extension. Pulling or pushing to
    another path will add lots of new remote bookmarks and that can be slow
    and slow down smartlog.
    """

    overrides = {}
    if "infinitepush" in ui.paths:
        overrides[("paths", "default")] = ui.paths["infinitepush"].loc
        overrides[("paths", "infinitepush")] = "!"
        with ui.configoverride(overrides, "infinitepush"):
            loc, sub = ui.configsuboptions("paths", "default")
            ui.paths["default"] = uimod.path(ui, "default", rawloc=loc, suboptions=sub)
            del ui.paths["infinitepush"]
            yield
    else:
        yield


def _pull(orig, ui, repo, source="default", **opts):
    # If '-r' or '-B' option is set, then prefer to pull from 'infinitepush' path
    # if it exists. 'infinitepush' path has both infinitepush and non-infinitepush
    # revisions, so pulling from it is safer.
    # This is useful for dogfooding other hg backend that stores only public commits
    # (e.g. Mononoke)
    if opts.get("rev") or opts.get("bookmark"):
        with _resetinfinitepushpath(ui):
            return _dopull(orig, ui, repo, source, **opts)

    return _dopull(orig, ui, repo, source, **opts)


def _dopull(orig, ui, repo, source="default", **opts):
    # Copy paste from `pull` command
    source, branches = hg.parseurl(ui.expandpath(source), opts.get("branch"))

    scratchbookmarks = {}
    unfi = repo.unfiltered()
    unknownnodes = []
    for rev in opts.get("rev", []):
        if rev not in unfi:
            unknownnodes.append(rev)
    if opts.get("bookmark"):
        bookmarks = []
        revs = opts.get("rev") or []
        for bookmark in opts.get("bookmark"):
            if _scratchbranchmatcher(bookmark):
                # rev is not known yet
                # it will be fetched with listkeyspatterns next
                scratchbookmarks[bookmark] = "REVTOFETCH"
            else:
                bookmarks.append(bookmark)

        if scratchbookmarks:
            other = hg.peer(repo, opts, source)
            fetchedbookmarks = other.listkeyspatterns(
                "bookmarks", patterns=scratchbookmarks
            )
            for bookmark in scratchbookmarks:
                if bookmark not in fetchedbookmarks:
                    raise error.Abort("remote bookmark %s not found!" % bookmark)
                scratchbookmarks[bookmark] = fetchedbookmarks[bookmark]
                revs.append(fetchedbookmarks[bookmark])
        opts["bookmark"] = bookmarks
        opts["rev"] = revs

    # Pulling revisions that were filtered results in a error.
    # Let's revive them.
    unfi = repo.unfiltered()
    torevive = []
    for rev in opts.get("rev", []):
        try:
            repo[rev]
        except error.FilteredRepoLookupError:
            torevive.append(rev)
        except error.RepoLookupError:
            pass
    obsolete.revive([unfi[r] for r in torevive])
    visibility.add(repo, [unfi[r].node() for r in torevive])

    if scratchbookmarks or unknownnodes:
        # Set anyincoming to True
        wrapfunction(discovery, "findcommonincoming", _findcommonincoming)
    try:
        # Remote scratch bookmarks will be deleted because remotenames doesn't
        # know about them. Let's save it before pull and restore after
        remotescratchbookmarks = _readscratchremotebookmarks(ui, repo, source)
        result = orig(ui, repo, source, **opts)
        # TODO(stash): race condition is possible
        # if scratch bookmarks was updated right after orig.
        # But that's unlikely and shouldn't be harmful.
        if common.isremotebooksenabled(ui):
            remotescratchbookmarks.update(scratchbookmarks)
            _saveremotebookmarks(repo, remotescratchbookmarks, source)
        else:
            _savelocalbookmarks(repo, scratchbookmarks)
        return result
    finally:
        if scratchbookmarks:
            unwrapfunction(discovery, "findcommonincoming")


def _readscratchremotebookmarks(ui, repo, other):
    if common.isremotebooksenabled(ui):
        remotenamesext = extensions.find("remotenames")
        remotepath = remotenamesext.activepath(repo.ui, other)
        result = {}
        # Let's refresh remotenames to make sure we have it up to date
        # Seems that `repo.names['remotebookmarks']` may return stale bookmarks
        # and it results in deleting scratch bookmarks. Our best guess how to
        # fix it is to use `clearnames()`
        repo._remotenames.clearnames()
        for remotebookmark in repo.names["remotebookmarks"].listnames(repo):
            path, bookname = remotenamesext.splitremotename(remotebookmark)
            if path == remotepath and _scratchbranchmatcher(bookname):
                nodes = repo.names["remotebookmarks"].nodes(repo, remotebookmark)
                if nodes:
                    result[bookname] = hex(nodes[0])
        return result
    else:
        return {}


def _saveremotebookmarks(repo, newbookmarks, remote):
    remotenamesext = extensions.find("remotenames")
    remotepath = remotenamesext.activepath(repo.ui, remote)
    bookmarks = {}
    remotenames = remotenamesext.readremotenames(repo)
    for hexnode, nametype, remote, rname in remotenames:
        if remote != remotepath:
            continue
        if nametype == "bookmarks":
            if rname in newbookmarks:
                # It's possible if we have a normal bookmark that matches
                # scratch branch pattern. In this case just use the current
                # bookmark node
                del newbookmarks[rname]
            bookmarks[rname] = hexnode

    for bookmark, hexnode in newbookmarks.iteritems():
        bookmarks[bookmark] = hexnode
    remotenamesext.saveremotenames(repo, remotepath, bookmarks)


def _savelocalbookmarks(repo, bookmarks):
    if not bookmarks:
        return
    with repo.wlock(), repo.lock(), repo.transaction("bookmark") as tr:
        changes = []
        for scratchbook, node in bookmarks.iteritems():
            changectx = repo[node]
            changes.append((scratchbook, changectx.node()))
        repo._bookmarks.applychanges(repo, tr, changes)


def _findcommonincoming(orig, *args, **kwargs):
    common, inc, remoteheads = orig(*args, **kwargs)
    return common, True, remoteheads


def _push(orig, ui, repo, dest=None, *args, **opts):
    bookmark = opts.get("to") or ""
    create = opts.get("create") or False

    oldphasemove = None
    overrides = {
        (experimental, configbookmark): bookmark,
        (experimental, configcreate): create,
    }

    with ui.configoverride(overrides, "infinitepush"):
        scratchpush = opts.get("bundle_store")
        if _scratchbranchmatcher(bookmark):
            # Hack to fix interaction with remotenames. Remotenames push
            # '--to' bookmark to the server but we don't want to push scratch
            # bookmark to the server. Let's delete '--to' and '--create' and
            # also set allow_anon to True (because if --to is not set
            # remotenames will think that we are pushing anonymoush head)
            if "to" in opts:
                del opts["to"]
            if "create" in opts:
                del opts["create"]
            opts["allow_anon"] = True
            scratchpush = True
            # bundle2 can be sent back after push (for example, bundle2
            # containing `pushkey` part to update bookmarks)
            ui.setconfig(experimental, "bundle2.pushback", True)

        ui.setconfig(
            experimental,
            confignonforwardmove,
            opts.get("non_forward_move"),
            "--non-forward-move",
        )
        if scratchpush:
            ui.setconfig(experimental, configscratchpush, True)
            oldphasemove = wrapfunction(exchange, "_localphasemove", _phasemove)
            path = ui.paths.getpath(
                dest, default=("infinitepush", "default-push", "default")
            )
        else:
            path = ui.paths.getpath(dest, default=("default-push", "default"))
        # Copy-paste from `push` command
        if not path:
            raise error.Abort(
                _("default repository not configured!"),
                hint=_("see 'hg help config.paths'"),
            )
        dest = path.pushloc or path.loc
        if dest.startswith("svn+") and scratchpush:
            raise error.Abort(
                "infinite push does not work with svn repo",
                hint="did you forget to `hg push default`?",
            )
        # Remote scratch bookmarks will be deleted because remotenames doesn't
        # know about them. Let's save it before push and restore after
        remotescratchbookmarks = _readscratchremotebookmarks(ui, repo, dest)
        result = orig(ui, repo, dest, *args, **opts)
        if common.isremotebooksenabled(ui):
            if bookmark and scratchpush:
                other = hg.peer(repo, opts, dest)
                fetchedbookmarks = other.listkeyspatterns(
                    "bookmarks", patterns=[bookmark]
                )
                remotescratchbookmarks.update(fetchedbookmarks)
            _saveremotebookmarks(repo, remotescratchbookmarks, dest)
    if oldphasemove:
        exchange._localphasemove = oldphasemove
    return result


def _deleteinfinitepushbookmarks(ui, repo, path, names):
    """Prune remote names by removing the bookmarks we don't want anymore,
    then writing the result back to disk
    """
    remotenamesext = extensions.find("remotenames")

    # remotename format is:
    # (node, nametype ("bookmarks"), remote, name)
    nametype_idx = 1
    remote_idx = 2
    name_idx = 3
    remotenames = [
        remotename
        for remotename in remotenamesext.readremotenames(repo)
        if remotename[remote_idx] == path
    ]
    remote_bm_names = [
        remotename[name_idx]
        for remotename in remotenames
        if remotename[nametype_idx] == "bookmarks"
    ]

    for name in names:
        if name not in remote_bm_names:
            raise error.Abort(
                _("infinitepush bookmark '{}' does not exist " "in path '{}'").format(
                    name, path
                )
            )

    bookmarks = {}
    for node, nametype, remote, name in remotenames:
        if nametype == "bookmarks" and name not in names:
            bookmarks[name] = node

    remotenamesext.saveremotenames(repo, path, bookmarks)


def _phasemove(orig, pushop, nodes, phase=phases.public):
    """prevent commits from being marked public

    Since these are going to a scratch branch, they aren't really being
    published."""

    if phase != phases.public:
        orig(pushop, nodes, phase)


@exchange.b2partsgenerator(scratchbranchparttype)
def partgen(pushop, bundler):
    bookmark = pushop.ui.config(experimental, configbookmark)
    create = pushop.ui.configbool(experimental, configcreate)
    scratchpush = pushop.ui.configbool(experimental, configscratchpush)
    if "changesets" in pushop.stepsdone or not scratchpush:
        return

    if scratchbranchparttype not in bundle2.bundle2caps(pushop.remote):
        return

    pushop.stepsdone.add("changesets")
    pushop.stepsdone.add("treepack")
    if not pushop.outgoing.missing:
        pushop.ui.status(_("no changes found\n"))
        pushop.cgresult = 0
        return

    # This parameter tells the server that the following bundle is an
    # infinitepush. This let's it switch the part processing to our infinitepush
    # code path.
    bundler.addparam("infinitepush", "True")

    nonforwardmove = pushop.force or pushop.ui.configbool(
        experimental, confignonforwardmove
    )
    scratchparts = getscratchbranchparts(
        pushop.repo,
        pushop.remote,
        pushop.outgoing,
        nonforwardmove,
        pushop.ui,
        bookmark,
        create,
    )

    for scratchpart in scratchparts:
        bundler.addpart(scratchpart)

    def handlereply(op):
        # server either succeeds or aborts; no code to read
        pushop.cgresult = 1

    return handlereply


bundle2.capabilities[scratchbranchparttype] = ()
bundle2.capabilities[scratchbookmarksparttype] = ()
bundle2.capabilities[scratchmutationparttype] = ()


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


def processparts(orig, repo, op, unbundler):
    if unbundler.params.get("infinitepush") != "True":
        return orig(repo, op, unbundler)

    handleallparts = repo.ui.configbool("infinitepush", "storeallparts")

    partforwardingwhitelist = [scratchmutationparttype]
    try:
        treemfmod = extensions.find("treemanifest")
        partforwardingwhitelist.append(treemfmod.TREEGROUP_PARTTYPE2)
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
            elif part.type == scratchbranchparttype:
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
                    op.records.add(scratchbranchparttype + "_skippushkey", True)
                    op.records.add(scratchbranchparttype + "_skipphaseheads", True)
            elif part.type == scratchbookmarksparttype:
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
                fp = os.fdopen(fd, "wb")
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


def storebundle(op, params, bundlefile):
    log = _getorcreateinfinitepushlogger(op)
    parthandlerstart = time.time()
    log(scratchbranchparttype, eventtype="start")
    index = op.repo.bundlestore.index
    store = op.repo.bundlestore.store
    op.records.add(scratchbranchparttype + "_skippushkey", True)

    bundle = None
    try:  # guards bundle
        bundlepath = "bundle:%s+%s" % (op.repo.root, bundlefile)
        bundle = repository(op.repo.ui, bundlepath)

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
        plural = "s" if len(revs) > 1 else ""
        op.repo.ui.warn(_("pushing %s commit%s:\n") % (len(revs), plural))
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
        # If there's a bookmark specified, there should be only one head,
        # so we choose the last node, which will be that head.
        # If a bug or malicious client allows there to be a bookmark
        # with multiple heads, we will place the bookmark on the last head.
        bookmarknode = nodesctx[-1].hex() if nodesctx else None
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
            if bookmark:
                index.addbookmark(bookmark, bookmarknode)
        log(
            scratchbranchparttype,
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
            scratchbranchparttype,
            eventtype="failure",
            elapsedms=(time.time() - parthandlerstart) * 1000,
            errormsg=str(e),
        )
        raise
    finally:
        if bundle:
            bundle.close()


@bundle2.b2streamparamhandler("infinitepush")
def processinfinitepush(unbundler, param, value):
    """ process the bundle2 stream level parameter containing whether this push
    is an infinitepush or not. """
    if value and unbundler.ui.configbool("infinitepush", "bundle-stream", False):
        pass


@bundle2.parthandler(
    scratchbranchparttype, ("bookmark", "create", "force", "cgversion")
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
        storebundle(op, part.params, bundlefile)
    finally:
        try:
            os.unlink(bundlefile)
        except OSError as e:
            if e.errno != errno.ENOENT:
                raise

    return 1


@bundle2.parthandler(scratchbookmarksparttype)
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
    log = _getorcreateinfinitepushlogger(op)
    with logservicecall(log, scratchbookmarksparttype), index:
        if todelete:
            index.deletebookmarks(todelete)
        if toinsert:
            index.addmanybookmarks(toinsert)


@bundle2.parthandler(scratchmutationparttype)
def bundle2scratchmutation(op, part):
    mutation.unbundle(op.repo, part.read())


def bundle2pushkey(orig, op, part):
    """Wrapper of bundle2.handlepushkey()

    The only goal is to skip calling the original function if flag is set.
    It's set if infinitepush push is happening.
    """
    if op.records[scratchbranchparttype + "_skippushkey"]:
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

    if op.records[scratchbranchparttype + "_skipphaseheads"]:
        return

    return orig(op, part)


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
    check on the server side if the nodes are backed up using 'lookup'

    TODO: deprecate this after supporting 'known' in infinitepush.
    """

    def isbackedup(node):
        try:
            with getconnection() as conn:
                conn.peer.lookup(node)
                return True
        except error.RepoError:
            return False

    return [isbackedup(node) for node in nodes]


def isbackedupnodes2(getconnection, nodes):
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
