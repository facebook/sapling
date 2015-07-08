# extension to emulate invoking 'dirstate.write()' at the time
# specified by '[fakedirstatewritetime] fakenow', only when
# 'dirstate.write()' is invoked via functions below:
#
#   - 'workingctx._checklookup()' (= 'repo.status()')
#   - 'committablectx.markcommitted()'

from mercurial import context, extensions, parsers, util

def pack_dirstate(fakenow, orig, dmap, copymap, pl, now):
    # execute what original parsers.pack_dirstate should do actually
    # for consistency
    actualnow = int(now)
    for f, e in dmap.iteritems():
        if e[0] == 'n' and e[3] == actualnow:
            e = parsers.dirstatetuple(e[0], e[1], e[2], -1)
            dmap[f] = e

    return orig(dmap, copymap, pl, fakenow)

def fakewrite(ui, func):
    # fake "now" of 'pack_dirstate' only if it is invoked while 'func'

    fakenow = ui.config('fakedirstatewritetime', 'fakenow')
    if not fakenow:
        # Execute original one, if fakenow isn't configured. This is
        # useful to prevent subrepos from executing replaced one,
        # because replacing 'parsers.pack_dirstate' is also effective
        # in subrepos.
        return func()

    # parsing 'fakenow' in YYYYmmddHHMM format makes comparison between
    # 'fakenow' value and 'touch -t YYYYmmddHHMM' argument easy
    timestamp = util.parsedate(fakenow, ['%Y%m%d%H%M'])[0]
    fakenow = float(timestamp)

    orig_pack_dirstate = parsers.pack_dirstate
    wrapper = lambda *args: pack_dirstate(fakenow, orig_pack_dirstate, *args)

    parsers.pack_dirstate = wrapper
    try:
        return func()
    finally:
        parsers.pack_dirstate = orig_pack_dirstate

def _checklookup(orig, workingctx, files):
    ui = workingctx.repo().ui
    return fakewrite(ui, lambda : orig(workingctx, files))

def markcommitted(orig, committablectx, node):
    ui = committablectx.repo().ui
    return fakewrite(ui, lambda : orig(committablectx, node))

def extsetup(ui):
    extensions.wrapfunction(context.workingctx, '_checklookup',
                            _checklookup)
    extensions.wrapfunction(context.committablectx, 'markcommitted',
                            markcommitted)
