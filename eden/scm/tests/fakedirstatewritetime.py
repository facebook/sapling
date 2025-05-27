# extension to emulate invoking 'dirstate.write()' at the time
# specified by '[fakedirstatewritetime] fakenow', only when
# 'dirstate.write()' is invoked via functions below:
#
#   - 'committablectx.markcommitted()'

from __future__ import absolute_import

import bindings
from sapling import context, dirstate, extensions, treestate, util

parsers = bindings.cext.parsers


def pack_dirstate(fakenow, orig, dmap, copymap, pl, now):
    # execute what original parsers.pack_dirstate should do actually
    # for consistency
    actualnow = int(now)
    for f, e in dmap.items():
        if e[0] == "n" and e[3] == actualnow:
            e = parsers.dirstatetuple(e[0], e[1], e[2], -1)
            dmap[f] = e

    return orig(dmap, copymap, pl, fakenow)


def fakewrite(ui, func):
    # fake "now" of 'pack_dirstate' only if it is invoked while 'func'

    fakenow = ui.config("fakedirstatewritetime", "fakenow")
    if not fakenow:
        # Execute original one, if fakenow isn't configured. This is
        # useful to prevent subrepos from executing replaced one,
        # because replacing 'parsers.pack_dirstate' is also effective
        # in subrepos.
        return func()

    fakenow = util.parsedate(fakenow)[0]

    orig_pack_dirstate = parsers.pack_dirstate
    orig_dirstate_getfsnow = dirstate._getfsnow
    wrapper = lambda *args: pack_dirstate(fakenow, orig_pack_dirstate, *args)

    parsers.pack_dirstate = wrapper
    dirstate._getfsnow = lambda *args: fakenow
    try:
        return func()
    finally:
        parsers.pack_dirstate = orig_pack_dirstate
        dirstate._getfsnow = orig_dirstate_getfsnow


def markcommitted(orig, committablectx, node):
    ui = committablectx.repo().ui
    return fakewrite(ui, lambda: orig(committablectx, node))


def treestatewrite(orig, self, st, now):
    ui = self._ui
    fakenow = ui.config("fakedirstatewritetime", "fakenow")
    if fakenow:
        now = util.parsedate(fakenow)[0]
    return orig(self, st, now)


def extsetup(ui):
    extensions.wrapfunction(context.committablectx, "markcommitted", markcommitted)
    extensions.wrapfunction(treestate.treestatemap, "write", treestatewrite)
