# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''share a common history between several working directories'''

from mercurial.i18n import _
from mercurial import cmdutil, hg, util, extensions, bookmarks
from mercurial.hg import repository, parseurl
import errno

cmdtable = {}
command = cmdutil.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'

@command('share',
    [('U', 'noupdate', None, _('do not create a working directory')),
     ('B', 'bookmarks', None, _('also share bookmarks'))],
    _('[-U] [-B] SOURCE [DEST]'),
    norepo=True)
def share(ui, source, dest=None, noupdate=False, bookmarks=False):
    """create a new shared repository

    Initialize a new repository and working directory that shares its
    history (and optionally bookmarks) with another repository.

    .. note::

       using rollback or extensions that destroy/modify history (mq,
       rebase, etc.) can cause considerable confusion with shared
       clones. In particular, if two shared clones are both updated to
       the same changeset, and one of them destroys that changeset
       with rollback, the other clone will suddenly stop working: all
       operations will fail with "abort: working directory has unknown
       parent". The only known workaround is to use debugsetparents on
       the broken clone to reset it to a changeset that still exists.
    """

    return hg.share(ui, source, dest, not noupdate, bookmarks)

@command('unshare', [], '')
def unshare(ui, repo):
    """convert a shared repository to a normal one

    Copy the store data to the repo and remove the sharedpath data.
    """

    if not repo.shared():
        raise util.Abort(_("this is not a shared repo"))

    destlock = lock = None
    lock = repo.lock()
    try:
        # we use locks here because if we race with commit, we
        # can end up with extra data in the cloned revlogs that's
        # not pointed to by changesets, thus causing verify to
        # fail

        destlock = hg.copystore(ui, repo, repo.path)

        sharefile = repo.join('sharedpath')
        util.rename(sharefile, sharefile + '.old')

        repo.requirements.discard('sharedpath')
        repo._writerequirements()
    finally:
        destlock and destlock.release()
        lock and lock.release()

    # update store, spath, sopener and sjoin of repo
    repo.unfiltered().__init__(repo.baseui, repo.root)

def extsetup(ui):
    extensions.wrapfunction(bookmarks.bmstore, 'getbkfile', getbkfile)
    extensions.wrapfunction(bookmarks.bmstore, 'recordchange', recordchange)
    extensions.wrapfunction(bookmarks.bmstore, 'write', write)

def _hassharedbookmarks(repo):
    """Returns whether this repo has shared bookmarks"""
    try:
        shared = repo.vfs.read('shared').splitlines()
    except IOError as inst:
        if inst.errno != errno.ENOENT:
            raise
        return False
    return 'bookmarks' in shared

def _getsrcrepo(repo):
    """
    Returns the source repository object for a given shared repository.
    If repo is not a shared repository, return None.
    """
    if repo.sharedpath == repo.path:
        return None

    # the sharedpath always ends in the .hg; we want the path to the repo
    source = repo.vfs.split(repo.sharedpath)[0]
    srcurl, branches = parseurl(source)
    return repository(repo.ui, srcurl)

def getbkfile(orig, self, repo):
    if _hassharedbookmarks(repo):
        srcrepo = _getsrcrepo(repo)
        if srcrepo is not None:
            repo = srcrepo
    return orig(self, repo)

def recordchange(orig, self, tr):
    # Continue with write to local bookmarks file as usual
    orig(self, tr)

    if _hassharedbookmarks(self._repo):
        srcrepo = _getsrcrepo(self._repo)
        if srcrepo is not None:
            category = 'share-bookmarks'
            tr.addpostclose(category, lambda tr: self._writerepo(srcrepo))

def write(orig, self):
    # First write local bookmarks file in case we ever unshare
    orig(self)
    if _hassharedbookmarks(self._repo):
        srcrepo = _getsrcrepo(self._repo)
        if srcrepo is not None:
            self._writerepo(srcrepo)
