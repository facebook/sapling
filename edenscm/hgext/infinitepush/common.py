# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os
import tempfile

from edenscm.mercurial import (
    encoding,
    error,
    extensions,
    node as nodemod,
    pushkey,
    util,
    wireproto,
)
from edenscm.mercurial.commands import debug as debugcommands

from . import bundleparts


def isserver(ui):
    return ui.configbool("infinitepush", "server")


def reposetup(ui, repo):
    repo._scratchbranchmatcher = scratchbranchmatcher(ui)


def extsetup(ui):
    wireproto.commands["listkeyspatterns"] = (
        wireprotolistkeyspatterns,
        "namespace patterns",
    )
    wireproto.commands["knownnodes"] = (wireprotoknownnodes, "nodes *")
    extensions.wrapfunction(
        debugcommands, "_debugbundle2part", bundleparts.debugbundle2part
    )


def wireprotolistkeyspatterns(repo, proto, namespace, patterns):
    patterns = wireproto.decodelist(patterns)
    d = repo.listkeys(encoding.tolocal(namespace), patterns).iteritems()
    return pushkey.encodekeys(d)


def wireprotoknownnodes(repo, proto, nodes, others):
    """similar to 'known' but also check in infinitepush storage"""
    nodes = wireproto.decodelist(nodes)
    knownlocally = repo.known(nodes)
    for index, known in enumerate(knownlocally):
        # TODO: make a single query to the bundlestore.index
        if not known and repo.bundlestore.index.getnodebyprefix(
            nodemod.hex(nodes[index])
        ):
            knownlocally[index] = True
    return "".join(b and "1" or "0" for b in knownlocally)


def downloadbundle(repo, unknownbinhead):
    index = repo.bundlestore.index
    store = repo.bundlestore.store
    bundleid = index.getbundle(nodemod.hex(unknownbinhead))
    if bundleid is None:
        raise error.Abort("%s head is not known" % nodemod.hex(unknownbinhead))
    data = store.read(bundleid)
    fp = None
    fd, bundlefile = tempfile.mkstemp()
    try:  # guards bundlefile
        try:  # guards fp
            fp = os.fdopen(fd, "wb")
            fp.write(data)
        finally:
            fp.close()
    except Exception:
        try:
            os.unlink(bundlefile)
        except Exception:
            # we would rather see the original exception
            pass
        raise

    return bundlefile


class scratchbranchmatcher(object):
    def __init__(self, ui):
        scratchbranchpat = ui.config("infinitepush", "branchpattern")
        if scratchbranchpat:
            _, _, matchfn = util.stringmatcher(scratchbranchpat)
        else:
            matchfn = lambda x: False
        self._matchfn = matchfn

    def match(self, bookmark):
        return self._matchfn(bookmark)
