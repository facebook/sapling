# fileserverclient.py - client for communicating with the cache process
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.i18n import _
from mercurial import util, sshpeer
import os, socket, lz4, time

# Statistics for debugging
fetchcost = 0
fetches = 0
fetched = 0
fetchmisses = 0

_downloading = _('downloading')

client = None

def getcachekey(file, id):
    pathhash = util.sha1(file).hexdigest()
    return os.path.join(pathhash, id)

class fileserverclient(object):
    """A client for requesting files from the remote file server.
    """
    def __init__(self, ui):
        self.ui = ui
        self.cachepath = ui.config("remotefilelog", "cachepath")
        self.cacheprocess = ui.config("remotefilelog", "cacheprocess")
        self.debugoutput = ui.configbool("remotefilelog", "debug")

        self.pipeo = self.pipei = self.pipee = None

        if not os.path.exists(self.cachepath):
            os.makedirs(self.cachepath)

    def request(self, repo, fileids):
        """Takes a list of filename/node pairs and fetches them from the
        server. Files are stored in the self.cachepath.
        A list of nodes that the server couldn't find is returned.
        If the connection fails, an exception is raised.
        """

        if not self.pipeo:
            self.connect()

        count = len(fileids)
        request = "get\n%d\n" % count
        idmap = {}
        for file, id in fileids:
            pathhash = util.sha1(file).hexdigest()
            fullid = "%s/%s" % (pathhash, id)
            request += fullid + "\n"
            idmap[fullid] = file

        self.pipei.write(request)
        self.pipei.flush()

        missing = []
        total = count
        self.ui.progress(_downloading, 0, total=count)

        fallbackrepo = repo.ui.config("remotefilelog", "fallbackrepo",
                                      repo.ui.config("paths", "default"))

        remote = None
        missed = []
        count = 0
        while True:
            missingid = self.pipeo.readline()[:-1]
            if not missingid:
                raise util.Abort(_("error downloading file contents: " +
                                   "connection closed early"))
            if missingid == "0":
                break
            if missingid.startswith("_hits_"):
                # receive progress reports
                parts = missingid.split("_")
                count += int(parts[2])
                self.ui.progress(_downloading, count, total=total)
                continue

            missed.append(missingid)

            # fetch from the master
            if not remote:
                verbose = self.ui.verbose
                try:
                    # When verbose is true, sshpeer prints 'running ssh...'
                    # to stdout, which can interfere with some command
                    # outputs
                    self.ui.verbose = False

                    remote = sshpeer.sshpeer(self.ui, fallbackrepo)
                    remote._callstream("getfiles")
                finally:
                    self.ui.verbose = verbose

            id = missingid[-40:]
            file = idmap[missingid]
            sshrequest = "%s%s\n" % (id, file)
            remote.pipeo.write(sshrequest)
            remote.pipeo.flush()


        count = total - len(missed)
        self.ui.progress(_downloading, count, total=total)

        oldumask = os.umask(0o002)
        try:
            # receive cache misses from master
            if missed:
                global fetchmisses
                fetchmisses += len(missed)
                # process remote
                pipei = remote.pipei
                for id in missed:
                    size = int(pipei.readline()[:-1])
                    data = pipei.read(size)

                    count += 1
                    self.ui.progress(_downloading, count, total=total)

                    idcachepath = os.path.join(self.cachepath, id)
                    dirpath = os.path.dirname(idcachepath)
                    if not os.path.exists(dirpath):
                        os.makedirs(dirpath)
                    f = open(idcachepath, "w")
                    try:
                        f.write(lz4.decompress(data))
                    finally:
                        f.close()

                remote.cleanup()
                remote = None

                # send to memcache
                count = len(missed)
                request = "set\n%d\n%s\n" % (count, "\n".join(missed))

                self.pipei.write(request)
                self.pipei.flush()

            self.ui.progress(_downloading, None)

            # mark ourselves as a user of this cache
            repospath = os.path.join(self.cachepath, "repos")
            reposfile = open(repospath, 'a')
            reposfile.write(os.path.dirname(repo.path) + "\n")
            reposfile.close()
        finally:
            os.umask(oldumask)

        return missing

    def connect(self):
        cmd = "%s %s" % (self.cacheprocess, self.cachepath)
        self.pipei, self.pipeo, self.pipee, self.subprocess = util.popen4(cmd)

    def close(self):
        if fetches and self.debugoutput:
            self.ui.warn(("%s files fetched over %d fetches - " +
                "(%d misses, %0.2f%% hit ratio) over %0.2fs\n") % (
                    fetched,
                    fetches,
                    fetchmisses,
                    float(fetched - fetchmisses) / float(fetched) * 100.0,
                    fetchcost))

        # if the process is still open, close the pipes
        if self.pipeo and self.subprocess.poll() == None:
            self.pipei.write("exit\n")
            self.pipei.close()
            self.pipeo.close()
            self.pipee.close()
            self.subprocess.wait()
            del self.subprocess
            self.pipeo = None
            self.pipei = None
            self.pipee = None

    def prefetch(self, repo, fileids):
        """downloads the given file versions to the cache
        """
        storepath = repo.sopener.vfs.base
        missingids = []
        for file, id in fileids:
            # hack
            # - we don't use .hgtags
            # - workingctx produces ids with length 42,
            #   which we skip since they aren't in any cache
            if file == '.hgtags' or len(id) == 42:
                continue

            key = getcachekey(file, id)
            idcachepath = os.path.join(self.cachepath, key)
            idlocalpath = os.path.join(storepath, 'data', key)
            if os.path.exists(idcachepath) or os.path.exists(idlocalpath):
                continue

            missingids.append((file, id))

        if missingids:
            global fetches, fetched, fetchcost
            fetches += 1
            fetched += len(missingids)
            start = time.time()
            missingids = self.request(repo, missingids)
            if missingids:
                raise util.Abort(_("unable to download %d files") % len(missingids))
            fetchcost += time.time() - start
