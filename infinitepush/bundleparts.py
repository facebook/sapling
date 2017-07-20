# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from .common import (
    encodebookmarks,
    isremotebooksenabled,
)
from mercurial import (
    bundle2,
    changegroup,
    error,
    extensions,
    revsetlang,
)
from mercurial.i18n import _

scratchbranchparttype = 'b2x:infinitepush'
scratchbookmarksparttype = 'b2x:infinitepushscratchbookmarks'

def getscratchbranchpart(repo, peer, outgoing, confignonforwardmove,
                         ui, bookmark, create):
    if not outgoing.missing:
        raise error.Abort(_('no commits to push'))

    if scratchbranchparttype not in bundle2.bundle2caps(peer):
        raise error.Abort(_('no server support for %r') % scratchbranchparttype)

    _validaterevset(repo, revsetlang.formatspec('%ln', outgoing.missing),
                    bookmark)

    supportedversions = changegroup.supportedoutgoingversions(repo)
    # Explicitly avoid using '01' changegroup version in infinitepush to
    # support general delta
    supportedversions.discard('01')
    cgversion = min(supportedversions)
    _handlelfs(repo, outgoing.missing)
    cg = changegroup.getlocalchangegroupraw(repo, 'push',
                                            outgoing, version=cgversion)

    params = {}
    params['cgversion'] = cgversion
    if bookmark:
        params['bookmark'] = bookmark
        # 'prevbooknode' is necessary for pushkey reply part
        params['bookprevnode'] = ''
        if bookmark in repo:
            params['bookprevnode'] = repo[bookmark].hex()
        if create:
            params['create'] = '1'
    if confignonforwardmove:
        params['force'] = '1'

    # Do not send pushback bundle2 part with bookmarks if remotenames extension
    # is enabled. It will be handled manually in `_push()`
    if not isremotebooksenabled(ui):
        params['pushbackbookmarks'] = '1'

    # .upper() marks this as a mandatory part: server will abort if there's no
    #  handler
    return bundle2.bundlepart(
        scratchbranchparttype.upper(),
        advisoryparams=params.iteritems(),
        data=cg)

def getscratchbookmarkspart(peer, bookmarks):
    if scratchbookmarksparttype not in bundle2.bundle2caps(peer):
        raise error.Abort(
            _('no server support for %r') % scratchbookmarksparttype)

    return bundle2.bundlepart(
        scratchbookmarksparttype.upper(),
        data=encodebookmarks(bookmarks))

def _validaterevset(repo, revset, bookmark):
    """Abort if the revs to be pushed aren't valid for a scratch branch."""
    if not repo.revs(revset):
        raise error.Abort(_('nothing to push'))
    if bookmark:
        # Allow bundle with many heads only if no bookmark is specified
        heads = repo.revs('heads(%r)', revset)
        if len(heads) > 1:
            raise error.Abort(
                _('cannot push more than one head to a scratch branch'))

def _handlelfs(repo, missing):
    '''Special case if lfs is enabled

    If lfs is enabled then we need to call prepush hook
    to make sure large files are uploaded to lfs
    '''
    try:
        lfsmod = extensions.find('lfs')
        lfsmod.wrapper.uploadblobsfromrevs(repo, missing)
    except KeyError:
        # Ignore if lfs extension is not enabled
        return
