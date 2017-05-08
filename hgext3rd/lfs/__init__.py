# coding=UTF-8
"""lfs - large file support

Configs::

    [lfs]
    # Remote endpoint. Multiple protocols are supported:
    # - http(s)://user:pass@example.com/path
    #   git-lfs endpoint
    # - file:///tmp/path
    #   local filesystem, usually for testing
    # if unset, lfs will prompt setting this when it must use this value.
    # (default: unset)
    url = https://example.com/lfs

    # size of a file to make it use LFS
    threshold = 10M

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
    cmdutil,
    context,
    exchange,
    extensions,
    filelog,
    revlog,
    scmutil,
    vfs as vfsmod,
)
from mercurial.i18n import _

from . import (
    blobstore,
    wrapper,
)

cmdtable = {}
command = cmdutil.command(cmdtable)

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

    repo.svfs.options['lfsthreshold'] = threshold
    repo.svfs.lfslocalblobstore = blobstore.local(repo)
    repo.svfs.lfsremoteblobstore = blobstore.remote(repo)

    # Push hook
    repo.prepushoutgoinghooks.add('lfs', wrapper.prepush)

def wrapfilelog(filelog):
    wrapfunction = extensions.wrapfunction

    wrapfunction(filelog, 'addrevision', wrapper.filelogaddrevision)
    wrapfunction(filelog, 'renamed', wrapper.filelogrenamed)
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

    wrapfunction(context.basefilectx, 'isbinary', wrapper.filectxisbinary)

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

@command('debuglfsupload',
         [('r', 'rev', [], _('upload large files introduced by REV')),
          ('o', 'oid', [], _('upload given lfs object ID'))])
def debuglfsupload(ui, repo, **opts):
    """upload lfs blobs added by the working copy parent or given revisions"""
    store = repo.svfs.lfslocalblobstore
    storeids = [store.getstoreid(o) for o in opts.get('oid', [])]
    revs = opts.get('rev')
    if revs:
        pointers = wrapper.extractpointers(repo, scmutil.revrange(repo, revs))
        storeids += [p.tostoreid() for p in pointers]
    wrapper.uploadblobs(repo, storeids)
