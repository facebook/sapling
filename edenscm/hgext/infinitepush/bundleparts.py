# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from edenscm.mercurial import (
    bundle2,
    changegroup,
    error,
    extensions,
    mutation,
    revsetlang,
    util,
)
from edenscm.mercurial.i18n import _

from .common import encodebookmarks


scratchbranchparttype = "b2x:infinitepush"
scratchbookmarksparttype = "b2x:infinitepushscratchbookmarks"
scratchmutationparttype = "b2x:infinitepushmutation"


def getscratchbranchparts(
    repo, peer, outgoing, confignonforwardmove, ui, bookmark, create, bookmarknode=None
):
    if scratchbranchparttype not in bundle2.bundle2caps(peer):
        raise error.Abort(_("no server support for %r") % scratchbranchparttype)

    _validaterevset(repo, revsetlang.formatspec("%ln", outgoing.missing), bookmark)

    supportedversions = changegroup.supportedoutgoingversions(repo)
    # Explicitly avoid using '01' changegroup version in infinitepush to
    # support general delta
    supportedversions.discard("01")
    cgversion = min(supportedversions)
    _handlelfs(repo, outgoing.missing)
    cg = changegroup.makestream(repo, outgoing, cgversion, "push")

    params = {}
    params["cgversion"] = cgversion
    if bookmark:
        params["bookmark"] = bookmark
        if bookmarknode:
            params["bookmarknode"] = bookmarknode
        if create:
            params["create"] = "1"
    if confignonforwardmove:
        params["force"] = "1"

    parts = []

    # .upper() marks this as a mandatory part: server will abort if there's no
    #  handler
    parts.append(
        bundle2.bundlepart(
            scratchbranchparttype.upper(), advisoryparams=params.iteritems(), data=cg
        )
    )

    if mutation.recording(repo):
        if scratchmutationparttype not in bundle2.bundle2caps(peer):
            repo.ui.warn(
                _("no server support for %r - skipping\n") % scratchmutationparttype
            )
        else:
            parts.append(
                bundle2.bundlepart(
                    scratchmutationparttype,
                    data=mutation.bundle(repo, outgoing.missing),
                )
            )

    try:
        treemod = extensions.find("treemanifest")
        remotefilelog = extensions.find("remotefilelog")
        sendtrees = remotefilelog.shallowbundle.cansendtrees(repo, outgoing.missing)
        if sendtrees != remotefilelog.shallowbundle.NoTrees:
            parts.append(
                treemod.createtreepackpart(
                    repo, outgoing, treemod.TREEGROUP_PARTTYPE2, sendtrees=sendtrees
                )
            )
    except KeyError:
        pass

    return parts


def getscratchbookmarkspart(peer, bookmarks):
    if scratchbookmarksparttype not in bundle2.bundle2caps(peer):
        raise error.Abort(_("no server support for %r") % scratchbookmarksparttype)

    return bundle2.bundlepart(
        scratchbookmarksparttype.upper(), data=encodebookmarks(bookmarks)
    )


def _validaterevset(repo, revset, bookmark):
    """Abort if the revs to be pushed aren't valid for a scratch branch."""
    if not bookmark and not repo.revs(revset):
        raise error.Abort(_("nothing to push"))
    if bookmark:
        # Allow bundle with many heads only if no bookmark is specified
        heads = repo.revs("heads(%r)", revset)
        if len(heads) > 1:
            raise error.Abort(_("cannot push more than one head to a scratch branch"))


def _handlelfs(repo, missing):
    """Special case if lfs is enabled

    If lfs is enabled then we need to call prepush hook
    to make sure large files are uploaded to lfs
    """
    try:
        lfsmod = extensions.find("lfs")
        lfsmod.wrapper.uploadblobsfromrevs(repo, missing)
    except KeyError:
        # Ignore if lfs extension is not enabled
        return


class copiedpart(object):
    """a copy of unbundlepart content that can be consumed later"""

    def __init__(self, part):
        # copy "public properties"
        self.type = part.type
        self.id = part.id
        self.mandatory = part.mandatory
        self.mandatoryparams = part.mandatoryparams
        self.advisoryparams = part.advisoryparams
        self.params = part.params
        self.mandatorykeys = part.mandatorykeys
        # copy the buffer
        self._io = util.stringio(part.read())

    def consume(self):
        return

    def read(self, size=None):
        if size is None:
            return self._io.read()
        else:
            return self._io.read(size)
