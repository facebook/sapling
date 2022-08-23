# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# fileserverclient.py - client for communicating with the cache process

from __future__ import absolute_import

import struct
import time
import traceback

from edenscm.mercurial import error, perftrace, pycompat, revlog, util, wireproto
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import bin

from . import constants, shallowutil, wirepack


# Statistics for debugging
fetchcost = 0
fetches = 0
fetched = 0
fetchmisses = 0

_lfsmod = None


def peersetup(ui, peer):
    class remotefilepeer(peer.__class__):
        @wireproto.batchable
        def getfile(self, file, node):
            if not self.capable("getfile"):
                raise error.Abort(
                    "configured remotefile server does not support getfile"
                )
            f = wireproto.future()
            yield {"file": file, "node": node}, f
            code, data = f.value.split("\0", 1)
            if int(code):
                raise error.LookupError(file, node, data)
            yield data

        @wireproto.batchable
        def getflogheads(self, path):
            if not self.capable("getflogheads"):
                raise error.Abort(
                    "configured remotefile server does not " "support getflogheads"
                )
            f = wireproto.future()
            yield {"path": path}, f
            heads = pycompat.decodeutf8(f.value).split("\n") if f.value else []
            yield heads

        def _updatecallstreamopts(self, command, opts):
            if command != "getbundle":
                return
            if "remotefilelog" not in shallowutil.peercapabilities(self):
                return
            if not util.safehasattr(self, "_localrepo"):
                return
            if constants.REQUIREMENT not in self._localrepo.requirements:
                return

            bundlecaps = opts.get("bundlecaps")
            if bundlecaps:
                bundlecaps = [bundlecaps]
            else:
                bundlecaps = []

            # shallow, includepattern, and excludepattern are a hacky way of
            # carrying over data from the local repo to this getbundle
            # command. We need to do it this way because bundle1 getbundle
            # doesn't provide any other place we can hook in to manipulate
            # getbundle args before it goes across the wire. Once we get rid
            # of bundle1, we can use bundle2's _pullbundle2extraprepare to
            # do this more cleanly.
            bundlecaps.append("remotefilelog")
            if self._localrepo.includepattern:
                patterns = "\0".join(self._localrepo.includepattern)
                includecap = "includepattern=" + patterns
                bundlecaps.append(includecap)
            if self._localrepo.excludepattern:
                patterns = "\0".join(self._localrepo.excludepattern)
                excludecap = "excludepattern=" + patterns
                bundlecaps.append(excludecap)
            opts["bundlecaps"] = ",".join(bundlecaps)

        def _callstream(self, cmd, **opts):
            self._updatecallstreamopts(cmd, opts)
            return super(remotefilepeer, self)._callstream(cmd, **opts)

    peer.__class__ = remotefilepeer


class getpackclient(object):
    def __init__(self, repo):
        self.repo = repo
        self.ui = repo.ui

    def _sendpackrequest(self, remote, fileids):
        """Formats and writes the given fileids to the remote as part of a
        getpackv1 call.
        """
        # Sort the requests by name, so we receive requests in batches by name
        grouped = {}
        for filename, node in fileids:
            grouped.setdefault(filename, set()).add(node)

        # Issue request
        pipeo = shallowutil.trygetattr(remote, ("_pipeo", "pipeo"))
        for filename, nodes in pycompat.iteritems(grouped):
            filename = pycompat.encodeutf8(filename)
            filenamelen = struct.pack(constants.FILENAMESTRUCT, len(filename))
            countlen = struct.pack(constants.PACKREQUESTCOUNTSTRUCT, len(nodes))
            rawnodes = b"".join(n for n in nodes)

            pipeo.write(b"%s%s%s%s" % (filenamelen, filename, countlen, rawnodes))
            pipeo.flush()
        pipeo.write(struct.pack(constants.FILENAMESTRUCT, 0))
        pipeo.flush()

    def _connect(self):
        return self.repo.connectionpool.get(
            self.repo.fallbackpath, reason="prefetchpacks"
        )

    def getpack(self, datastore, historystore, fileids):
        chunksize = self.ui.configint("remotefilelog", "prefetchchunksize", 200000)

        receiveddatalen = 0
        for start_id in range(0, len(fileids), chunksize):
            ids = fileids[start_id : start_id + chunksize]

            with self._connect() as conn:
                self.ui.metrics.gauge("ssh_getpack_revs", len(ids))
                self.ui.metrics.gauge("ssh_getpack_calls", 1)

                getpackversion = self.ui.configint("remotefilelog", "getpackversion")

                remote = conn.peer
                remote._callstream("getpackv%d" % getpackversion)

                self._sendpackrequest(remote, ids)

                pipei = shallowutil.trygetattr(remote, ("_pipei", "pipei"))

                receiveddata, _receivedhistory = wirepack.receivepack(
                    self.repo.ui, pipei, datastore, historystore, version=getpackversion
                )
                receiveddatalen += len(receiveddata)

        return receiveddatalen

    @perftrace.tracefunc("Fetch Pack")
    def prefetch(self, datastore, historystore, fileids):
        total = len(fileids)
        perftrace.tracevalue("Files requested", len(fileids))

        try:
            rcvd = None
            if self.repo.ui.configbool("remotefilelog", "retryprefetch"):
                retries = 0
                for backoff in [1, 5, 10, 20]:
                    try:
                        rcvd = self.getpack(datastore, historystore, fileids)
                        break
                    except (error.BadResponseError, error.NetworkError):
                        missingids = set()
                        missingids.update(datastore.getmissing(fileids))
                        missingids.update(historystore.getmissing(fileids))

                        fileids = list(missingids)

                        self.ui.warn(
                            _(
                                "Network connection dropped while fetching data, retrying after %d seconds\n"
                            )
                            % backoff
                        )
                        time.sleep(backoff)
                        retries += 1
                        continue
                if retries > 0:
                    perftrace.tracevalue("Retries", retries)

            if rcvd is None:
                rcvd = self.getpack(datastore, historystore, fileids)

            self.ui.log(
                "remotefilefetchlog",
                "Success(pack)\n" if (rcvd == total) else "Fail(pack)\n",
                fetched_files=rcvd,
                total_to_fetch=total,
            )
        except Exception:
            self.ui.log(
                "remotefilefetchlog",
                "Fail(pack)\n",
                fetched_files=total - len(fileids),
                total_to_fetch=total,
            )
            raise


class fileserverclient(object):
    """A client for requesting files from the remote file server."""

    def __init__(self, repo):
        ui = repo.ui
        self.repo = repo
        self.ui = ui

    @perftrace.tracefunc("Prefetch Files")
    def prefetch(self, fileids, force=False, fetchdata=True, fetchhistory=True):
        """downloads the given file versions to the cache"""
        repo = self.repo
        idstocheck = set()
        for file, id in fileids:
            # hack
            # - we don't use .hgtags
            # - workingctx produces ids with length 42,
            #   which we skip since they aren't in any cache
            if file == ".hgtags" or len(id) == 42 or not repo.shallowmatch(file):
                continue

            idstocheck.add((file, bin(id)))

        batchlfsdownloads = self.ui.configbool(
            "remotefilelog", "_batchlfsdownloads", True
        )
        dolfsprefetch = self.ui.configbool("remotefilelog", "dolfsprefetch", True)

        idstocheck = list(idstocheck)
        if not force:
            contentstore = repo.fileslog.contentstore
            metadatastore = repo.fileslog.metadatastore
        else:
            # TODO(meyer): Convert this to support scmstore.
            contentstore, metadatastore = repo.fileslog.makesharedonlyruststore(repo)

        if fetchdata:
            contentstore.prefetch(idstocheck)
        if fetchhistory:
            metadatastore.prefetch(idstocheck)

        if batchlfsdownloads and dolfsprefetch:
            self._lfsprefetch(fileids)

        if force:
            # Yay, since the shared-only stores and the regular ones aren't
            # shared, we need to commit data to force the stores to be
            # rebuilt. Forced prefetch are very rare and thus it is most
            # likely OK to do this.
            contentstore = None
            metadatastore = None
            repo.commitpending()

    @perftrace.tracefunc("LFS Prefetch")
    def _lfsprefetch(self, fileids):
        if not _lfsmod or not util.safehasattr(self.repo.svfs, "lfslocalblobstore"):
            return
        if not _lfsmod.wrapper.candownload(self.repo):
            return
        pointers = []
        filenames = {}
        store = self.repo.svfs.lfslocalblobstore
        for file, id in fileids:
            node = bin(id)
            rlog = self.repo.file(file)
            if rlog.flags(node) & revlog.REVIDX_EXTSTORED:
                text = rlog.revision(node, raw=True)
                p = _lfsmod.pointer.deserialize(text)
                oid = p.oid()
                if not store.has(oid):
                    pointers.append(p)
                    filenames[oid] = file
        if len(pointers) > 0:
            perftrace.tracevalue("Missing", len(pointers))
            self.repo.svfs.lfsremoteblobstore.readbatch(
                pointers, store, objectnames=filenames
            )
            assert all(store.has(p.oid()) for p in pointers)

    def logstacktrace(self):
        self.ui.log(
            "remotefilelog",
            "excess remotefilelog fetching:\n%s\n",
            "".join(traceback.format_stack()),
        )
