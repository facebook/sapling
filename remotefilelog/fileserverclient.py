# fileserverclient.py - client for communicating with the cache process
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.i18n import _
from mercurial.node import hex, bin
from mercurial import util, sshpeer, hg, error, util, wireproto, node, httppeer
import os, socket, lz4, time, grp, io
import errno
import itertools

# Statistics for debugging
fetchcost = 0
fetches = 0
fetched = 0
fetchmisses = 0

_downloading = _('downloading')

def makedirs(root, path, owner):
    try:
        os.makedirs(path)
    except OSError, ex:
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
                raise util.Abort(
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
            raise util.Abort(_("cache connection already open"))
        self.pipei, self.pipeo, self.pipee, self.subprocess = \
            util.popen4(cachecommand)
        self.connected = True

    def close(self):
        def tryclose(pipe):
            try:
                pipe.close()
            except:
                pass
        if self.connected:
            try:
                self.pipei.write("exit\n")
            except:
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
            except:
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
    def __init__(self, repo, stores):
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

        self.localcache = localcache(repo)
        def hexprefetch(keys):
            return self.prefetch((filename, hex(node)) for filename, node
                                 in keys)
        for store in stores:
            store.addfetcher(hexprefetch)
        self.contentstore = stores[0]
        self.sharedcache = stores[0]._shared

        self.remotecache = cacheconnection()
        self.remoteserver = None

    def _connect(self):
        fallbackpath = self.repo.fallbackpath
        if not self.remoteserver:
            if not fallbackpath:
                raise util.Abort("no remotefilelog server "
                    "configured - is your .hg/hgrc trusted?")
            self.remoteserver = hg.peer(self.ui, {}, fallbackpath)
        elif (isinstance(self.remoteserver, sshpeer.sshpeer) and
                 self.remoteserver.subprocess.poll() != None):
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
        localcache = self.localcache

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
                            raise util.Abort('remotefilelog requires ssh servers')
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
                        raise util.Abort("configured remotefilelog server"
                                         " does not support remotefilelog")
                finally:
                    self.ui.verbose = verbose
                # send to memcache
                count[0] = len(missed)
                request = "set\n%d\n%s\n" % (count[0], "\n".join(missed))
                cache.request(request)

            self.ui.progress(_downloading, None)

            # mark ourselves as a user of this cache
            localcache.markrepo()
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

        self.sharedcache.addremotefilelog(filename, bin(node), lz4.decompress(data))

    def connect(self):
        if self.cacheprocess:
            cmd = "%s %s" % (self.cacheprocess, self.sharedcache._path)
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
            if file == '.hgtags' or len(id) == 42 or not repo.shallowmatch(file):
                continue

            idstocheck.append((file, bin(id)))

        store = self.contentstore
        if force:
            store = self.contentstore._shared
        missingids = store.contains(idstocheck)

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
                raise util.Abort(_("unable to download %d files") % len(missingids))
            fetchcost += time.time() - start

    def logstacktrace(self):
        import traceback
        self.ui.log('remotefilelog', 'excess remotefilelog fetching:\n%s',
                    ''.join(traceback.format_stack()))

class localcache(object):
    def __init__(self, repo):
        self.ui = repo.ui
        self.repo = repo
        self.cachepath = self.ui.config("remotefilelog", "cachepath")
        if not self.cachepath:
            raise util.Abort(_("could not find config option remotefilelog.cachepath"))
        self._validatecachelog = self.ui.config("remotefilelog", "validatecachelog")
        self._validatecache = self.ui.config("remotefilelog", "validatecache",
                                             'on')
        if self._validatecache not in ('on', 'strict', 'off'):
            self._validatecache = 'on'
        if self._validatecache == 'off':
            self._validatecache = False

        if self.cachepath:
            self.cachepath = util.expandpath(self.cachepath)
        self.uid = os.getuid()

        if not os.path.exists(self.cachepath):
            oldumask = os.umask(0o002)
            try:
                os.makedirs(self.cachepath)

                groupname = self.ui.config("remotefilelog", "cachegroup")
                if groupname:
                    gid = grp.getgrnam(groupname).gr_gid
                    if gid:
                        os.chown(self.cachepath, os.getuid(), gid)
                        os.chmod(self.cachepath, 0o2775)
            finally:
                os.umask(oldumask)

    def __contains__(self, key):
        path = os.path.join(self.cachepath, key)
        exists = os.path.exists(path)

        # only validate during contains if strict mode is enabled
        # to avoid doubling iops in the hot path
        if exists and self._validatecache == 'strict' and not \
                self._validatekey(path, 'contains'):
            return False

        return exists

    def write(self, key, data):
        path = os.path.join(self.cachepath, key)
        dirpath = os.path.dirname(path)
        if not os.path.exists(dirpath):
            makedirs(self.cachepath, dirpath, self.uid)

        f = util.atomictempfile(path, 'w')
        f.write(data)
        # after a successful write, close will rename us in to place
        f.close()

        if self._validatecache:
            if not self._validatekey(path, 'write'):
                raise util.Abort(_("local cache write was corrupted %s") % path)

        stat = os.stat(path)
        if stat.st_uid == self.uid:
            os.chmod(path, 0o0664)

    def read(self, key):
        try:
            path = os.path.join(self.cachepath, key)
            with open(path, "r") as f:
                result = f.read()

            # we should never have empty files
            if not result:
                os.remove(path)
                raise KeyError("empty local cache file %s" % path)

            if self._validatecache and not self._validatedata(result, path):
                if self._validatecachelog:
                    with open(self._validatecachelog, 'a+') as f:
                        f.write("corrupt %s during read\n" % path)

                os.rename(path, path + ".corrupt")
                raise KeyError("corrupt local cache file %s" % path)

            return result
        except IOError:
            raise KeyError("key not in local cache")

    def _validatekey(self, path, action):
        with open(path, 'r') as f:
            data = f.read()

        if self._validatedata(data, path):
            return True

        if self._validatecachelog:
            with open(self._validatecachelog, 'a+') as f:
                f.write("corrupt %s during %s\n" % (path, action))

        os.rename(path, path + ".corrupt")
        return False

    def _validatedata(self, data, path):
        try:
            if len(data) > 0:
                size, remainder = data.split('\0', 1)
                size = int(size)
                if len(data) <= size:
                    # it is truncated
                    return False

                # extract the node from the metadata
                datanode = remainder[size:size+20]

                # and compare against the path
                if os.path.basename(path) == node.hex(datanode):
                    # Content matches the intended path
                    return True
                return False
        except ValueError:
            pass

        return False

    def markrepo(self):
        repospath = os.path.join(self.cachepath, "repos")
        with open(repospath, 'a') as reposfile:
            reposfile.write(os.path.dirname(self.repo.path) + "\n")

        stat = os.stat(repospath)
        if stat.st_uid == self.uid:
            os.chmod(repospath, 0o0664)

    def gc(self, keepkeys):
        ui = self.ui
        cachepath = self.cachepath
        _removing = _("removing unnecessary files")
        _truncating = _("enforcing cache limit")

        # prune cache
        import Queue
        queue = Queue.PriorityQueue()
        originalsize = 0
        size = 0
        count = 0
        removed = 0

        # keep files newer than a day even if they aren't needed
        limit = time.time() - (60 * 60 * 24)

        ui.progress(_removing, count, unit="files")
        for root, dirs, files in os.walk(cachepath):
            for file in files:
                if file == 'repos':
                    continue

                ui.progress(_removing, count, unit="files")
                path = os.path.join(root, file)
                key = os.path.relpath(path, cachepath)
                count += 1
                try:
                    stat = os.stat(path)
                except OSError as e:
                    if e.errno != errno.ENOENT: # errno.ENOENT = no such file or directory
                        raise
                    msg = _("warning: file %s was removed by another process\n")
                    ui.warn(msg % path)
                    continue

                originalsize += stat.st_size

                if key in keepkeys or stat.st_atime > limit:
                    queue.put((stat.st_atime, path, stat))
                    size += stat.st_size
                else:
                    try:
                        os.remove(path)
                    except OSError as e:
                        if e.errno != errno.ENOENT: # errno.ENOENT = no such file or directory
                            raise
                        msg = _("warning: file %s was removed by another process\n")
                        ui.warn(msg % path)
                        continue
                    removed += 1
        ui.progress(_removing, None)

        # remove oldest files until under limit
        limit = ui.configbytes("remotefilelog", "cachelimit", "1000 GB")
        if size > limit:
            excess = size - limit
            removedexcess = 0
            while queue and size > limit and size > 0:
                ui.progress(_truncating, removedexcess, unit="bytes", total=excess)
                atime, oldpath, stat = queue.get()
                try:
                    os.remove(oldpath)
                except OSError as e:
                    if e.errno != errno.ENOENT: # errno.ENOENT = no such file or directory
                        raise
                    msg = _("warning: file %s was removed by another process\n")
                    ui.warn(msg % oldpath)
                size -= stat.st_size
                removed += 1
                removedexcess += stat.st_size
        ui.progress(_truncating, None)

        ui.status("finished: removed %s of %s files (%0.2f GB to %0.2f GB)\n" %
                  (removed, count, float(originalsize) / 1024.0 / 1024.0 / 1024.0,
                  float(size) / 1024.0 / 1024.0 / 1024.0))
