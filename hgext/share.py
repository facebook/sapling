# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''share a common history between several working directories

Automatic Pooled Storage for Clones
-----------------------------------

When this extension is active, :hg:`clone` can be configured to
automatically share/pool storage across multiple clones. This
mode effectively converts :hg:`clone` to :hg:`clone` + :hg:`share`.
The benefit of using this mode is the automatic management of
store paths and intelligent pooling of related repositories.

The following ``share.`` config options influence this feature:

``share.pool``
    Filesystem path where shared repository data will be stored. When
    defined, :hg:`clone` will automatically use shared repository
    storage instead of creating a store inside each clone.

``share.poolnaming``
    How directory names in ``share.pool`` are constructed.

    "identity" means the name is derived from the first changeset in the
    repository. In this mode, different remotes share storage if their
    root/initial changeset is identical. In this mode, the local shared
    repository is an aggregate of all encountered remote repositories.

    "remote" means the name is derived from the source repository's
    path or URL. In this mode, storage is only shared if the path or URL
    requested in the :hg:`clone` command matches exactly to a repository
    that was cloned before.

    The default naming mode is "identity".
'''

from __future__ import absolute_import

import errno
from mercurial.i18n import _
from mercurial import (
    bookmarks,
    commands,
    error,
    extensions,
    hg,
    registrar,
    txnutil,
    util,
)

repository = hg.repository
parseurl = hg.parseurl

cmdtable = {}
command = registrar.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

@command('share',
    [('U', 'noupdate', None, _('do not create a working directory')),
     ('B', 'bookmarks', None, _('also share bookmarks')),
     ('', 'relative', None, _('point to source using a relative path '
                              '(EXPERIMENTAL)')),
    ],
    _('[-U] [-B] SOURCE [DEST]'),
    norepo=True)
def share(ui, source, dest=None, noupdate=False, bookmarks=False,
          relative=False):
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

    hg.share(ui, source, dest=dest, update=not noupdate,
             bookmarks=bookmarks, relative=relative)
    return 0

@command('unshare', [], '')
def unshare(ui, repo):
    """convert a shared repository to a normal one

    Copy the store data to the repo and remove the sharedpath data.
    """

    if not repo.shared():
        raise error.Abort(_("this is not a shared repo"))

    hg.unshare(ui, repo)

# Wrap clone command to pass auto share options.
def clone(orig, ui, source, *args, **opts):
    pool = ui.config('share', 'pool')
    if pool:
        pool = util.expandpath(pool)

    opts[r'shareopts'] = {
        'pool': pool,
        'mode': ui.config('share', 'poolnaming'),
    }

    return orig(ui, source, *args, **opts)

def extsetup(ui):
    extensions.wrapfunction(bookmarks, '_getbkfile', getbkfile)
    extensions.wrapfunction(bookmarks.bmstore, '_recordchange', recordchange)
    extensions.wrapfunction(bookmarks.bmstore, '_writerepo', writerepo)
    extensions.wrapcommand(commands.table, 'clone', clone)

def _hassharedbookmarks(repo):
    """Returns whether this repo has shared bookmarks"""
    try:
        shared = repo.vfs.read('shared').splitlines()
    except IOError as inst:
        if inst.errno != errno.ENOENT:
            raise
        return False
    return hg.sharedbookmarks in shared

def _getsrcrepo(repo):
    """
    Returns the source repository object for a given shared repository.
    If repo is not a shared repository, return None.
    """
    if repo.sharedpath == repo.path:
        return None

    if util.safehasattr(repo, 'srcrepo') and repo.srcrepo:
        return repo.srcrepo

    # the sharedpath always ends in the .hg; we want the path to the repo
    source = repo.vfs.split(repo.sharedpath)[0]
    srcurl, branches = parseurl(source)
    srcrepo = repository(repo.ui, srcurl)
    repo.srcrepo = srcrepo
    return srcrepo

def getbkfile(orig, repo):
    if _hassharedbookmarks(repo):
        srcrepo = _getsrcrepo(repo)
        if srcrepo is not None:
            # just orig(srcrepo) doesn't work as expected, because
            # HG_PENDING refers repo.root.
            try:
                fp, pending = txnutil.trypending(repo.root, repo.vfs,
                                                 'bookmarks')
                if pending:
                    # only in this case, bookmark information in repo
                    # is up-to-date.
                    return fp
                fp.close()
            except IOError as inst:
                if inst.errno != errno.ENOENT:
                    raise

            # otherwise, we should read bookmarks from srcrepo,
            # because .hg/bookmarks in srcrepo might be already
            # changed via another sharing repo
            repo = srcrepo

            # TODO: Pending changes in repo are still invisible in
            # srcrepo, because bookmarks.pending is written only into repo.
            # See also https://www.mercurial-scm.org/wiki/SharedRepository
    return orig(repo)

def recordchange(orig, self, tr):
    # Continue with write to local bookmarks file as usual
    orig(self, tr)

    if _hassharedbookmarks(self._repo):
        srcrepo = _getsrcrepo(self._repo)
        if srcrepo is not None:
            category = 'share-bookmarks'
            tr.addpostclose(category, lambda tr: self._writerepo(srcrepo))

def writerepo(orig, self, repo):
    # First write local bookmarks file in case we ever unshare
    orig(self, repo)

    if _hassharedbookmarks(self._repo):
        srcrepo = _getsrcrepo(self._repo)
        if srcrepo is not None:
            orig(self, srcrepo)
