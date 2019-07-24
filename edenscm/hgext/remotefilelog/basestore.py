# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import hashlib
import itertools
import os
import random
import shutil
import stat
import time

from edenscm.mercurial import error, phases, progress, pycompat, revlog, util
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import bin, hex

from . import constants, datapack, historypack, shallowutil


try:
    xrange(0)
except NameError:
    xrange = range


# Cache of filename sha to filename, to prevent repeated search for the same
# filename shas.
filenamehashcache = {}


class basestore(object):
    def __init__(self, repo, path, reponame, shared=False):
        """Creates a remotefilelog store object for the given repo name.

        `path` - The file path where this store keeps its data
        `reponame` - The name of the repo. This is used to partition data from
        many repos.
        `shared` - True if this store is a shared cache of data from the central
        server, for many repos on this machine. False means this store is for
        the local data for one repo.
        """
        self.repo = repo
        self.ui = repo.ui
        self._path = path
        self._reponame = reponame
        self._shared = shared
        self._uid = os.getuid() if not pycompat.iswindows else None

        self._validatecachelog = self.ui.config("remotefilelog", "validatecachelog")
        self._validatecache = self.ui.config("remotefilelog", "validatecache", "on")
        self._validatehashes = self.ui.configbool(
            "remotefilelog", "validatecachehashes", True
        )
        self._incrementalloosefilesrepack = self.ui.configbool(
            "remotefilelog", "incrementalloosefilerepack", True
        )
        if self._validatecache not in ("on", "strict", "off"):
            self._validatecache = "on"
        if self._validatecache == "off":
            self._validatecache = False

        self._mutablepacks = None

        self._repackdir = None

        if shared:
            shallowutil.mkstickygroupdir(self.ui, path)

    def getmissing(self, keys):
        missing = []
        with progress.bar(
            self.repo.ui, _("discovering"), _("files"), len(keys)
        ) as prog:
            for name, node in keys:
                prog.value += 1

                filepath = self._getfilepath(name, node)
                try:
                    size = os.path.getsize(filepath)
                    # An empty file is considered corrupt and we pretend it
                    # doesn't exist.
                    exists = size > 0
                except os.error:
                    exists = False

                if (
                    exists
                    and self._validatecache == "strict"
                    and not self._validatekey(filepath, "contains")
                ):
                    exists = False
                if not exists:
                    missing.append((name, node))

        return missing

    # BELOW THIS ARE IMPLEMENTATIONS OF REPACK SOURCE

    def markledger(self, ledger, options=None):
        if options and options.get(constants.OPTION_PACKSONLY):
            return

        incremental = False
        if options and options.get("incremental") and self._incrementalloosefilesrepack:
            incremental = True

        with ledger.location(self._path):
            for filename, nodes in self._getfiles(incremental):
                for node in nodes:
                    ledger.markdataentry(self, filename, node)
                    ledger.markhistoryentry(self, filename, node)

    def cleanup(self, ledger):
        entries = ledger.sources.get(self, [])
        with progress.bar(self.ui, _("cleaning up"), _("files"), len(entries)) as prog:
            for entry in entries:
                if entry.gced or (entry.datarepacked and entry.historyrepacked):
                    path = self._getfilepath(entry.filename, entry.node)
                    util.tryunlink(path)
                prog.value += 1

        if self._repackdir is not None:
            # Clean up the repo cache directory.
            self._cleanupdirectory(self._repackdir)

    def markforrefresh(self):
        # This only applies to stores that keep a snapshot of whats on disk.
        pass

    # BELOW THIS ARE NON-STANDARD APIS

    def _cleanupdirectory(self, rootdir):
        """Removes the empty directories and unnecessary files within the root
        directory recursively. Note that this method does not remove the root
        directory itself. """

        oldfiles = set()
        otherfiles = set()
        havefilename = False
        # util.listdir returns stat information which saves some rmdir/listdir
        # syscalls.
        for name, mode in util.listdir(rootdir):
            if stat.S_ISDIR(mode):
                dirpath = os.path.join(rootdir, name)
                self._cleanupdirectory(dirpath)

                # Now that the directory specified by dirpath is potentially
                # empty, try and remove it.
                try:
                    os.rmdir(dirpath)
                except OSError:
                    pass

            elif stat.S_ISREG(mode):
                if name == "filename":
                    havefilename = True
                elif name.endswith("_old"):
                    oldfiles.add(name[:-4])
                else:
                    otherfiles.add(name)

        # Remove the files which end with suffix '_old' and have no
        # corresponding file without the suffix '_old'. See addremotefilelognode
        # method for the generation/purpose of files with '_old' suffix.
        for filename in oldfiles - otherfiles:
            filepath = os.path.join(rootdir, filename + "_old")
            util.tryunlink(filepath)

        # If we've deleted all the files and have a "filename" left over, delete
        # the filename, too.
        if havefilename and not otherfiles:
            filepath = os.path.join(rootdir, "filename")
            util.tryunlink(filepath)

    def _getfiles(self, incrementalrepack):
        """Return a list of (filename, [node,...]) for all the revisions that
        exist in the store.

        This is useful for obtaining a list of all the contents of the store
        when performing a repack to another store, since the store API requires
        name+node keys and not namehash+node keys.
        """
        existing = {}
        for filenamehash, node in self._listkeys(incrementalrepack):
            existing.setdefault(filenamehash, []).append(node)

        filenamemap = self._resolvefilenames(existing.keys())

        for filename, sha in filenamemap.iteritems():
            yield (filename, existing[sha])

    def _resolvefilenames(self, hashes):
        """Given a list of filename hashes that are present in the
        remotefilelog store, return a mapping from filename->hash.

        This is useful when converting remotefilelog blobs into other storage
        formats.
        """
        if not hashes:
            return {}

        filenames = {}
        missingfilename = set(hashes)

        if self._shared:
            getfilenamepath = lambda sha: os.path.join(
                self._path, self._reponame, sha[:2], sha[2:], "filename"
            )
        else:
            getfilenamepath = lambda sha: os.path.join(self._path, sha, "filename")

        # Search the local cache and filename files in case we look for files
        # we've already found
        for sha in hashes:
            if sha in filenamehashcache:
                if filenamehashcache[sha] is not None:
                    filenames[filenamehashcache[sha]] = sha
                missingfilename.discard(sha)
            filenamepath = getfilenamepath(hex(sha))
            if os.path.exists(filenamepath):
                try:
                    filename = shallowutil.readfile(filenamepath)
                except Exception:
                    pass
                else:
                    checksha = hashlib.sha1(filename).digest()
                    if checksha == sha:
                        filenames[filename] = sha
                        filenamehashcache[sha] = filename
                        missingfilename.discard(sha)
                    else:
                        # The filename file is invalid - delete it.
                        util.tryunlink(filenamepath)

        if not missingfilename:
            return filenames

        # Scan all draft commits and the last 250000 commits in the changelog
        # looking for the files. If they're not there, we don't bother looking
        # further.
        # developer config: remotefilelog.resolvechangeloglimit
        unfi = self.repo.unfiltered()
        cl = unfi.changelog
        revs = list(unfi.revs("not public()"))
        scanlen = min(
            len(cl), self.ui.configint("remotefilelog", "resolvechangeloglimit", 250000)
        )
        remainingstr = "%d remaining" % len(missingfilename)
        with progress.bar(
            self.ui, "resolving filenames", total=len(revs) + scanlen
        ) as prog:
            for i, rev in enumerate(
                itertools.chain(revs, xrange(len(cl) - 1, len(cl) - scanlen, -1))
            ):
                files = cl.readfiles(cl.node(rev))
                prog.value = i, remainingstr
                for filename in files:
                    sha = hashlib.sha1(filename).digest()
                    if sha in missingfilename:
                        filenames[filename] = sha
                        filenamehashcache[sha] = filename
                        missingfilename.discard(sha)
                        remainingstr = "%d remaining" % len(missingfilename)
                        if not missingfilename:
                            break

        # Record anything we didn't find in the cache so that we don't look
        # for it again.
        filenamehashcache.update((h, None) for h in missingfilename)

        return filenames

    def _getrepocachepath(self):
        return os.path.join(self._path, self._reponame) if self._shared else self._path

    def _getincrementalrootdir(self):
        rootdir = self._getrepocachepath()
        entries = os.listdir(rootdir)
        entries = [os.path.join(rootdir, p) for p in entries]
        entries = [folder for folder in entries if os.path.isdir(folder)]

        if len(entries) == 0:
            return None

        # Since the distribution of loosefile should be uniform accross all of
        # the loosefile directories, let's randomly pick one to repack.
        for tries in range(10):
            entry = entries[random.randrange(len(entries))]
            for root, dirs, files in os.walk(entry):
                for filename in files:
                    if len(filename) != 40:
                        continue

                    try:
                        int(filename, 16)
                    except ValueError:
                        continue

                    parent, d = os.path.split(root)
                    if self._shared:
                        d += os.path.basename(parent)

                    if len(d) != 40:
                        continue

                    try:
                        int(d, 16)
                    except ValueError:
                        continue

                    if self._shared:
                        return parent
                    else:
                        return root
        return None

    def _listkeys(self, incrementalrepack):
        """List all the remotefilelog keys that exist in the store.

        Returns a iterator of (filename hash, filecontent hash) tuples.
        """

        if self._repackdir is not None:
            rootdir = self._repackdir
        else:
            if not incrementalrepack:
                rootdir = self._getrepocachepath()
            else:
                rootdir = self._getincrementalrootdir()
            self._repackdir = rootdir

        if rootdir is not None:
            for root, dirs, files in os.walk(rootdir):
                for filename in files:
                    if len(filename) != 40:
                        continue
                    node = filename
                    if self._shared:
                        # .../1a/85ffda..be21
                        filenamehash = root[-41:-39] + root[-38:]
                    else:
                        filenamehash = root[-40:]

                    self._reportmetrics(root, filename)

                    yield (bin(filenamehash), bin(node))

    def _reportmetrics(self, root, filename):
        """Log total remotefilelog blob size and count.

        The method is overloaded in remotefilelogstore class, because we can
        only count metrics for the datastore. History is kept in the same files
        so we don't need to log metrics twice.
        """
        pass

    def _getfilepath(self, name, node):
        """
        The path of the file used to store the content of the named file
        with a particular node hash.
        """
        node = hex(node)
        if self._shared:
            key = shallowutil.getcachekey(self._reponame, name, node)
        else:
            key = shallowutil.getlocalkey(name, node)

        return os.path.join(self._path, key)

    def _getfilenamepath(self, name):
        """
        The path of the file used to store the name of the named file.  This
        allows reverse lookup from the hashed name back to the original name.

        This is a file named ``filename`` inside the directory where the file
        content is stored.
        """
        if self._shared:
            key = shallowutil.getcachekey(self._reponame, name, "filename")
        else:
            key = shallowutil.getlocalkey(name, "filename")

        return os.path.join(self._path, key)

    def _getdata(self, name, node):
        filepath = self._getfilepath(name, node)
        filenamepath = self._getfilenamepath(name)
        try:
            data = shallowutil.readfile(filepath)
            if not os.path.exists(filenamepath):
                try:
                    shallowutil.writefile(filenamepath, name, readonly=True)
                except Exception:
                    pass
            if self._validatecache:
                validationresult = self._validatedata(data, filepath)

                if validationresult == shallowutil.ValidationResult.Invalid:
                    if self._validatecachelog:
                        with util.posixfile(self._validatecachelog, "a+") as f:
                            f.write("corrupt %s during read\n" % filepath)
                    os.rename(filepath, filepath + ".corrupt")
                    raise KeyError("corrupt local cache file %s" % filepath)
            else:
                # only check if the content is censored
                offset, size, flags = shallowutil.parsesizeflags(data)
                text = data[offset : offset + size]
                validationresult = (
                    shallowutil.ValidationResult.Censored
                    if shallowutil.verifycensoreddata(text)
                    else shallowutil.ValidationResult.Valid
                )

            if validationresult == shallowutil.ValidationResult.Censored:
                data = self.createcensoredfileblob(data)
        except IOError:
            raise KeyError(
                "no file found at %s for %s:%s" % (filepath, name, hex(node))
            )

        return data

    def createcensoredfileblob(self, raw):
        """Creates a fileblob that contains a default message when
        the file is blacklisted and the actual content cannot be accessed.
        """
        offset, size, flags = shallowutil.parsesizeflags(raw)
        ancestortext = raw[offset + size :]
        text = constants.BLACKLISTED_MESSAGE
        revlogflags = revlog.REVIDX_DEFAULT_FLAGS
        header = shallowutil.buildfileblobheader(len(text), revlogflags)

        return "%s\0%s%s" % (header, text, ancestortext)

    def addremotefilelognode(self, name, node, data):
        filepath = self._getfilepath(name, node)
        filenamepath = self._getfilenamepath(name)

        oldumask = os.umask(0o002)
        try:
            # if this node already exists, save the old version for
            # recovery/debugging purposes.
            if os.path.exists(filepath):
                newfilename = filepath + "_old"
                # newfilename can be read-only and shutil.copy will fail.
                # Delete newfilename to avoid it
                if os.path.exists(newfilename):
                    shallowutil.unlinkfile(newfilename)
                shutil.copy(filepath, newfilename)

            shallowutil.mkstickygroupdir(self.ui, os.path.dirname(filepath))
            shallowutil.writefile(filepath, data, readonly=True)
            if not os.path.exists(filenamepath):
                shallowutil.writefile(filenamepath, name, readonly=True)

            if self._validatecache:
                if not self._validatekey(filepath, "write"):
                    raise error.Abort(
                        _("local cache write was corrupted %s") % filepath
                    )
        finally:
            os.umask(oldumask)

    def markrepo(self, path):
        """Call this to add the given repo path to the store's list of
        repositories that are using it. This is useful later when doing garbage
        collection, since it allows us to insecpt the repos to see what nodes
        they want to be kept alive in the store.
        """
        repospath = os.path.join(self._path, "repos")
        line = os.path.dirname(path) + "\n"
        # Skip writing to the repos file if the line is already written.
        try:
            if line in util.iterfile(open(repospath, "rb")):
                return
        except IOError:
            pass

        with util.posixfile(repospath, "a") as reposfile:
            reposfile.write(line)

        repospathstat = os.stat(repospath)
        if repospathstat.st_uid == self._uid:
            os.chmod(repospath, 0o0664)

    def _validatekey(self, path, action):
        with util.posixfile(path, "rb") as f:
            data = f.read()

        validationresult = self._validatedata(data, path)
        if validationresult != shallowutil.ValidationResult.Invalid:
            return True

        if self._validatecachelog:
            with util.posixfile(self._validatecachelog, "a+") as f:
                f.write("corrupt %s during %s\n" % (path, action))

        os.rename(path, path + ".corrupt")
        return False

    def _validatedata(self, data, path):
        try:
            if len(data) > 0:
                # see remotefilelogserver.createfileblob for the format
                offset, size, flags = shallowutil.parsesizeflags(data)
                if len(data) <= size:
                    # it is truncated
                    return shallowutil.ValidationResult.Invalid

                # extract the node from the metadata
                offset += size
                datanode = data[offset : offset + 20]

                hexdatanode = hex(datanode)
                validationresult = shallowutil.verifyfilenode(
                    self.ui, data, hexdatanode, self._validatehashes
                )

                if validationresult == shallowutil.ValidationResult.Invalid:
                    return validationresult

                # and compare against the path
                if os.path.basename(path) == hexdatanode:
                    # Content matches the intended path
                    return validationresult

                return shallowutil.ValidationResult.Invalid
        except (ValueError, RuntimeError):
            pass

        return shallowutil.ValidationResult.Invalid

    def gc(self, keepkeys):
        ui = self.ui
        cachepath = self._path

        # prune cache
        import Queue

        queue = Queue.PriorityQueue()
        originalsize = 0
        size = 0
        count = 0
        removed = 0

        # keep files newer than a day even if they aren't needed
        limit = time.time() - (60 * 60 * 24)

        with progress.bar(ui, _("removing unnecessary files"), _("files")) as prog:
            for root, dirs, files in os.walk(cachepath):
                for file in files:
                    if file == "repos" or file == "filename":
                        continue

                    # Don't delete pack files or indexedlog data.
                    if "/packs/" in root or "/indexedlogdatastore/" in root:
                        continue

                    count += 1
                    prog.value = count
                    path = os.path.join(root, file)
                    key = os.path.relpath(path, cachepath)
                    try:
                        pathstat = os.stat(path)
                    except OSError as e:
                        # errno.ENOENT = no such file or directory
                        if e.errno != errno.ENOENT:
                            raise
                        msg = _("warning: file %s was removed by another " "process\n")
                        ui.warn(msg % path)
                        continue

                    originalsize += pathstat.st_size

                    if key in keepkeys or pathstat.st_atime > limit:
                        queue.put((pathstat.st_atime, path, pathstat))
                        size += pathstat.st_size
                    else:
                        try:
                            shallowutil.unlinkfile(path)
                        except OSError as e:
                            # errno.ENOENT = no such file or directory
                            if e.errno != errno.ENOENT:
                                raise
                            msg = _(
                                "warning: file %s was removed by another " "process\n"
                            )
                            ui.warn(msg % path)
                            continue
                        removed += 1

        # remove oldest files until under limit
        limit = ui.configbytes("remotefilelog", "cachelimit", "1000 GB")
        if size > limit:
            excess = size - limit
            with progress.bar(
                ui, _("enforcing cache limit"), _("bytes"), excess
            ) as prog:
                while queue and size > limit and size > 0:
                    atime, oldpath, oldpathstat = queue.get()
                    try:
                        shallowutil.unlinkfile(oldpath)
                    except OSError as e:
                        # errno.ENOENT = no such file or directory
                        if e.errno != errno.ENOENT:
                            raise
                        msg = _("warning: file %s was removed by another " "process\n")
                        ui.warn(msg % oldpath)
                    size -= oldpathstat.st_size
                    removed += 1
                    prog.value += oldpathstat.st_size

        ui.status(
            _("finished: removed %s of %s files (%0.2f GB to %0.2f GB)\n")
            % (
                removed,
                count,
                float(originalsize) / 1024.0 / 1024.0 / 1024.0,
                float(size) / 1024.0 / 1024.0 / 1024.0,
            )
        )

    def handlecorruption(self, name, node):
        filepath = self._getfilepath(name, node)
        if self._shared:
            self.ui.warn(_("detected corruption in '%s', moving it aside\n") % filepath)
            os.rename(filepath, filepath + ".corrupt")
            # Throw a KeyError so UnionStore can catch it and proceed to the
            # next store.
            raise KeyError(
                "corruption in file '%s' for %s:%s" % (filepath, name, hex(node))
            )
        else:
            # Throw a ValueError so UnionStore does not attempt to read further
            # stores, since local data corruption is not recoverable.
            raise ValueError(
                "corruption in file '%s' for %s:%s" % (filepath, name, hex(node))
            )
