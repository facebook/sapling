# coding=UTF-8
"""lfs - large file support

Configs::

    [lfs]
    # remote endpoint
    remoteurl = https://example.com/lfs
    # user for HTTP auth
    remoteuser = user
    # password for HTTP auth
    remotepassword = password
    # blobstore type. "git-lfs", or "dummy" (test-only)
    remotestore = git-lfs
    # local filesystem path (only used by the dummy blobstore, test-only)
    remotepath = /tmp/test
    # location of the blob storage
    blobstore = cache/localblobstore

    # When bypass is set to True, lfs will bypass downloading or uploading
    # blobs, and only skip some hash checks. For example, "hg cat FILE" will
    # display the internal lfs metadata instead of the large file content.
    # Set this to true if the repo is only used for serving (i.e. its working
    # directory parent is always null)
    bypass = false
"""

from __future__ import absolute_import

from mercurial import (
    changegroup,
    extensions,
    filelog,
    revlog,
)

from . import (
    blobstore,
    wrapper,
)

def reposetup(ui, repo):
    bypass = repo.ui.configbool('lfs', 'bypass', False)
    # Some code (without repo access) needs to test "bypass". They can only
    # access repo.svfs as self.opener.
    repo.svfs.options['lfsbypass'] = bypass
    if bypass:
        # Do not setup blobstores if bypass is True
        return

    threshold = repo.ui.configbytes('lfs', 'threshold', None)
    chunksize = repo.ui.configbytes('lfs', 'chunksize', None)

    repo.svfs.options['lfsthreshold'] = threshold
    repo.svfs.options['lfschunksize'] = chunksize
    repo.svfs.lfslocalblobstore = blobstore.local(repo)
    repo.svfs.lfsremoteblobstore = blobstore.remote(repo)

    # Push hook
    repo.prepushoutgoinghooks.add('lfs', wrapper.prepush)

def extsetup(ui):
    wrapfunction = extensions.wrapfunction

    wrapfunction(filelog.filelog, 'add', wrapper.filelogadd)
    wrapfunction(filelog.filelog, 'cmp', wrapper.filelogcmp)
    wrapfunction(filelog.filelog, 'read', wrapper.filelogread)
    wrapfunction(filelog.filelog, 'renamed', wrapper.filelogrenamed)
    wrapfunction(changegroup,
                 'supportedoutgoingversions',
                 wrapper.supportedoutgoingversions)
    wrapfunction(changegroup,
                 'allsupportedversions',
                 wrapper.allsupportedversions)

    revlog.addflagprocessor(
        revlog.REVIDX_EXTSTORED,
        (
            wrapper.readfromstore,
            wrapper.writetostore,
            wrapper.bypasscheckhash,
        ),
    )
