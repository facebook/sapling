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
    # size of a file to make it use LFS
    threshold = 10M
    # chunk large files into small blobs client-side. note: this feature is
    # an extension, not part of the standard Git-LFS specification. if this is
    # not set, large files will not be chunked.
    chunksize = 10M

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
    exchange,
    extensions,
    filelog,
    revlog,
    vfs as vfsmod,
)

from . import (
    blobstore,
    wrapper,
)

def reposetup(ui, repo):
    # Nothing to do with a remote repo
    if not repo.local():
        return

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

def wrapfilelog(filelog):
    wrapfunction = extensions.wrapfunction

    wrapfunction(filelog, 'addrevision', wrapper.filelogaddrevision)
    wrapfunction(filelog, 'size', wrapper.filelogsize)

def extsetup(ui):
    wrapfilelog(filelog.filelog)

    wrapfunction = extensions.wrapfunction
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

    # Make bundle choose changegroup3 instead of changegroup2. This affects
    # "hg bundle" command. Note: it does not cover all bundle formats like
    # "packed1". Using "packed1" with lfs will likely cause trouble.
    names = [k for k, v in exchange._bundlespeccgversions.items() if v == '02']
    for k in names:
        exchange._bundlespeccgversions[k] = '03'

    # bundlerepo uses "vfsmod.readonlyvfs(othervfs)", we need to make sure lfs
    # options and blob stores are passed from othervfs to the new readonlyvfs.
    wrapfunction(vfsmod.readonlyvfs, '__init__', wrapper.vfsinit)
