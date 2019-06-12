# fileserverclient.py - client for communicating with the cache process
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import functools
import hashlib
import io
import itertools
import os
import struct
import subprocess
import threading
import time
import traceback

from edenscm.mercurial import (
    encoding,
    error,
    httppeer,
    perftrace,
    progress,
    revlog,
    sshpeer,
    util,
    wireproto,
)
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import bin, hex, nullid

from . import constants, edenapi, shallowutil, wirepack
from .contentstore import unioncontentstore
from .lz4wrapper import lz4decompress
from .metadatastore import unionmetadatastore
from .repack import domaintenancerepack


# Statistics for debugging
fetchcost = 0
fetches = 0
fetched = 0
fetchmisses = 0

_lfsmod = None


def getcachekey(reponame, file, id):
    pathhash = hashlib.sha1(file).hexdigest()
    return os.path.join(reponame, pathhash[:2], pathhash[2:], id)


def getlocalkey(file, id):
    pathhash = hashlib.sha1(file).hexdigest()
    return os.path.join(pathhash, id)


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
            heads = f.value.split("\n") if f.value else []
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


class CacheConnectionError(Exception):
    """Exception raised if the cache connection was unexpectedly closed."""

    def __init__(self):
        super(CacheConnectionError, self).__init__(
            "Scmmemcache connection was unexpectedly closed"
        )


class cacheconnection(object):
    """The connection for communicating with the remote cache. Performs
    gets and sets by communicating with an external process that has the
    cache-specific implementation.
    """

    def __init__(self, repo):
        self.pipeo = self.pipei = None
        self.subprocess = None
        self.connected = False
        self.repo = repo

    def connect(self, cachecommand):
        if self.pipeo:
            raise error.Abort(_("cache connection already open"))

        # Use subprocess.Popen() directly rather than the wrappers in
        # util in order to pipe stderr to /dev/null, thereby preventing
        # hangs in cases where the cache process fills the stderr pipe
        # buffer (since remotefilelog never reads from stderr).
        self.subprocess = subprocess.Popen(
            cachecommand,
            shell=True,
            close_fds=util.closefds,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=open(os.devnull, "wb"),
        )

        self.pipei = self.subprocess.stdin
        self.pipeo = self.subprocess.stdout
        self.bufferedpipeo = io.open(self.pipeo.fileno(), mode="rb", closefd=False)
        self.connected = True

    def close(self):
        def tryclose(pipe):
            try:
                pipe.close()
            except Exception:
                pass

        if self.connected:
            try:
                self.pipei.write("exit\n")
            except Exception:
                pass
            tryclose(self.pipei)
            self.pipei = None
            self.bufferedpipeo = None
            tryclose(self.pipeo)
            self.pipeo = None

            # When hg makes many fetch request, memcache will store each
            # request into its own packfile. This can happen when using "hg log
            # -p", and this causes the number of packfiles to grow very
            # quickly, killing the performance of other commands. To avoid this
            # pathological case, we can run a quick repack on these files.
            if self.repo.ui.configbool("remotefilelog", "fetchpacks"):
                domaintenancerepack(self.repo)

            # The cacheclient may be executing expensive set commands in the
            # background, As soon as the exit command, or the pipe above is
            # closed, the client will get notified that hg has terminated and
            # will voluntarily exit soon. Hence, there is no need to wait for
            # it to terminate.
            self.subprocess = None
        self.connected = False

    def _request(self, request, flush=True):
        if self.connected:
            try:
                self.pipei.write(request)
                if flush:
                    self.pipei.flush()
            except IOError:
                self.close()

    def _makerequest(self, command, keys):
        request = "%s\n%d\n%s\n" % (command, len(keys), "\n".join(keys))
        self._request(request)

    def getdatapack(self, keys):
        self._makerequest("getdata", keys)

    def gethistorypack(self, keys):
        self._makerequest("gethistory", keys)

    def get(self, keys):
        self._makerequest("get", keys)

    def set(self, keys):
        self._makerequest("set", keys)

    def setdatapack(self, keys):
        self._makerequest("setdata", keys)

    def sethistorypack(self, keys):
        self._makerequest("sethistory", keys)

    def receive(self, prog=None):
        """Reads the cacheprocess' reply for the request sent and tracks
        the progress. Returns a set of missed keys
        """
        missed = set()
        while True:
            key = self._receiveline()
            if not key:
                raise CacheConnectionError()
            if key == "0":
                # the end of the stream
                break

            if key.startswith("_hits_"):
                # hit -> receive progress reports
                parts = key.split("_")
                if prog is not None:
                    prog.value += int(parts[2])
            else:
                missed.add(key)
        return missed

    def _receiveline(self):
        if not self.connected:
            return None
        try:
            result = self.bufferedpipeo.readline()[:-1]
            if not result:
                self.close()
        except IOError:
            self.close()

        return result


def _getfilesbatch(remote, receivemissing, progresstick, missed, idmap, batchsize):
    # Over http(s), iterbatch is a streamy method and we can start
    # looking at results early. This means we send one (potentially
    # large) request, but then we show nice progress as we process
    # file results, rather than showing chunks of $batchsize in
    # progress.
    #
    # Over ssh, iterbatch isn't streamy because batch() wasn't
    # explicitly designed as a streaming method. In the future we
    # should probably introduce a streambatch() method upstream and
    # use that for this.
    if (
        getattr(remote, "iterbatch", False)
        and remote.capable("httppostargs")
        and isinstance(remote, httppeer.httppeer)
    ):
        b = remote.iterbatch()
        for m in missed:
            file_ = idmap[m]
            node = m[-40:]
            b.getfile(file_, node)
        b.submit()
        for m, r in itertools.izip(missed, b.results()):
            file_ = idmap[m]
            node = m[-40:]
            progresstick(file_)
            receivemissing(io.BytesIO("%d\n%s" % (len(r), r)), file_, node, m)
        return
    while missed:
        chunk, missed = missed[:batchsize], missed[batchsize:]
        b = remote.iterbatch()
        for m in chunk:
            file_ = idmap[m]
            node = m[-40:]
            b.getfile(file_, node)
        b.submit()
        for m, v in zip(chunk, b.results()):
            file_ = idmap[m]
            node = m[-40:]
            progresstick(file_)
            receivemissing(io.BytesIO("%d\n%s" % (len(v), v)), file_, node, m)


def _getfiles_optimistic(remote, receivemissing, progresstick, missed, idmap, step):
    remote._callstream("getfiles")
    i = 0
    pipeo = shallowutil.trygetattr(remote, ("_pipeo", "pipeo"))
    pipei = shallowutil.trygetattr(remote, ("_pipei", "pipei"))
    while i < len(missed):
        # issue a batch of requests
        start = i
        end = min(len(missed), start + step)
        i = end
        for missingid in missed[start:end]:
            # issue new request
            versionid = missingid[-40:]
            file = idmap[missingid]
            sshrequest = "%s%s\n" % (versionid, file)
            pipeo.write(sshrequest)
        pipeo.flush()

        # receive batch results
        for missingid in missed[start:end]:
            versionid = missingid[-40:]
            file = idmap[missingid]
            progresstick(file)
            receivemissing(pipei, file, versionid, missingid)

    # End the command
    pipeo.write("\n")
    pipeo.flush()


def _getfiles_threaded(remote, receivemissing, progresstick, missed, idmap, step):
    remote._callstream("getfiles")
    pipeo = shallowutil.trygetattr(remote, ("_pipeo", "pipeo"))
    pipei = shallowutil.trygetattr(remote, ("_pipei", "pipei"))

    def writer():
        for missingid in missed:
            versionid = missingid[-40:]
            file = idmap[missingid]
            sshrequest = "%s%s\n" % (versionid, file)
            pipeo.write(sshrequest)
        pipeo.flush()

    writerthread = threading.Thread(target=writer)
    writerthread.daemon = True
    writerthread.start()

    for missingid in missed:
        versionid = missingid[-40:]
        file = idmap[missingid]
        progresstick(file)
        receivemissing(pipei, file, versionid, missingid)

    writerthread.join()
    # End the command
    pipeo.write("\n")
    pipeo.flush()


class fileserverclient(object):
    """A client for requesting files from the remote file server.
    """

    def __init__(self, repo):
        ui = repo.ui
        self.repo = repo
        self.ui = ui
        self.cacheprocess = ui.config("remotefilelog", "cacheprocess")
        if self.cacheprocess:
            self.cacheprocess = util.expandpath(self.cacheprocess)

        # This option causes remotefilelog to pass the full file path to the
        # cacheprocess instead of a hashed key.
        self.cacheprocesspasspath = ui.configbool(
            "remotefilelog", "cacheprocess.includepath"
        )

        self.debugoutput = ui.configbool("remotefilelog", "debug")

        self.remotecache = cacheconnection(repo)

    def setstore(self, datastore, historystore, writedata, writehistory):
        self.datastore = datastore
        self.historystore = historystore
        self.writedata = writedata
        self.writehistory = writehistory

    def _connect(self):
        return self.repo.connectionpool.get(self.repo.fallbackpath)

    def request(self, fileids, fetchdata, fetchhistory):
        if self.ui.configbool("remotefilelog", "fetchpacks"):
            return self.requestpacks(fileids, fetchdata, fetchhistory)
        return self.requestloose(fileids)

    def updatecache(self, dpackpath, hpackpath):
        if self.remotecache.connected:
            # send to the memcache
            if self.ui.configbool("remotefilelog", "updatesharedcache"):
                if dpackpath:
                    self.remotecache.setdatapack([dpackpath])
                if hpackpath:
                    self.remotecache.sethistorypack([hpackpath])

    def requestpacks(self, fileids, fetchdata, fetchhistory):
        if not self.remotecache.connected:
            self.connect()
        perftrace.traceflag("packs")
        cache = self.remotecache
        fileslog = self.repo.fileslog

        total = len(fileids)
        totalfetches = 0
        if fetchdata:
            totalfetches += total
        if fetchhistory:
            totalfetches += total
        with progress.bar(
            self.ui, _("fetching from memcache"), total=totalfetches
        ) as prog:
            # generate `get` keys and make data request
            getkeys = [file + "\0" + node for file, node in fileids]
            if fetchdata:
                cache.getdatapack(getkeys)
            if fetchhistory:
                cache.gethistorypack(getkeys)

            # receive both data and history
            misses = []
            try:
                allmisses = set()
                if fetchdata:
                    allmisses.update(cache.receive(prog))
                    fileslog.contentstore.markforrefresh()
                if fetchhistory:
                    allmisses.update(cache.receive(prog))
                    fileslog.metadatastore.markforrefresh()

                misses = map(lambda key: key.split("\0"), allmisses)
                perftrace.tracevalue("Memcache Misses", len(misses))
            except CacheConnectionError:
                misses = fileids
                self.ui.warn(
                    _(
                        "warning: cache connection closed early - "
                        + "falling back to server\n"
                    )
                )

            global fetchmisses
            missedfiles = len(misses)
            fetchmisses += missedfiles

            fromcache = total - missedfiles
            self.ui.log(
                "remotefilelog",
                "remote cache hit rate is %r of %r\n",
                fromcache,
                total,
                hit=fromcache,
                total=total,
            )

        oldumask = os.umask(0o002)
        try:
            # receive cache misses from master
            if missedfiles > 0:
                self._fetchpackfiles(misses, fetchdata, fetchhistory)
        finally:
            os.umask(oldumask)

    def requestloose(self, fileids):
        """Takes a list of filename/node pairs and fetches them from the
        server. Files are stored in the local cache.
        A list of nodes that the server couldn't find is returned.
        If the connection fails, an exception is raised.
        """
        if not self.remotecache.connected:
            self.connect()
        perftrace.traceflag("loose")

        cache = self.remotecache
        writedata = self.writedata

        repo = self.repo
        idmap = {}
        reponame = repo.name
        for file, id in fileids:
            key = getcachekey(reponame, file, id)
            idmap[key] = file

        # requesting scmmemcache
        filefullids = idmap.keys()
        if self.cacheprocesspasspath:
            filefullids = [
                file + "\0" + filefullid for filefullid, file in idmap.iteritems()
            ]
        cache.get(filefullids)

        getfilenamepath = lambda name: None
        if util.safehasattr(self.writedata, "_getfilenamepath"):
            getfilenamepath = self.writedata._getfilenamepath

        total = len(fileids)
        with progress.bar(self.ui, _("downloading"), total=total) as prog:
            missed = set()
            try:
                missed = cache.receive(prog)
            except CacheConnectionError:
                for missingid in idmap.iterkeys():
                    missed.add(missingid)
                self.ui.warn(
                    _(
                        "warning: cache connection closed early - "
                        + "falling back to server\n"
                    )
                )

            # If the cacheprocess knew the filename, it should store it
            # somewhere useful (e.g. in a pack file it generates).  Otherwise,
            # we must write the filename out for it.
            if not self.cacheprocesspasspath:
                for fullid, file in idmap.iteritems():
                    if fullid not in missed:
                        filenamepath = getfilenamepath(fullid)
                        if (
                            filenamepath is not None
                            and os.path.isdir(os.path.dirname(filenamepath))
                            and not os.path.exists(filenamepath)
                        ):
                            shallowutil.writefile(filenamepath, file, readonly=True)

            global fetchmisses
            missedlen = len(missed)
            perftrace.tracevalue("Memcache Misses", len(missed))
            fetchmisses += missedlen

            fromcache = total - missedlen
            prog.value = fromcache
            self.ui.log(
                "remotefilelog",
                "remote cache hit rate is %r of %r\n",
                fromcache,
                total,
                hit=fromcache,
                total=total,
            )

            oldumask = os.umask(0o002)
            try:
                # receive cache misses from master
                if missed:

                    fetchedfiles = self._fetchloosefiles(idmap, missed, prog=prog)
                    # send to memcache
                    if self.ui.configbool("remotefilelog", "updatesharedcache"):
                        if fetchedfiles:
                            cache.set(fetchedfiles)

                # mark ourselves as a user of this cache
                writedata.markrepo(self.repo.path)
            finally:
                os.umask(oldumask)

    def _fetchloosefiles(self, idmap, missed, prog=None):
        """Fetch missed filesnodes from the server in the loose files format.
        Returns a set of the fetched file fullids.
        """
        # let's track the progress
        total = len(idmap)
        count = [total - len(missed)]
        fromcache = count[0]

        def progresstick(name=""):
            count[0] += 1
            if prog is not None:
                prog.value = (count[0], name)

        # When verbose is true, sshpeer prints 'running ssh...'
        # to stdout, which can interfere with some command
        # outputs
        verbose = self.ui.verbose
        self.ui.verbose = False
        draft = set()
        try:
            with self._connect() as conn:
                remote = conn.peer
                missedlist = list(missed)

                self.ui.metrics.gauge("ssh_getfiles_revs", len(missedlist))
                self.ui.metrics.gauge("ssh_getfiles_calls", 1)

                # TODO: deduplicate this with the constant in
                #       shallowrepo
                if remote.capable("remotefilelog"):
                    if not isinstance(remote, sshpeer.sshpeer):
                        msg = "remotefilelog requires ssh servers"
                        raise error.Abort(msg)
                    step = self.ui.configint("remotefilelog", "getfilesstep", 10000)
                    getfilestype = self.ui.config(
                        "remotefilelog", "getfilestype", "optimistic"
                    )
                    if getfilestype == "threaded":
                        _getfiles = _getfiles_threaded
                    else:
                        _getfiles = _getfiles_optimistic
                    _getfiles(
                        remote,
                        functools.partial(self.receivemissing, draft),
                        progresstick,
                        missedlist,
                        idmap,
                        step,
                    )
                elif remote.capable("getfile"):
                    if remote.capable("batch"):
                        batchdefault = 100
                    else:
                        batchdefault = 10
                    batchsize = self.ui.configint(
                        "remotefilelog", "batchsize", batchdefault
                    )
                    _getfilesbatch(
                        remote,
                        functools.partial(self.receivemissing, draft),
                        progresstick,
                        missedlist,
                        idmap,
                        batchsize,
                    )
                else:
                    msg = (
                        "configured remotefilelog server"
                        " does not support remotefilelog"
                    )
                    raise error.Abort(msg)

            self.ui.log(
                "remotefilefetchlog",
                "Success\n",
                fetched_files=count[0] - fromcache,
                total_to_fetch=total - fromcache,
            )
        except Exception:
            self.ui.log(
                "remotefilefetchlog",
                "Fail\n",
                fetched_files=count[0] - fromcache,
                total_to_fetch=total - fromcache,
            )
            raise
        finally:
            self.ui.verbose = verbose

        return missed - draft

    def receivemissing(self, draftset, pipe, filename, node, key):
        line = pipe.readline()[:-1]
        if not line:
            raise error.ResponseError(
                _("error downloading file contents:"),
                _("connection closed early for filename %s and node %s")
                % (filename, node),
            )
        size = int(line)
        data = pipe.read(size)
        if len(data) != size:
            raise error.ResponseError(
                _("error downloading file contents:"),
                _("only received %s of %s bytes") % (len(data), size),
            )
        data = lz4decompress(data)
        mapping = shallowutil.ancestormap(data)
        if any(
            linknode == nullid for _p1, _p2, linknode, _copyfrom in mapping.values()
        ):
            draftset.add(key)
        self.writedata.addremotefilelognode(filename, bin(node), data)

    def _fetchpackfiles(self, fileids, fetchdata, fetchhistory):
        """Requests the given file revisions from the server in a pack files
        format.

        See `remotefilelogserver.getpack` for the file format.
        """

        # Try fetching packs via HTTP first; fall back to SSH on error.
        if edenapi.enabled(self.ui):
            try:
                self._httpfetchpacks(fileids, fetchdata, fetchhistory)
            except Exception as e:
                self.ui.warn(_("encountered error during HTTPS fetching;"))
                self.ui.warn(_(" falling back to SSH\n"))
                if edenapi.debug(self.ui):
                    self.ui.warn(_("exception: %s\n") % str(e))
                self.ui.log(
                    "edenapi_error", msg=str(e), traceback=traceback.format_exc()
                )
                self.ui.metrics.gauge("edenapi_fallbacks", 1)

        rcvd = 0
        total = len(fileids)

        try:
            with self._connect() as conn:
                self.ui.metrics.gauge("ssh_getpack_revs", len(fileids))
                self.ui.metrics.gauge("ssh_getpack_calls", 1)

                remote = conn.peer
                remote._callstream("getpackv1")

                self._sendpackrequest(remote, fileids)

                pipei = shallowutil.trygetattr(remote, ("_pipei", "pipei"))

                dpack, hpack = self.repo.fileslog.getmutablesharedpacks()
                receiveddata, receivedhistory = wirepack.receivepack(
                    self.repo.ui, pipei, dpack, hpack
                )

                rcvd = len(receiveddata)

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
                fetched_files=rcvd,
                total_to_fetch=total,
            )
            raise

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
        for filename, nodes in grouped.iteritems():
            filenamelen = struct.pack(constants.FILENAMESTRUCT, len(filename))
            countlen = struct.pack(constants.PACKREQUESTCOUNTSTRUCT, len(nodes))
            rawnodes = "".join(bin(n) for n in nodes)

            pipeo.write("%s%s%s%s" % (filenamelen, filename, countlen, rawnodes))
            pipeo.flush()
        pipeo.write(struct.pack(constants.FILENAMESTRUCT, 0))
        pipeo.flush()

    def _httpfetchpacks(self, fileids, fetchdata, fetchhistory):
        """Fetch packs via HTTPS using the Eden API"""
        perftrace.traceflag("http")

        # The Eden API Rust bindings require that fileids
        # be a list of tuples; lists-of-lists or generators
        # will result in a type error, so convert them here.
        fileids = [tuple(i) for i in fileids]

        dpack, hpack = self.repo.fileslog.getmutablesharedpacks()
        if fetchdata:
            self._httpfetchdata(fileids, dpack)
        if fetchhistory:
            self._httpfetchhistory(fileids, hpack)

    def _httpfetchdata(self, fileids, dpack):
        """Fetch file data over HTTPS using the Eden API"""
        n = len(fileids)
        s = "" if n == 1 else "s"
        msg = _("fetching content for %d file%s over HTTPS") % (n, s)

        if edenapi.debug(self.ui):
            self.ui.warn(_("%s\n") % msg)

        self.ui.metrics.gauge("http_getfiles_revs", n)
        self.ui.metrics.gauge("http_getfiles_calls", 1)

        with progress.bar(
            self.ui, msg, start=None, unit=_("bytes"), formatfunc=util.bytecount
        ) as prog:

            def progcallback(dl, dlt, ul, ult):
                if dl > 0:
                    prog._total = dlt
                    prog.value = dl

            stats = self.repo.edenapi.get_files(fileids, dpack, progcallback)

        if edenapi.debug(self.ui):
            self.ui.warn(_("%s\n") % stats.to_str())

        self.ui.metrics.gauge("http_getfiles_time_ms", stats.time_in_millis())
        self.ui.metrics.gauge("http_getfiles_latency_ms", stats.latency_in_millis())
        self.ui.metrics.gauge("http_getfiles_bytes_downloaded", stats.downloaded())
        self.ui.metrics.gauge("http_getfiles_bytes_uploaded", stats.uploaded())
        self.ui.metrics.gauge("http_getfiles_requests", stats.requests())

    def _httpfetchhistory(self, fileids, hpack, depth=None):
        """Fetch file history over HTTPS using the Eden API"""
        n = len(fileids)
        s = "" if n == 1 else "s"
        msg = _("fetching history for %d file%s over HTTPS") % (n, s)

        if edenapi.debug(self.ui):
            self.ui.warn(_("%s\n") % msg)

        self.ui.metrics.gauge("http_gethistory_revs", n)
        self.ui.metrics.gauge("http_gethistory_calls", 1)

        with progress.bar(
            self.ui, msg, start=None, unit=_("bytes"), formatfunc=util.bytecount
        ) as prog:

            def progcallback(dl, dlt, ul, ult):
                if dl > 0:
                    prog._total = dlt
                    prog.value = dl

            stats = self.repo.edenapi.get_history(fileids, hpack, depth, progcallback)

        if edenapi.debug(self.ui):
            self.ui.warn(_("%s\n") % stats.to_str())

        self.ui.metrics.gauge("http_gethistory_time_ms", stats.time_in_millis())
        self.ui.metrics.gauge("http_gethistory_latency_ms", stats.latency_in_millis())
        self.ui.metrics.gauge("http_gethistory_bytes_downloaded", stats.downloaded())
        self.ui.metrics.gauge("http_gethistory_bytes_uploaded", stats.uploaded())
        self.ui.metrics.gauge("http_gethistory_requests", stats.requests())

    def connect(self):
        if self.cacheprocess:
            options = ""
            if self.ui.configbool("remotefilelog", "fetchpacks"):
                cachepath = shallowutil.getcachepackpath(
                    self.repo, constants.FILEPACK_CATEGORY
                )

                if self.ui.configbool("remotefilelog", "indexedlogdatastore"):
                    path = shallowutil.getexperimentalcachepath(self.repo)
                    path = os.path.join(path, "indexedlogdatastore")
                    if self.ui.configbool(
                        "remotefilelog", "mirrorindexedlogwrites", True
                    ):
                        options += "--mirrored "
                    options += "--indexedlog_dir %s" % path
            else:
                cachepath = shallowutil.getcachepath(self.ui)

            cmd = " ".join([self.cacheprocess, cachepath, options])
            self.remotecache.connect(cmd)
        else:
            # If no cache process is specified, we fake one that always
            # returns cache misses.  This enables tests to run easily
            # and may eventually allow us to be a drop in replacement
            # for the largefiles extension.
            class simplecache(object):
                def __init__(self):
                    self.missingids = []
                    self.connected = True

                def close(self):
                    pass

                def set(self, keys):
                    pass

                def setdatapack(self, keys):
                    pass

                def sethistorypack(self, keys):
                    pass

                def get(self, keys):
                    self.missingids.append(keys)

                def getdatapack(self, keys):
                    self.missingids.append(keys)

                def gethistorypack(self, keys):
                    self.missingids.append(keys)

                def receive(self, prog=None):
                    missing = self.missingids.pop(0) if self.missingids else []
                    return set(missing)

            self.remotecache = simplecache()

    def close(self):
        # Make it "run-tests.py -i" friendly
        if "TESTTMP" in encoding.environ:
            global fetchcost
            fetchcost = 0
        if fetches:
            msg = (
                "%s files fetched over %d fetches - "
                + "(%d misses, %0.2f%% hit ratio) over %0.2fs\n"
            ) % (
                fetched,
                fetches,
                fetchmisses,
                float(fetched - fetchmisses) / float(fetched) * 100.0,
                fetchcost,
            )
            if self.debugoutput:
                self.ui.warn(msg)
            self.ui.log(
                "remotefilelog.prefetch",
                msg.replace("%", "%%"),
                remotefilelogfetched=fetched,
                remotefilelogfetches=fetches,
                remotefilelogfetchmisses=fetchmisses,
                remotefilelogfetchtime=fetchcost * 1000,
            )

        if self.remotecache.connected:
            self.remotecache.close()

    @perftrace.tracefunc("Prefetch Files")
    def prefetch(self, fileids, force=False, fetchdata=True, fetchhistory=True):
        """downloads the given file versions to the cache
        """
        repo = self.repo
        idstocheck = []
        for file, id in fileids:
            # hack
            # - we don't use .hgtags
            # - workingctx produces ids with length 42,
            #   which we skip since they aren't in any cache
            if file == ".hgtags" or len(id) == 42 or not repo.shallowmatch(file):
                continue

            idstocheck.append((file, bin(id)))

        datastore = self.datastore
        historystore = self.historystore
        if force:
            datastore = unioncontentstore(*repo.fileslog.shareddatastores)
            historystore = unionmetadatastore(*repo.fileslog.sharedhistorystores)

        perftrace.tracevalue("Keys", len(idstocheck))
        missingids = set()
        if fetchdata:
            missingids.update(datastore.getmissing(idstocheck))
            perftrace.tracevalue("Missing Data", len(missingids))
        if fetchhistory:
            missinghistory = historystore.getmissing(idstocheck)
            missingids.update(missinghistory)
            perftrace.tracevalue("Missing History", len(missinghistory))

        # partition missing nodes into nullid and not-nullid so we can
        # warn about this filtering potentially shadowing bugs.
        nullids = len([None for unused, id in missingids if id == nullid])
        if nullids:
            missingids = [(f, id) for f, id in missingids if id != nullid]
            repo.ui.develwarn(
                (
                    "remotefilelog not fetching %d null revs"
                    " - this is likely hiding bugs" % nullids
                ),
                config="remotefilelog-ext",
            )
        batchlfsdownloads = self.ui.configbool(
            "remotefilelog", "_batchlfsdownloads", True
        )
        dolfsprefetch = self.ui.configbool("remotefilelog", "dolfsprefetch", True)
        if missingids:
            global fetches, fetched, fetchcost
            fetches += 1

            missingids = [(file, hex(id)) for file, id in missingids]

            fetched += len(missingids)

            start = time.time()
            with self.ui.timesection("fetchingfiles"):
                self.request(missingids, fetchdata, fetchhistory)
            fetchcost += time.time() - start
            if not batchlfsdownloads and dolfsprefetch:
                self._lfsprefetch(fileids)
        if batchlfsdownloads and dolfsprefetch:
            self._lfsprefetch(fileids)

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
        import traceback

        self.ui.log(
            "remotefilelog",
            "excess remotefilelog fetching:\n%s\n",
            "".join(traceback.format_stack()),
        )
