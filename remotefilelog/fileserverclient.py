# fileserverclient.py - client for communicating with the cache process
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.i18n import _
from mercurial.node import hex, bin
from mercurial import util, sshpeer, hg, error, util, wireproto, node, httppeer
from mercurial import scmutil
import os, socket, lz4, time, grp, io, struct
import errno
import itertools

import constants, datapack, historypack, shallowutil
from shallowutil import readexactly, readunpack

# Statistics for debugging
fetchcost = 0
fetches = 0
fetched = 0
fetchmisses = 0

_downloading = _('downloading')

def makedirs(root, path, owner):
    try:
        os.makedirs(path)
    except OSError as ex:
        if ex.errno != errno.EEXIST:
            raise

    while path != root:
        stat = os.stat(path)
        if stat.st_uid == owner:
            os.chmod(path, 0o2775)
        path = os.path.dirname(path)

def getcachekey(reponame, file, id):
    pathhash = util.sha1(file).hexdigest()
    return os.path.join(reponame, pathhash[:2], pathhash[2:], id)

def getlocalkey(file, id):
    pathhash = util.sha1(file).hexdigest()
    return os.path.join(pathhash, id)

def peersetup(ui, peer):
    class remotefilepeer(peer.__class__):
        @wireproto.batchable
        def getfile(self, file, node):
            if not self.capable('getfile'):
                raise error.Abort(
                    'configured remotefile server does not support getfile')
            f = wireproto.future()
            yield {'file': file, 'node': node}, f
            code, data = f.value.split('\0', 1)
            if int(code):
                raise error.LookupError(file, node, data)
            yield data
    peer.__class__ = remotefilepeer

class cacheconnection(object):
    """The connection for communicating with the remote cache. Performs
    gets and sets by communicating with an external process that has the
    cache-specific implementation.
    """
    def __init__(self):
        self.pipeo = self.pipei = self.pipee = None
        self.subprocess = None
        self.connected = False

    def connect(self, cachecommand):
        if self.pipeo:
            raise error.Abort(_("cache connection already open"))
        self.pipei, self.pipeo, self.pipee, self.subprocess = \
            util.popen4(cachecommand)
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
            tryclose(self.pipeo)
            self.pipeo = None
            tryclose(self.pipee)
            self.pipee = None
            try:
                # Wait for process to terminate, making sure to avoid deadlock.
                # See https://docs.python.org/2/library/subprocess.html for
                # warnings about wait() and deadlocking.
                self.subprocess.communicate()
            except Exception:
                pass
            self.subprocess = None
        self.connected = False

    def request(self, request, flush=True):
        if self.connected:
            try:
                self.pipei.write(request)
                if flush:
                    self.pipei.flush()
            except IOError:
                self.close()

    def receiveline(self):
        if not self.connected:
            return None
        try:
            result = self.pipeo.readline()[:-1]
            if not result:
                self.close()
        except IOError:
            self.close()

        return result

def _getfilesbatch(
        remote, receivemissing, progresstick, missed, idmap, batchsize):
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
    if (getattr(remote, 'iterbatch', False) and remote.capable('httppostargs')
        and isinstance(remote, httppeer.httppeer)):
        b = remote.iterbatch()
        for m in missed:
            file_ = idmap[m]
            node = m[-40:]
            b.getfile(file_, node)
        b.submit()
        for m, r in itertools.izip(missed, b.results()):
            receivemissing(io.BytesIO('%d\n%s' % (len(r), r)), m)
            progresstick()
        return
    while missed:
        chunk, missed = missed[:batchsize], missed[batchsize:]
        b = remote.batch()
        futures = {}
        for m in chunk:
            file_ = idmap[m]
            node = m[-40:]
            futures[m] = b.getfile(file_, node)
        b.submit()
        for m in chunk:
            v = futures[m].value
            file_ = idmap[m]
            node = m[-40:]
            receivemissing(io.BytesIO('%d\n%s' % (len(v), v)), file_, node)
            progresstick()

def _getfiles(
    remote, receivemissing, progresstick, missed, idmap):
    i = 0
    while i < len(missed):
        # issue a batch of requests
        start = i
        end = min(len(missed), start + 10000)
        i = end
        for missingid in missed[start:end]:
            # issue new request
            versionid = missingid[-40:]
            file = idmap[missingid]
            sshrequest = "%s%s\n" % (versionid, file)
            remote.pipeo.write(sshrequest)
        remote.pipeo.flush()

        # receive batch results
        for missingid in missed[start:end]:
            versionid = missingid[-40:]
            file = idmap[missingid]
            receivemissing(remote.pipei, file, versionid)
            progresstick()

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
            "remotefilelog", "cacheprocess.includepath")

        self.debugoutput = ui.configbool("remotefilelog", "debug")

        self.contentstore = None
        self.writestore = None

        self.remotecache = cacheconnection()
        self.remoteserver = None

    def setstore(self, store, writestore):
        self.contentstore = store
        self.writestore = writestore

    def _connect(self):
        fallbackpath = self.repo.fallbackpath
        if not self.remoteserver:
            if not fallbackpath:
                raise error.Abort("no remotefilelog server "
                    "configured - is your .hg/hgrc trusted?")
            self.remoteserver = hg.peer(self.ui, {}, fallbackpath)
        elif (isinstance(self.remoteserver, sshpeer.sshpeer) and
                 self.remoteserver.subprocess.poll() is not None):
            # The ssh connection died, so recreate it.
            self.remoteserver = hg.peer(self.ui, {}, fallbackpath)

        return self.remoteserver

    def request(self, fileids):
        """Takes a list of filename/node pairs and fetches them from the
        server. Files are stored in the local cache.
        A list of nodes that the server couldn't find is returned.
        If the connection fails, an exception is raised.
        """
        if not self.remotecache.connected:
            self.connect()
        cache = self.remotecache
        writestore = self.writestore

        if self.ui.configbool('remotefilelog', 'fetchpacks'):
            self.requestpack(fileids)
            return

        repo = self.repo
        count = len(fileids)
        request = "get\n%d\n" % count
        idmap = {}
        reponame = repo.name
        for file, id in fileids:
            fullid = getcachekey(reponame, file, id)
            if self.cacheprocesspasspath:
                request += file + '\0'
            request += fullid + "\n"
            idmap[fullid] = file

        cache.request(request)

        missing = []
        total = count
        self.ui.progress(_downloading, 0, total=count)

        missed = []
        count = 0
        while True:
            missingid = cache.receiveline()
            if not missingid:
                missedset = set(missed)
                for missingid in idmap.iterkeys():
                    if not missingid in missedset:
                        missed.append(missingid)
                self.ui.warn(_("warning: cache connection closed early - " +
                    "falling back to server\n"))
                break
            if missingid == "0":
                break
            if missingid.startswith("_hits_"):
                # receive progress reports
                parts = missingid.split("_")
                count += int(parts[2])
                self.ui.progress(_downloading, count, total=total)
                continue

            missed.append(missingid)

        global fetchmisses
        fetchmisses += len(missed)

        count = [total - len(missed)]
        self.ui.progress(_downloading, count[0], total=total)
        self.ui.log("remotefilelog", "remote cache hit rate is %r of %r ",
                    count[0], total, hit=count[0], total=total)

        oldumask = os.umask(0o002)
        try:
            # receive cache misses from master
            if missed:
                def progresstick():
                    count[0] += 1
                    self.ui.progress(_downloading, count[0], total=total)
                # When verbose is true, sshpeer prints 'running ssh...'
                # to stdout, which can interfere with some command
                # outputs
                verbose = self.ui.verbose
                self.ui.verbose = False
                try:
                    oldremote = self.remoteserver
                    remote = self._connect()

                    # TODO: deduplicate this with the constant in shallowrepo
                    if remote.capable("remotefilelog"):
                        if not isinstance(remote, sshpeer.sshpeer):
                            raise error.Abort('remotefilelog requires ssh '
                                              'servers')
                        # If it's a new connection, issue the getfiles command
                        if oldremote != remote:
                            remote._callstream("getfiles")
                        _getfiles(remote, self.receivemissing, progresstick,
                                  missed, idmap)
                    elif remote.capable("getfile"):
                        batchdefault = 100 if remote.capable('batch') else 10
                        batchsize = self.ui.configint(
                            'remotefilelog', 'batchsize', batchdefault)
                        _getfilesbatch(
                            remote, self.receivemissing, progresstick, missed,
                            idmap, batchsize)
                    else:
                        raise error.Abort("configured remotefilelog server"
                                         " does not support remotefilelog")
                finally:
                    self.ui.verbose = verbose
                # send to memcache
                count[0] = len(missed)
                request = "set\n%d\n%s\n" % (count[0], "\n".join(missed))
                cache.request(request)

            self.ui.progress(_downloading, None)

            # mark ourselves as a user of this cache
            writestore.markrepo(self.repo.path)
        finally:
            os.umask(oldumask)

        return missing

    def receivemissing(self, pipe, filename, node):
        line = pipe.readline()[:-1]
        if not line:
            raise error.ResponseError(_("error downloading file " +
                "contents: connection closed early\n"), '')
        size = int(line)
        data = pipe.read(size)
        if len(data) != size:
            raise error.ResponseError(_("error downloading file contents: "
                                        "only received %s of %s bytes") %
                                      (len(data), size))

        self.writestore.addremotefilelognode(filename, bin(node),
                                             lz4.decompress(data))

    def requestpack(self, fileids):
        """Requests the given file revisions from the server in a pack format.

        See `remotefilelogserver.getpack` for the file format.
        """
        remote = self._connect()
        remote._callstream("getpackv1")

        groupedfiles = self._sendpackrequest(remote, fileids)

        packpath = shallowutil.getpackpath(self.repo)
        util.makedirs(packpath)
        opener = scmutil.vfs(packpath)
        # Packs should be write-once files, so set them to read-only.
        opener.createmode = 0o444
        with datapack.mutabledatapack(opener) as dpack:
            with historypack.mutablehistorypack(opener) as hpack:
                for filename in self.readfiles(remote):
                    for value in self.readhistory(remote):
                        node, p1, p2, linknode, copyfrom = value
                        hpack.add(filename, node, p1, p2, linknode, copyfrom)

                    for node, deltabase, delta in self.readdeltas(remote):
                        dpack.add(filename, node, deltabase, delta)

    def _sendpackrequest(self, remote, fileids):
        """Formats and writes the given fileids to the remote as part of a
        getpackv1 call.
        """
        # Sort the requests by name, so we receive requests in batches by name
        grouped = {}
        for filename, node in fileids:
            grouped.setdefault(filename, set()).add(node)

        # Issue request
        for filename, nodes in grouped.iteritems():
            filenamelen = struct.pack(constants.FILENAMESTRUCT, len(filename))
            countlen = struct.pack(constants.PACKREQUESTCOUNTSTRUCT, len(nodes))
            rawnodes = ''.join(bin(n) for n in nodes)

            remote.pipeo.write('%s%s%s%s' % (filenamelen, filename, countlen,
                                             rawnodes))
            remote.pipeo.flush()
        remote.pipeo.write(struct.pack(constants.FILENAMESTRUCT, 0))
        remote.pipeo.flush()

        return grouped

    def readfiles(self, remote):
        while True:
            filenamelen = readunpack(remote.pipei, constants.FILENAMESTRUCT)[0]
            if filenamelen == 0:
                break
            yield readexactly(remote.pipei, filenamelen)

    def readhistory(self, remote):
        count = readunpack(remote.pipei, '!I')[0]
        for i in xrange(count):
            entry = readunpack(remote.pipei,'!20s20s20s20sH')
            if entry[4] != 0:
                copyfrom = readexactly(remote.pipei, entry[4])
            else:
                copyfrom = ''
            entry = entry[:4] + (copyfrom,)
            yield entry

    def readdeltas(self, remote):
        count = readunpack(remote.pipei, '!I')[0]
        for i in xrange(count):
            node, deltabase, deltalen = readunpack(remote.pipei, '!20s20sQ')
            delta = readexactly(remote.pipei, deltalen)
            yield (node, deltabase, delta)

    def connect(self):
        if self.cacheprocess:
            cmd = "%s %s" % (self.cacheprocess, self.writestore._path)
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

                def request(self, value, flush=True):
                    lines = value.split("\n")
                    if lines[0] != "get":
                        return
                    self.missingids = lines[2:-1]
                    self.missingids.append('0')

                def receiveline(self):
                    if len(self.missingids) > 0:
                        return self.missingids.pop(0)
                    return None

            self.remotecache = simplecache()

    def close(self):
        if fetches and self.debugoutput:
            self.ui.warn(("%s files fetched over %d fetches - " +
                "(%d misses, %0.2f%% hit ratio) over %0.2fs\n") % (
                    fetched,
                    fetches,
                    fetchmisses,
                    float(fetched - fetchmisses) / float(fetched) * 100.0,
                    fetchcost))

        if self.remotecache.connected:
            self.remotecache.close()

        if self.remoteserver and util.safehasattr(self.remoteserver, 'cleanup'):
            self.remoteserver.cleanup()
            self.remoteserver = None

    def prefetch(self, fileids, force=False):
        """downloads the given file versions to the cache
        """
        repo = self.repo
        storepath = repo.svfs.vfs.base
        reponame = repo.name
        idstocheck = []
        for file, id in fileids:
            # hack
            # - we don't use .hgtags
            # - workingctx produces ids with length 42,
            #   which we skip since they aren't in any cache
            if (file == '.hgtags' or len(id) == 42
                or not repo.shallowmatch(file)):
                continue

            idstocheck.append((file, bin(id)))

        store = self.contentstore
        if force:
            store = self.writestore
        missingids = store.getmissing(idstocheck)

        if missingids:
            global fetches, fetched, fetchcost
            fetches += 1

            # We want to be able to detect excess individual file downloads, so
            # let's log that information for debugging.
            if fetches >= 15 and fetches < 18:
                if fetches == 15:
                    fetchwarning = self.ui.config('remotefilelog',
                                                  'fetchwarning')
                    if fetchwarning:
                        self.ui.warn(fetchwarning + '\n')
                self.logstacktrace()
            missingids = [(file, hex(id)) for file, id in missingids]
            fetched += len(missingids)
            start = time.time()
            missingids = self.request(missingids)
            if missingids:
                raise error.Abort(_("unable to download %d files") %
                                  len(missingids))
            fetchcost += time.time() - start

    def logstacktrace(self):
        import traceback
        self.ui.log('remotefilelog', 'excess remotefilelog fetching:\n%s',
                    ''.join(traceback.format_stack()))
