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
"""

from __future__ import absolute_import

from mercurial import (
    changegroup,
    extensions,
    filelog,
    revlog,
)

from . import (
    setup,
    wrapper,
)

def reposetup(ui, repo):
    setup.threshold(ui, repo)
    setup.localblobstore(ui, repo)
    setup.chunking(ui, repo)
    setup.remoteblobstore(ui, repo)

    # Push hook
    repo.prepushoutgoinghooks.add('lfs', wrapper.prepush)

def extsetup(ui):
    wrapfunction = extensions.wrapfunction

    wrapfunction(filelog.filelog, 'addrevision', wrapper.addrevision)
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
