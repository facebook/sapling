# manifestdiskcache.py - manifest disk cache for mercurial
#
# Copyright 2012 Facebook
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''Cache manifests on disk to speed up access.

This extension intercepts reads and writes of manifests to cache them on disk.
Enable by setting the config variable manifestdiskcache.enabled to True.

On writes, we spawn a second process (to avoid penalizing interactive use) to
check if we should prune the cache.  The pruning is guided by several
configuration variables:

manifestdiskcache.pinned-revsets: revsets to pin in the cache.

manifestdiskcache.cache-size: the upper limit for the size of the cache.

manifestdiskcache.runs-between-prunes: the approximate number of writes that
will elapse before we prune.

manifestdiskcache.seconds-between-prunes: the number of seconds since the last
prune that can elapse before we prune.

Because this is a cache, exceptions are generally suppresed.  If the
configuration variable manifestdiskcache.logging is set to True, exceptions will
be written to standard error, but will still be suppressed.
err

'''

from mercurial import bookmarks, changegroup, cmdutil, error, extensions
from mercurial import localrepo, manifest, revlog, util
from mercurial.node import bin, hex
from mercurial.i18n import _

import collections
import os
import random
import subprocess
import sys
import time
import traceback

from extutil import replaceclass

CACHE_SUBDIR = 'manifestdiskcache'
CONFIG_KEY = 'manifestdiskcache'
REPO_ROOT_KEY = 'manifestdiskcachee.repo_root'
HEX_SHA_SIZE_BYTES = 40

testedwith = 'internal'

def extsetup(ui):
    global logging
    logging = ui.configbool(CONFIG_KEY, 'logging', False)

cmdtable = {}
command = cmdutil.command(cmdtable)
@command(
    'prunemanifestdiskcache', [],
    _('hg prunemanifestdiskcache'))
def prunemanifestdiskcache(ui, repo):
    masterrevset = _masterrevset(ui, repo)

    # retrieve the options.
    pinnedrevsets = ui.config(CONFIG_KEY,
                              'pinned-revsets',
                              "{0} or (draft() and date(-3))".format(
                                  masterrevset))
    cachesizelimit = ui.configbytes(CONFIG_KEY, 'cache-size', '5g')
    runsbetween = ui.configint(CONFIG_KEY, 'runs-between-prunes', 100)
    secondsbetween = ui.configint(CONFIG_KEY, 'seconds-between-prunes', 86400)

    # validate the arguments
    if runsbetween < 1:
        raise error.Abort("runs-between-prunes should be >= 1")
    if secondsbetween < 0:
        raise error.Abort("seconds-between-prunes should be >= 0")

    store = repo.store
    opener = store.opener
    base = store.opener.join(None)

    # decide whether we run.
    markerpath = os.path.join(base, CACHE_SUBDIR, '.marker')
    try:
        stat = os.stat(markerpath)
    except OSError:
        # create the file.
        with open(markerpath, 'w'):
            pass
    else:
        now = time.time()
        delta = now - stat.st_mtime

        intercept = (1.0 / runsbetween)
        odds = intercept + (((1 - intercept) * delta) / secondsbetween)

        if odds < random.random():
            # no pruning.
            ui.note(_("no pruning needed at this time."))
            return

        # update the file timestamp.
        os.utime(markerpath, None)

    # fined all the pinned revs.
    changelog = repo.changelog
    revs = set()
    if pinnedrevsets:
        try:
            revs = repo.revs(pinnedrevsets)
        except error.ParseError:
            error.Abort("Cannot parse {0}.pinned-revsets.".format(CONFIG_KEY))

    pinnednodes = set(hex(changelog.read(changelog.node(rev))[0])
                      for rev in revs)

    # enumerate all the existing cache entries, ordered by time ascending.
    entries = []
    for dirpath, dirs, files in opener.walk(CACHE_SUBDIR):
        for fname in files:
            # don't remove the marker.
            if fname == '.marker':
                continue

            path = os.path.join(base, dirpath, fname)
            if len(fname) > HEX_SHA_SIZE_BYTES:
                # this is probably a temp file.  trash it, but do it directly,
                # because opener.unlink will try to case-escape.
                try:
                    os.unlink(path)
                except Exception:
                    pass
                continue

            if fname in pinnednodes:
                # pinned rev, move on.
                continue

            try:
                stat = os.stat(path)
            except OSError:
                # file presumably does not exist.
                continue

            entries.append(
                (stat.st_atime, stat.st_size, path))
    entries.sort(reverse=True)
    ui.debug("pid: {0}\ncache entries: {1}\n".format(
        os.getpid(),
        "\n".join(["{0}".format(entry)
                   for entry in entries])))

    # accumulate up to cachesize, then remove the remainder.
    accumsize = 0
    for atime, size, path in entries:
        accumsize += size

        if accumsize > cachesizelimit:
            # remove the file, but once again, do it directly because
            # opener.unlink will try to case-escape.
            try:
                os.unlink(path)
            except Exception:
                pass

@replaceclass(changegroup, 'cg1unpacker')
class cg1unpackerwithdc(changegroup.cg1unpacker):
    def apply(self, repo, *args, **kwargs):
        # disable manifest caching.
        repo.manifest.markbatchoperationstart()
        try:
            # call the original function
            return super(cg1unpackerwithdc, self).apply(repo, *args, **kwargs)
        finally:
            # re-enable manifest caching.
            repo.manifest.markbatchoperationend()

@replaceclass(manifest, 'manifest')
class manifestwithdc(manifest.manifest):
    def __init__(self, opener, dir='', dirlogcache=None):
        super(manifestwithdc, self).__init__(opener, dir, dirlogcache)

        self.manifestdiskcacheenabled = False
        opts = getattr(opener, 'options', None)
        if opts is not None:
            self.manifestdiskcacheenabled = opts.get(
                CONFIG_KEY, False)

        if self.manifestdiskcacheenabled:
            # this logic is copied from the constructor of manifest.__init__
            if self._dir:
                self.diskcachedir = "meta/" + self._dir + CACHE_SUBDIR
            else:
                self.diskcachedir = CACHE_SUBDIR

            self.inbatchoperation = False
            self.repo_root = opts[REPO_ROOT_KEY]

    def markbatchoperationstart(self):
        self.inbatchoperation = True

    def markbatchoperationend(self):
        self.inbatchoperation = False

    def revision(self, nodeorrev, *args, **kwargs):
        global logging

        if self.manifestdiskcacheenabled:
            expectedexception = False

            try:
                if isinstance(nodeorrev, int):
                    rev = nodeorrev
                    node = self.node(nodeorrev)
                else:
                    rev = self.rev(nodeorrev)
                    node = nodeorrev

                hexnode = hex(node)

                subpath = os.path.join(self.diskcachedir,
                                       hexnode[0:2], hexnode[2:4], hexnode)

                result = None
                try:
                    with self.opener(subpath, "r") as fh:
                        result = fh.read()
                except IOError:
                    # this is an expected exception, so no need to sound the
                    # alarms.
                    expectedexception = True
                    raise

                if result:
                    # verify that the output passes _checkhash(..)
                    result = self._checkhash(result, node, rev)

                    return result
            except Exception:
                # it's a cache.  suppress the exception, disable caching
                # going forward, and then report if logging is enabled.
                if logging and not expectedexception:
                    sys.stderr.write("Encountered exception in extension "
                                     "manifestdiskcache: {0}\n".format(
                                         traceback.format_exc()))

        result = super(manifestwithdc, self).revision(nodeorrev,
                                                      *args, **kwargs)

        if self.manifestdiskcacheenabled:
            self._writetomanifestcache(hexnode, result, logging)
            self._prune_cache()

        return result

    def _addrevision(self, node, text, *args, **kwargs):
        global logging

        node = super(manifestwithdc, self)._addrevision(
            node, text, *args, **kwargs)

        if self.manifestdiskcacheenabled and not self.inbatchoperation:
            hexnode = hex(node)
            self._writetomanifestcache(hexnode, str(text), logging)

            self._prune_cache()

        return node

    def _writetomanifestcache(self, hexnode, text, loggingenabled):
        try:
            base = self.opener.join(None)
            dirsubpath = os.path.join(self.diskcachedir,
                                      hexnode[0:2],
                                      hexnode[2:4])
            entrysubpath = os.path.join(dirsubpath, hexnode)

            try:
                os.makedirs(os.path.join(base, dirsubpath))
            except OSError:
                pass
            fh = util.atomictempfile(
                    os.path.join(base, entrysubpath),
                    mode="w+")
            try:
                fh.write(text)
            finally:
                fh.close()
        except Exception:
            # it's a cache.  suppress the exception, disable caching
            # going forward, and then report if logging is enabled.
            if loggingenabled:
                sys.stderr.write("Encountered exception in extension "
                                 "manifestdiskcache: {0}\n".format(
                                     traceback.format_exc()))

    def _prune_cache(self):
        # spawn a subprocess (but don't wait for it) to prune the cache.  this
        # may result in us (the main process) becoming a zombie, because we
        # could finish execution before the subprocess finishes.  if this
        # becomes an issue, we can have the spawned subprocess execute the
        # double-fork daemonization.
        cmd = util.hgcmd()[:]
        cmd.extend(["--repository",
                    self.repo_root,
                    "prunemanifestdiskcache"])
        subprocess.Popen(cmd, close_fds=True)

@replaceclass(localrepo, 'localrepository')
class repowithmdc(localrepo.localrepository):
    def _applyopenerreqs(self):
        super(repowithmdc, self)._applyopenerreqs()
        self.svfs.options[CONFIG_KEY] = self.ui.configbool(
            CONFIG_KEY, 'enabled', False)
        self.svfs.options[REPO_ROOT_KEY] = self.root

def _reposnames(ui):
    # '' is local repo. This also defines an order precedence for master.
    repos = ui.configlist(CONFIG_KEY, 'repos', ['', 'remote/', 'default/'])
    names = ui.configlist(CONFIG_KEY, 'names', ['@', 'master', 'stable'])

    for repo in repos:
        for name in names:
            yield repo + name

def _masterrevset(ui, repo):
    """
    Try to find the name of ``master`` -- usually a bookmark.

    Defaults to 'tip' if no suitable local or remote bookmark is found.
    """

    masterstring = ui.config(CONFIG_KEY, 'master')
    if masterstring:
        return masterstring

    names = set(bookmarks.bmstore(repo).keys())
    if util.safehasattr(repo, 'names') and 'remotebookmarks' in repo.names:
        names.update(set(repo.names['remotebookmarks'].listnames(repo)))

    for name in _reposnames(ui):
        if name in names:
            return name

    return 'tip'
