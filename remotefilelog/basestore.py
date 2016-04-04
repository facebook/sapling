import os, shutil, time
import shallowutil
from mercurial import util
from mercurial.i18n import _
from mercurial.node import hex

class basestore(object):
    def __init__(self, ui, path, reponame, shared=False):
        path = util.expandpath(path)
        self.ui = ui
        self._path = path
        self._reponame = reponame
        self._shared = shared
        self._uid = os.getuid()
        self._fetches = []

        self._validatecachelog = self.ui.config("remotefilelog", "validatecachelog")
        self._validatecache = self.ui.config("remotefilelog", "validatecache",
                                             'on')
        if self._validatecache not in ('on', 'strict', 'off'):
            self._validatecache = 'on'
        if self._validatecache == 'off':
            self._validatecache = False

        if shared:
            if not os.path.exists(path):
                oldumask = os.umask(0o002)
                try:
                    os.makedirs(path)

                    groupname = self.ui.config("remotefilelog", "cachegroup")
                    if groupname:
                        gid = grp.getgrnam(groupname).gr_gid
                        if gid:
                            os.chown(cachepath, os.getuid(), gid)
                            os.chmod(cachepath, 0o2775)
                finally:
                    os.umask(oldumask)

    def addfetcher(self, fetchfunc):
        self._fetches.append(fetchfunc)

    def triggerfetches(self, keys):
        for fetcher in self._fetches:
            fetcher(keys)

    def contains(self, keys):
        missing = []
        for name, node in keys:
            filepath = self._getfilepath(name, node)
            exists = os.path.exists(filepath)
            if (exists and self._validatecache == 'strict' and
                not self._validatekey(filepath, 'contains')):
                exists = False
            if not exists:
                missing.append((name, node))

        return missing

    # BELOW THIS ARE NON-STANDARD APIS

    def _getfilepath(self, name, node):
        node = hex(node)
        if self._shared:
            key = shallowutil.getcachekey(self._reponame, name, node)
        else:
            key = shallowutil.getlocalkey(name, node)

        return os.path.join(self._path, key)

    def _getdata(self, name, node):
        filepath = self._getfilepath(name, node)
        try:
            data = shallowutil.readfile(filepath)
            if self._validatecache and not self._validatedata(data, filepath):
                if self._validatecachelog:
                    with open(self._validatecachelog, 'a+') as f:
                        f.write("corrupt %s during read\n" % filepath)
                os.rename(filepath, filepath + ".corrupt")
                raise KeyError("corrupt local cache file %s" % filepath)
        except IOError:
            raise KeyError("no file found at %s for %s:%s" % (filepath, name, hex(node)))

        return data

    def addremotefilelog(self, name, node, data):
        filepath = self._getfilepath(name, node)

        oldumask = os.umask(0o002)
        try:
            # if this node already exists, save the old version for
            # recovery/debugging purposes.
            if os.path.exists(filepath):
                newfilename = filepath + '_old'
                # newfilename can be read-only and shutil.copy will fail.
                # Delete newfilename to avoid it
                if os.path.exists(newfilename):
                    os.unlink(newfilename)
                shutil.copy(filepath, newfilename)
                # writefile creates atomictempfile, which copies
                # access permission from file 'path', if it exists.
                # It's better to delete it
                os.unlink(filepath)

            shallowutil.writefile(filepath, data, readonly=True)

            if self._validatecache:
                if not self._validatekey(filepath, 'write'):
                    raise util.Abort(_("local cache write was corrupted %s") % path)
        finally:
            os.umask(oldumask)

    def markrepo(self, path):
        repospath = os.path.join(self._path, "repos")
        with open(repospath, 'a') as reposfile:
            reposfile.write(os.path.dirname(path) + "\n")

        stat = os.stat(repospath)
        if stat.st_uid == self._uid:
            os.chmod(repospath, 0o0664)

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
                if os.path.basename(path) == hex(datanode):
                    # Content matches the intended path
                    return True
                return False
        except ValueError:
            pass

        return False

    def gc(self, keepkeys):
        ui = self.ui
        cachepath = self._path
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
