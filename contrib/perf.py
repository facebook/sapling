# perf.py - performance test routines
"""helper extension to measure performance"""

# "historical portability" policy of perf.py:
#
# We have to do:
# - make perf.py "loadable" with as wide Mercurial version as possible
#   This doesn't mean that perf commands work correctly with that Mercurial.
#   BTW, perf.py itself has been available since 1.1 (or eb240755386d).
# - make historical perf command work correctly with as wide Mercurial
#   version as possible
#
# We have to do, if possible with reasonable cost:
# - make recent perf command for historical feature work correctly
#   with early Mercurial
#
# We don't have to do:
# - make perf command for recent feature work correctly with early
#   Mercurial

from __future__ import absolute_import

import functools
import gc
import os
import random
import struct
import sys
import time

from edenscm.mercurial import (
    changegroup,
    changelog,
    cmdutil,
    commands,
    copies,
    error,
    extensions,
    manifest,
    mdiff,
    merge,
    revlog,
    util,
)


# for "historical portability":
# try to import modules separately (in dict order), and ignore
# failure, because these aren't available with early Mercurial
try:
    from edenscm.mercurial import branchmap  # since 2.5 (or bcee63733aad)
except ImportError:
    pass
try:
    from edenscm.mercurial import obsolete  # since 2.3 (or ad0d6c2b3279)
except ImportError:
    pass
try:
    from edenscm.mercurial import registrar  # since 3.7 (or 37d50250b696)

    dir(registrar)  # forcibly load it
except ImportError:
    registrar = None
try:
    from edenscm.mercurial import repoview  # since 2.5 (or 3a6ddacb7198)
except ImportError:
    pass
try:
    from edenscm.mercurial import scmutil  # since 1.9 (or 8b252e826c68)
except ImportError:
    pass

if sys.version_info.major >= 3:
    xrange = range

# for "historical portability":
# define util.safehasattr forcibly, because util.safehasattr has been
# available since 1.9.3 (or 94b200a11cf7)
_undefined = object()


def safehasattr(thing, attr):
    return getattr(thing, attr, _undefined) is not _undefined


setattr(util, "safehasattr", safehasattr)

# for "historical portability":
# define util.timer forcibly, because util.timer has been available
# since ae5d60bb70c9
if safehasattr(time, "perf_counter"):
    util.timer = time.perf_counter
elif os.name == "nt":
    util.timer = time.clock
else:
    util.timer = time.time

# for "historical portability":
# use locally defined empty option list, if formatteropts isn't
# available, because commands.formatteropts has been available since
# 3.2 (or 7a7eed5176a4), even though formatting itself has been
# available since 2.2 (or ae5f92e154d3)
formatteropts = getattr(
    cmdutil, "formatteropts", getattr(commands, "formatteropts", [])
)

# for "historical portability":
# use locally defined option list, if debugrevlogopts isn't available,
# because commands.debugrevlogopts has been available since 3.7 (or
# 5606f7d0d063), even though cmdutil.openrevlog() has been available
# since 1.9 (or a79fea6b3e77).
revlogopts = getattr(
    cmdutil,
    "debugrevlogopts",
    getattr(
        commands,
        "debugrevlogopts",
        [
            ("c", "changelog", False, ("open changelog")),
            ("m", "manifest", False, ("open manifest")),
            ("", "dir", False, ("open directory manifest")),
        ],
    ),
)

cmdtable = {}

# for "historical portability":
# define parsealiases locally, because cmdutil.parsealiases has been
# available since 1.5 (or 6252852b4332)
def parsealiases(cmd):
    return cmd.lstrip("^").split("|")


if safehasattr(registrar, "command"):
    command = registrar.command(cmdtable)
elif safehasattr(cmdutil, "command"):
    import inspect

    command = cmdutil.command(cmdtable)
    if "norepo" not in inspect.getargspec(command)[0]:
        # for "historical portability":
        # wrap original cmdutil.command, because "norepo" option has
        # been available since 3.1 (or 75a96326cecb)
        _command = command

        def command(name, options=(), synopsis=None, norepo=False):
            if norepo:
                commands.norepo += " %s" % " ".join(parsealiases(name))
            return _command(name, list(options), synopsis)


else:
    # for "historical portability":
    # define "@command" annotation locally, because cmdutil.command
    # has been available since 1.9 (or 2daa5179e73f)
    def command(name, options=(), synopsis=None, norepo=False):
        def decorator(func):
            if synopsis:
                cmdtable[name] = func, list(options), synopsis
            else:
                cmdtable[name] = func, list(options)
            if norepo:
                commands.norepo += " %s" % " ".join(parsealiases(name))
            return func

        return decorator


try:
    import edemscm.mercurial.registrar
    import edemscm.mercurial.configitems
    import edenscm.mercurial as mercurial

    configtable = {}
    configitem = mercurial.registrar.configitem(configtable)
    configitem("perf", "presleep", default=mercurial.configitems.dynamicdefault)
    configitem("perf", "stub", default=mercurial.configitems.dynamicdefault)
    configitem("perf", "parentscount", default=mercurial.configitems.dynamicdefault)
except (ImportError, AttributeError):
    pass


def getlen(ui):
    if ui.configbool("perf", "stub", False):
        return lambda x: 1
    return len


def gettimer(ui, opts=None):
    """return a timer function and formatter: (timer, formatter)

    This function exists to gather the creation of formatter in a single
    place instead of duplicating it in all performance commands."""

    # enforce an idle period before execution to counteract power management
    # experimental config: perf.presleep
    time.sleep(getint(ui, "perf", "presleep", 1))

    if opts is None:
        opts = {}
    # redirect all to stderr unless buffer api is in use
    if not ui._buffers:
        ui = ui.copy()
        uifout = safeattrsetter(ui, "fout", ignoremissing=True)
        if uifout:
            # for "historical portability":
            # ui.fout/ferr have been available since 1.9 (or 4e1ccd4c2b6d)
            uifout.set(ui.ferr)

    # get a formatter
    uiformatter = getattr(ui, "formatter", None)
    if uiformatter:
        fm = uiformatter("perf", opts)
    else:
        # for "historical portability":
        # define formatter locally, because ui.formatter has been
        # available since 2.2 (or ae5f92e154d3)
        from edenscm.mercurial import node

        class defaultformatter(object):
            """Minimized composition of baseformatter and plainformatter
            """

            def __init__(self, ui, topic, opts):
                self._ui = ui
                if ui.debugflag:
                    self.hexfunc = node.hex
                else:
                    self.hexfunc = node.short

            def __nonzero__(self):
                return False

            __bool__ = __nonzero__

            def startitem(self):
                pass

            def data(self, **data):
                pass

            def write(self, fields, deftext, *fielddata, **opts):
                self._ui.write(deftext % fielddata, **opts)

            def condwrite(self, cond, fields, deftext, *fielddata, **opts):
                if cond:
                    self._ui.write(deftext % fielddata, **opts)

            def plain(self, text, **opts):
                self._ui.write(text, **opts)

            def end(self):
                pass

        fm = defaultformatter(ui, "perf", opts)

    # stub function, runs code only once instead of in a loop
    # experimental config: perf.stub
    if ui.configbool("perf", "stub", False):
        return functools.partial(stub_timer, fm), fm
    return functools.partial(_timer, fm), fm


def stub_timer(fm, func, title=None):
    func()


def _timer(fm, func, title=None):
    gc.collect()
    results = []
    begin = util.timer()
    count = 0
    while True:
        ostart = os.times()
        cstart = util.timer()
        r = func()
        cstop = util.timer()
        ostop = os.times()
        count += 1
        a, b = ostart, ostop
        results.append((cstop - cstart, b[0] - a[0], b[1] - a[1]))
        if cstop - begin > 3 and count >= 100:
            break
        if cstop - begin > 10 and count >= 3:
            break

    fm.startitem()

    if title:
        fm.write("title", "! %s\n", title)
    if r:
        fm.write("result", "! result: %s\n", r)
    m = min(results)
    fm.plain("!")
    fm.write("wall", " wall %f", m[0])
    fm.write("comb", " comb %f", m[1] + m[2])
    fm.write("user", " user %f", m[1])
    fm.write("sys", " sys %f", m[2])
    fm.write("count", " (best of %d)", count)
    fm.plain("\n")


# utilities for historical portability


def getint(ui, section, name, default):
    # for "historical portability":
    # ui.configint has been available since 1.9 (or fa2b596db182)
    v = ui.config(section, name, None)
    if v is None:
        return default
    try:
        return int(v)
    except ValueError:
        raise error.ConfigError(("%s.%s is not an integer ('%s')") % (section, name, v))


def safeattrsetter(obj, name, ignoremissing=False):
    """Ensure that 'obj' has 'name' attribute before subsequent setattr

    This function is aborted, if 'obj' doesn't have 'name' attribute
    at runtime. This avoids overlooking removal of an attribute, which
    breaks assumption of performance measurement, in the future.

    This function returns the object to (1) assign a new value, and
    (2) restore an original value to the attribute.

    If 'ignoremissing' is true, missing 'name' attribute doesn't cause
    abortion, and this function returns None. This is useful to
    examine an attribute, which isn't ensured in all Mercurial
    versions.
    """
    if not util.safehasattr(obj, name):
        if ignoremissing:
            return None
        raise error.Abort(
            (
                "missing attribute %s of %s might break assumption"
                " of performance measurement"
            )
            % (name, obj)
        )

    origvalue = getattr(obj, name)

    class attrutil(object):
        def set(self, newvalue):
            setattr(obj, name, newvalue)

        def restore(self):
            setattr(obj, name, origvalue)

    return attrutil()


# utilities to examine each internal API changes


def getsvfs(repo):
    """Return appropriate object to access files under .hg/store
    """
    # for "historical portability":
    # repo.svfs has been available since 2.3 (or 7034365089bf)
    svfs = getattr(repo, "svfs", None)
    if svfs:
        return svfs
    else:
        return getattr(repo, "sopener")


def getvfs(repo):
    """Return appropriate object to access files under .hg
    """
    # for "historical portability":
    # repo.vfs has been available since 2.3 (or 7034365089bf)
    vfs = getattr(repo, "vfs", None)
    if vfs:
        return vfs
    else:
        return getattr(repo, "opener")


def repocleartagscachefunc(repo):
    """Return the function to clear tags cache according to repo internal API
    """
    if util.safehasattr(repo, "_tagscache"):  # since 2.0 (or 9dca7653b525)
        # in this case, setattr(repo, '_tagscache', None) or so isn't
        # correct way to clear tags cache, because existing code paths
        # expect _tagscache to be a structured object.
        def clearcache():
            # _tagscache has been filteredpropertycache since 2.5 (or
            # 98c867ac1330), and delattr() can't work in such case
            if "_tagscache" in vars(repo):
                del repo.__dict__["_tagscache"]

        return clearcache

    repotags = safeattrsetter(repo, "_tags", ignoremissing=True)
    if repotags:  # since 1.4 (or 5614a628d173)
        return lambda: repotags.set(None)

    repotagscache = safeattrsetter(repo, "tagscache", ignoremissing=True)
    if repotagscache:  # since 0.6 (or d7df759d0e97)
        return lambda: repotagscache.set(None)

    # Mercurial earlier than 0.6 (or d7df759d0e97) logically reaches
    # this point, but it isn't so problematic, because:
    # - repo.tags of such Mercurial isn't "callable", and repo.tags()
    #   in perftags() causes failure soon
    # - perf.py itself has been available since 1.1 (or eb240755386d)
    raise error.Abort(("tags API of this hg command is unknown"))


# utilities to clear cache


def clearfilecache(repo, attrname):
    unfi = repo.unfiltered()
    if attrname in vars(unfi):
        delattr(unfi, attrname)
    unfi._filecache.pop(attrname, None)


# perf commands


@command("perfwalk", formatteropts)
def perfwalk(ui, repo, *pats, **opts):
    timer, fm = gettimer(ui, opts)
    m = scmutil.match(repo[None], pats, {})
    timer(lambda: len(list(repo.dirstate.walk(m, unknown=True, ignored=False))))
    fm.end()


@command("perfannotate", formatteropts)
def perfannotate(ui, repo, f, **opts):
    timer, fm = gettimer(ui, opts)
    fc = repo["."][f]
    timer(lambda: len(fc.annotate(True)))
    fm.end()


@command("perfdatapack", formatteropts)
def perfdatapack(ui, repo, packpath, **opts):
    from edenscm.hgext.remotefilelog.datapack import datapack

    keys = list(iter(datapack(packpath)))
    ui.write(("\nGetMissing (Key Count: %s)\n") % len(keys))
    _packtestfn(ui, packpath, opts, lambda pack: pack.getmissing(keys))

    partkeys = keys[:100]
    ui.write(("\nGetMissing (Key Count: %s)\n") % len(partkeys))
    _packtestfn(ui, packpath, opts, lambda pack: pack.getmissing(partkeys))

    key = keys[0]
    ui.write(("\nGet\n"))

    def f(pack):
        pack.getdelta(*key)

    _packtestfn(ui, packpath, opts, f)

    ui.write(("\nMark Ledger (Key Count: %s)\n") % len(keys))
    from edenscm.hgext.remotefilelog.repack import repackledger

    def f(pack):
        ledger = repackledger()
        pack.markledger(ledger, None)

    _packtestfn(ui, packpath, opts, f)


def _packtestfn(ui, packpath, opts, func):
    from edenscm.hgext.remotefilelog.datapack import datapack, fastdatapack
    from bindings import revisionstore

    kinds = [
        ("Python", datapack),
        ("C", fastdatapack),
        ("Rust", revisionstore.datapack),
    ]

    prepacks = [(name, f(packpath)) for name, f in kinds]

    for name, pack in prepacks:
        ui.write("%s\n" % name)
        timer, fm = gettimer(ui, opts)
        timer(lambda: func(pack))
        fm.end()


@command(
    "perfstatus",
    [("u", "unknown", False, "ask status to look for unknown files")] + formatteropts,
)
def perfstatus(ui, repo, **opts):
    # m = match.always(repo.root, repo.getcwd())
    # timer(lambda: sum(map(len, repo.dirstate.status(m, [], False, False,
    #                                                False))))
    timer, fm = gettimer(ui, opts)
    timer(lambda: sum(map(len, repo.status(unknown=opts["unknown"]))))
    fm.end()


@command("perfaddremove", formatteropts)
def perfaddremove(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    try:
        oldquiet = repo.ui.quiet
        repo.ui.quiet = True
        matcher = scmutil.match(repo[None])
        timer(lambda: scmutil.addremove(repo, matcher, "", dry_run=True))
    finally:
        repo.ui.quiet = oldquiet
        fm.end()


def clearcaches(cl):
    # behave somewhat consistently across internal API changes
    if util.safehasattr(cl, "clearcaches"):
        cl.clearcaches()
    elif util.safehasattr(cl, "_nodecache"):
        from edenscm.mercurial.node import nullid, nullrev

        cl._nodecache = {nullid: nullrev}
        cl._nodepos = None


@command("perfheads", formatteropts)
def perfheads(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    cl = repo.changelog

    def d():
        len(cl.headrevs())
        clearcaches(cl)

    timer(d)
    fm.end()


@command("perftags", formatteropts)
def perftags(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    svfs = getsvfs(repo)
    repocleartagscache = repocleartagscachefunc(repo)

    def t():
        repo.changelog = changelog.changelog(svfs, uiconfig=ui.uiconfig())
        repo.manifestlog = manifest.manifestlog(svfs, repo)
        repocleartagscache()
        return len(repo.tags())

    timer(t)
    fm.end()


@command("perfancestors", formatteropts)
def perfancestors(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    heads = repo.changelog.headrevs()

    def d():
        for a in repo.changelog.ancestors(heads):
            pass

    timer(d)
    fm.end()


@command("perfancestorset", formatteropts)
def perfancestorset(ui, repo, revset, **opts):
    timer, fm = gettimer(ui, opts)
    revs = repo.revs(revset)
    heads = repo.changelog.headrevs()

    def d():
        s = repo.changelog.ancestors(heads)
        for rev in revs:
            rev in s

    timer(d)
    fm.end()


@command("perfbookmarks", formatteropts)
def perfbookmarks(ui, repo, **opts):
    """benchmark parsing bookmarks from disk to memory"""
    timer, fm = gettimer(ui, opts)

    def d():
        clearfilecache(repo, "_bookmarks")
        repo._bookmarks

    timer(d)
    fm.end()


@command("perfbundleread", formatteropts, "BUNDLE")
def perfbundleread(ui, repo, bundlepath, **opts):
    """Benchmark reading of bundle files.

    This command is meant to isolate the I/O part of bundle reading as
    much as possible.
    """
    from edenscm.mercurial import bundle2, exchange, streamclone

    def makebench(fn):
        def run():
            with open(bundlepath, "rb") as fh:
                bundle = exchange.readbundle(ui, fh, bundlepath)
                fn(bundle)

        return run

    def makereadnbytes(size):
        def run():
            with open(bundlepath, "rb") as fh:
                bundle = exchange.readbundle(ui, fh, bundlepath)
                while bundle.read(size):
                    pass

        return run

    def makestdioread(size):
        def run():
            with open(bundlepath, "rb") as fh:
                while fh.read(size):
                    pass

        return run

    # bundle1

    def deltaiter(bundle):
        for delta in bundle.deltaiter():
            pass

    def iterchunks(bundle):
        for chunk in bundle.getchunks():
            pass

    # bundle2

    def forwardchunks(bundle):
        for chunk in bundle._forwardchunks():
            pass

    def iterparts(bundle):
        for part in bundle.iterparts():
            pass

    def iterpartsseekable(bundle):
        for part in bundle.iterparts(seekable=True):
            pass

    def seek(bundle):
        for part in bundle.iterparts(seekable=True):
            part.seek(0, os.SEEK_END)

    def makepartreadnbytes(size):
        def run():
            with open(bundlepath, "rb") as fh:
                bundle = exchange.readbundle(ui, fh, bundlepath)
                for part in bundle.iterparts():
                    while part.read(size):
                        pass

        return run

    benches = [
        (makestdioread(8192), "read(8k)"),
        (makestdioread(16384), "read(16k)"),
        (makestdioread(32768), "read(32k)"),
        (makestdioread(131072), "read(128k)"),
    ]

    with open(bundlepath, "rb") as fh:
        bundle = exchange.readbundle(ui, fh, bundlepath)

        if isinstance(bundle, changegroup.cg1unpacker):
            benches.extend(
                [
                    (makebench(deltaiter), "cg1 deltaiter()"),
                    (makebench(iterchunks), "cg1 getchunks()"),
                    (makereadnbytes(8192), "cg1 read(8k)"),
                    (makereadnbytes(16384), "cg1 read(16k)"),
                    (makereadnbytes(32768), "cg1 read(32k)"),
                    (makereadnbytes(131072), "cg1 read(128k)"),
                ]
            )
        elif isinstance(bundle, bundle2.unbundle20):
            benches.extend(
                [
                    (makebench(forwardchunks), "bundle2 forwardchunks()"),
                    (makebench(iterparts), "bundle2 iterparts()"),
                    (makebench(iterpartsseekable), "bundle2 iterparts() seekable"),
                    (makebench(seek), "bundle2 part seek()"),
                    (makepartreadnbytes(8192), "bundle2 part read(8k)"),
                    (makepartreadnbytes(16384), "bundle2 part read(16k)"),
                    (makepartreadnbytes(32768), "bundle2 part read(32k)"),
                    (makepartreadnbytes(131072), "bundle2 part read(128k)"),
                ]
            )
        elif isinstance(bundle, streamclone.streamcloneapplier):
            raise error.Abort("stream clone bundles not supported")
        else:
            raise error.Abort("unhandled bundle type: %s" % type(bundle))

    for fn, title in benches:
        timer, fm = gettimer(ui, opts)
        timer(fn, title=title)
        fm.end()


@command(
    "perfchangegroupchangelog",
    formatteropts
    + [
        ("", "version", "02", "changegroup version"),
        ("r", "rev", "", "revisions to add to changegroup"),
    ],
)
def perfchangegroupchangelog(ui, repo, version="02", rev=None, **opts):
    """Benchmark producing a changelog group for a changegroup.

    This measures the time spent processing the changelog during a
    bundle operation. This occurs during `hg bundle` and on a server
    processing a `getbundle` wire protocol request (handles clones
    and pull requests).

    By default, all revisions are added to the changegroup.
    """
    cl = repo.changelog
    revs = [cl.lookup(r) for r in repo.revs(rev or "all()")]
    bundler = changegroup.getbundler(version, repo)

    def lookup(node):
        # The real bundler reads the revision in order to access the
        # manifest node and files list. Do that here.
        cl.read(node)
        return node

    def d():
        for chunk in bundler.group(revs, cl, lookup):
            pass

    timer, fm = gettimer(ui, opts)
    timer(d)
    fm.end()


@command("perfdirs", formatteropts)
def perfdirs(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    dirstate = repo.dirstate
    "a" in dirstate

    def d():
        dirstate.hasdir("a")
        del dirstate._map._dirs

    timer(d)
    fm.end()


@command("perfdirstate", formatteropts)
def perfdirstate(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    "a" in repo.dirstate

    def d():
        repo.dirstate.invalidate()
        "a" in repo.dirstate

    timer(d)
    fm.end()


@command("perfdirstatedirs", formatteropts)
def perfdirstatedirs(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    "a" in repo.dirstate

    def d():
        repo.dirstate.hasdir("a")
        del repo.dirstate._map._dirs

    timer(d)
    fm.end()


@command("perfdirstatefoldmap", formatteropts)
def perfdirstatefoldmap(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    dirstate = repo.dirstate
    "a" in dirstate

    def d():
        dirstate._map.filefoldmap.get("a")
        del dirstate._map.filefoldmap

    timer(d)
    fm.end()


@command("perfdirfoldmap", formatteropts)
def perfdirfoldmap(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    dirstate = repo.dirstate
    "a" in dirstate

    def d():
        dirstate._map.dirfoldmap.get("a")
        del dirstate._map.dirfoldmap
        del dirstate._map._dirs

    timer(d)
    fm.end()


@command("perfdirstatewrite", formatteropts)
def perfdirstatewrite(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    ds = repo.dirstate
    "a" in ds

    def d():
        ds._dirty = True
        ds.write(repo.currenttransaction())

    timer(d)
    fm.end()


@command(
    "perfmergecalculate", [("r", "rev", ".", "rev to merge against")] + formatteropts
)
def perfmergecalculate(ui, repo, rev, **opts):
    timer, fm = gettimer(ui, opts)
    wctx = repo[None]
    rctx = scmutil.revsingle(repo, rev, rev)
    ancestor = wctx.ancestor(rctx)
    # we don't want working dir files to be stat'd in the benchmark, so prime
    # that cache
    wctx.dirty()

    def d():
        # acceptremote is True because we don't want prompts in the middle of
        # our benchmark
        merge.calculateupdates(
            repo,
            wctx,
            rctx,
            [ancestor],
            False,
            False,
            acceptremote=True,
            followcopies=True,
        )

    timer(d)
    fm.end()


@command("perfpathcopies", [], "REV REV")
def perfpathcopies(ui, repo, rev1, rev2, **opts):
    timer, fm = gettimer(ui, opts)
    ctx1 = scmutil.revsingle(repo, rev1, rev1)
    ctx2 = scmutil.revsingle(repo, rev2, rev2)

    def d():
        copies.pathcopies(ctx1, ctx2)

    timer(d)
    fm.end()


@command("perfphases", [("", "full", False, "include file reading time too")], "")
def perfphases(ui, repo, **opts):
    """benchmark phasesets computation"""
    timer, fm = gettimer(ui, opts)
    _phases = repo._phasecache
    full = opts.get("full")

    def d():
        phases = _phases
        if full:
            clearfilecache(repo, "_phasecache")
            phases = repo._phasecache
        phases.invalidate()
        phases.loadphaserevs(repo)

    timer(d)
    fm.end()


@command("perfmanifest", [], "REV")
def perfmanifest(ui, repo, rev, **opts):
    timer, fm = gettimer(ui, opts)
    ctx = scmutil.revsingle(repo, rev, rev)
    t = ctx.manifestnode()

    def d():
        repo.manifestlog.clearcaches()
        repo.manifestlog[t].read()

    timer(d)
    fm.end()


@command("perfchangeset", formatteropts)
def perfchangeset(ui, repo, rev, **opts):
    timer, fm = gettimer(ui, opts)
    n = repo[rev].node()

    def d():
        repo.changelog.read(n)
        # repo.changelog._cache = None

    timer(d)
    fm.end()


@command("perfindex", formatteropts)
def perfindex(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    revlog._prereadsize = 2 ** 24  # disable lazy parser in old hg
    n = repo["tip"].node()
    svfs = getsvfs(repo)

    def d():
        cl = revlog.revlog(svfs, "00changelog.i")
        cl.rev(n)

    timer(d)
    fm.end()


@command("perfstartup", formatteropts)
def perfstartup(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    cmd = " ".join(util.hgcmd())

    def d():
        if os.name != "nt":
            os.system("HGRCPATH= %s version -q > /dev/null" % cmd)
        else:
            os.environ["HGRCPATH"] = " "
            os.system("%s version -q > NUL" % cmd)

    timer(d)
    fm.end()


@command("perfparents", formatteropts)
def perfparents(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    # control the number of commits perfparents iterates over
    # experimental config: perf.parentscount
    count = getint(ui, "perf", "parentscount", 1000)
    if len(repo.changelog) < count:
        raise error.Abort("repo needs %d commits for this test" % count)
    repo = repo.unfiltered()
    nl = [repo.changelog.node(i) for i in xrange(count)]

    def d():
        for n in nl:
            repo.changelog.parents(n)

    timer(d)
    fm.end()


@command("perfctxfiles", formatteropts)
def perfctxfiles(ui, repo, x, **opts):
    x = int(x)
    timer, fm = gettimer(ui, opts)

    def d():
        len(repo[x].files())

    timer(d)
    fm.end()


@command("perfrawfiles", formatteropts)
def perfrawfiles(ui, repo, x, **opts):
    x = int(x)
    timer, fm = gettimer(ui, opts)
    cl = repo.changelog

    def d():
        len(cl.read(x)[3])

    timer(d)
    fm.end()


@command("perflookup", formatteropts)
def perflookup(ui, repo, rev, **opts):
    timer, fm = gettimer(ui, opts)
    timer(lambda: len(repo.lookup(rev)))
    fm.end()


@command("perfrevrange", formatteropts)
def perfrevrange(ui, repo, *specs, **opts):
    timer, fm = gettimer(ui, opts)
    revrange = scmutil.revrange
    timer(lambda: len(revrange(repo, specs)))
    fm.end()


@command("perfnodelookup", formatteropts)
def perfnodelookup(ui, repo, rev, **opts):
    timer, fm = gettimer(ui, opts)
    revlog._prereadsize = 2 ** 24  # disable lazy parser in old hg
    n = repo[rev].node()
    cl = revlog.revlog(getsvfs(repo), "00changelog.i")

    def d():
        cl.rev(n)
        clearcaches(cl)

    timer(d)
    fm.end()


@command(
    "perflog", [("", "rename", False, "ask log to follow renames")] + formatteropts
)
def perflog(ui, repo, rev=None, **opts):
    if rev is None:
        rev = []
    timer, fm = gettimer(ui, opts)
    ui.pushbuffer()
    timer(
        lambda: commands.log(
            ui, repo, rev=rev, date="", user="", copies=opts.get("rename")
        )
    )
    ui.popbuffer()
    fm.end()


@command("perfmoonwalk", formatteropts)
def perfmoonwalk(ui, repo, **opts):
    """benchmark walking the changelog backwards

    This also loads the changelog data for each revision in the changelog.
    """
    timer, fm = gettimer(ui, opts)

    def moonwalk():
        for i in xrange(len(repo), -1, -1):
            ctx = repo[i]
            ctx.branch()  # read changelog data (in addition to the index)

    timer(moonwalk)
    fm.end()


@command("perftemplating", formatteropts)
def perftemplating(ui, repo, rev=None, **opts):
    if rev is None:
        rev = []
    timer, fm = gettimer(ui, opts)
    ui.pushbuffer()
    timer(
        lambda: commands.log(
            ui,
            repo,
            rev=rev,
            date="",
            user="",
            template="{date|shortdate} [{rev}:{node|short}]"
            " {author|person}: {desc|firstline}\n",
        )
    )
    ui.popbuffer()
    fm.end()


@command("perfcca", formatteropts)
def perfcca(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    timer(lambda: scmutil.casecollisionauditor(ui, False, repo.dirstate))
    fm.end()


@command("perffncacheload", formatteropts)
def perffncacheload(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    s = repo.store

    def d():
        s.fncache._load()

    timer(d)
    fm.end()


@command("perffncachewrite", formatteropts)
def perffncachewrite(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    s = repo.store
    s.fncache._load()
    lock = repo.lock()
    tr = repo.transaction("perffncachewrite")

    def d():
        s.fncache._dirty = True
        s.fncache.write(tr)

    timer(d)
    tr.close()
    lock.release()
    fm.end()


@command("perffncacheencode", formatteropts)
def perffncacheencode(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    s = repo.store
    s.fncache._load()

    def d():
        for p in s.fncache.entries:
            s.encode(p)

    timer(d)
    fm.end()


@command(
    "perfbdiff",
    revlogopts
    + formatteropts
    + [
        ("", "count", 1, "number of revisions to test (when using --startrev)"),
        ("", "alldata", False, "test bdiffs for all associated revisions"),
    ],
    "-c|-m|FILE REV",
)
def perfbdiff(ui, repo, file_, rev=None, count=None, **opts):
    """benchmark a bdiff between revisions

    By default, benchmark a bdiff between its delta parent and itself.

    With ``--count``, benchmark bdiffs between delta parents and self for N
    revisions starting at the specified revision.

    With ``--alldata``, assume the requested revision is a changeset and
    measure bdiffs for all changes related to that changeset (manifest
    and filelogs).
    """
    if opts["alldata"]:
        opts["changelog"] = True

    if opts.get("changelog") or opts.get("manifest"):
        file_, rev = None, file_
    elif rev is None:
        raise error.CommandError("perfbdiff", "invalid arguments")

    textpairs = []

    r = cmdutil.openrevlog(repo, "perfbdiff", file_, opts)

    startrev = r.rev(r.lookup(rev))
    for rev in range(startrev, min(startrev + count, len(r) - 1)):
        if opts["alldata"]:
            # Load revisions associated with changeset.
            ctx = repo[rev]
            mtext = repo.manifestlog._revlog.revision(ctx.manifestnode())
            for pctx in ctx.parents():
                pman = repo.manifestlog._revlog.revision(pctx.manifestnode())
                textpairs.append((pman, mtext))

            # Load filelog revisions by iterating manifest delta.
            man = ctx.manifest()
            pman = ctx.p1().manifest()
            for filename, change in pman.diff(man).items():
                fctx = repo.file(filename)
                f1 = fctx.revision(change[0][0] or -1)
                f2 = fctx.revision(change[1][0] or -1)
                textpairs.append((f1, f2))
        else:
            dp = r.deltaparent(rev)
            textpairs.append((r.revision(dp), r.revision(rev)))

    def d():
        for pair in textpairs:
            mdiff.textdiff(*pair)

    timer, fm = gettimer(ui, opts)
    timer(d)
    fm.end()


@command("perfdiffwd", formatteropts)
def perfdiffwd(ui, repo, **opts):
    """Profile diff of working directory changes"""
    timer, fm = gettimer(ui, opts)
    options = {
        "w": "ignore_all_space",
        "b": "ignore_space_change",
        "B": "ignore_blank_lines",
    }

    for diffopt in ("", "w", "b", "B", "wB"):
        opts = dict((options[c], "1") for c in diffopt)

        def d():
            ui.pushbuffer()
            commands.diff(ui, repo, **opts)
            ui.popbuffer()

        title = "diffopts: %s" % (diffopt and ("-" + diffopt) or "none")
        timer(d, title)
    fm.end()


@command("perfrevlogindex", revlogopts + formatteropts, "-c|-m|FILE")
def perfrevlogindex(ui, repo, file_=None, **opts):
    """Benchmark operations against a revlog index.

    This tests constructing a revlog instance, reading index data,
    parsing index data, and performing various operations related to
    index data.
    """

    rl = cmdutil.openrevlog(repo, "perfrevlogindex", file_, opts)

    opener = getattr(rl, "opener")  # trick linter
    indexfile = rl.indexfile
    data = opener.read(indexfile)

    header = struct.unpack(">I", data[0:4])[0]
    version = header & 0xFFFF
    if version == 1:
        revlogio = revlog.revlogio()
        inline = header & (1 << 16)
    else:
        raise error.Abort(("unsupported revlog version: %d") % version)

    rllen = len(rl)

    node0 = rl.node(0)
    node25 = rl.node(rllen // 4)
    node50 = rl.node(rllen // 2)
    node75 = rl.node(rllen // 4 * 3)
    node100 = rl.node(rllen - 1)

    allrevs = range(rllen)
    allrevsrev = list(reversed(allrevs))
    allnodes = [rl.node(rev) for rev in range(rllen)]
    allnodesrev = list(reversed(allnodes))

    def constructor():
        revlog.revlog(opener, indexfile)

    def read():
        with opener(indexfile) as fh:
            fh.read()

    def parseindex():
        revlogio.parseindex(data, inline)

    def getentry(revornode):
        index = revlogio.parseindex(data, inline)[0]
        index[revornode]

    def getentries(revs, count=1):
        index = revlogio.parseindex(data, inline)[0]

        for i in range(count):
            for rev in revs:
                index[rev]

    def resolvenode(node):
        nodemap = revlogio.parseindex(data, inline)[1]
        # This only works for the C code.
        if nodemap is None:
            return

        try:
            nodemap[node]
        except error.RevlogError:
            pass

    def resolvenodes(nodes, count=1):
        nodemap = revlogio.parseindex(data, inline)[1]
        if nodemap is None:
            return

        for i in range(count):
            for node in nodes:
                try:
                    nodemap[node]
                except error.RevlogError:
                    pass

    benches = [
        (constructor, "revlog constructor"),
        (read, "read"),
        (parseindex, "create index object"),
        (lambda: getentry(0), "retrieve index entry for rev 0"),
        (lambda: resolvenode("a" * 20), "look up missing node"),
        (lambda: resolvenode(node0), "look up node at rev 0"),
        (lambda: resolvenode(node25), "look up node at 1/4 len"),
        (lambda: resolvenode(node50), "look up node at 1/2 len"),
        (lambda: resolvenode(node75), "look up node at 3/4 len"),
        (lambda: resolvenode(node100), "look up node at tip"),
        # 2x variation is to measure caching impact.
        (lambda: resolvenodes(allnodes), "look up all nodes (forward)"),
        (lambda: resolvenodes(allnodes, 2), "look up all nodes 2x (forward)"),
        (lambda: resolvenodes(allnodesrev), "look up all nodes (reverse)"),
        (lambda: resolvenodes(allnodesrev, 2), "look up all nodes 2x (reverse)"),
        (lambda: getentries(allrevs), "retrieve all index entries (forward)"),
        (lambda: getentries(allrevs, 2), "retrieve all index entries 2x (forward)"),
        (lambda: getentries(allrevsrev), "retrieve all index entries (reverse)"),
        (lambda: getentries(allrevsrev, 2), "retrieve all index entries 2x (reverse)"),
    ]

    for fn, title in benches:
        timer, fm = gettimer(ui, opts)
        timer(fn, title=title)
        fm.end()


@command(
    "perfrevlogrevisions",
    revlogopts
    + formatteropts
    + [
        ("d", "dist", 100, "distance between the revisions"),
        ("s", "startrev", 0, "revision to start reading at"),
        ("", "reverse", False, "read in reverse"),
    ],
    "-c|-m|FILE",
)
def perfrevlogrevisions(ui, repo, file_=None, startrev=0, reverse=False, **opts):
    """Benchmark reading a series of revisions from a revlog.

    By default, we read every ``-d/--dist`` revision from 0 to tip of
    the specified revlog.

    The start revision can be defined via ``-s/--startrev``.
    """
    rl = cmdutil.openrevlog(repo, "perfrevlogrevisions", file_, opts)
    rllen = getlen(ui)(rl)

    def d():
        rl.clearcaches()

        beginrev = startrev
        endrev = rllen
        dist = opts["dist"]

        if reverse:
            beginrev, endrev = endrev, beginrev
            dist = -1 * dist

        for x in xrange(beginrev, endrev, dist):
            # Old revisions don't support passing int.
            n = rl.node(x)
            rl.revision(n)

    timer, fm = gettimer(ui, opts)
    timer(d)
    fm.end()


@command(
    "perfrevlogchunks",
    revlogopts
    + formatteropts
    + [
        ("e", "engines", "", "compression engines to use"),
        ("s", "startrev", 0, "revision to start at"),
    ],
    "-c|-m|FILE",
)
def perfrevlogchunks(ui, repo, file_=None, engines=None, startrev=0, **opts):
    """Benchmark operations on revlog chunks.

    Logically, each revlog is a collection of fulltext revisions. However,
    stored within each revlog are "chunks" of possibly compressed data. This
    data needs to be read and decompressed or compressed and written.

    This command measures the time it takes to read+decompress and recompress
    chunks in a revlog. It effectively isolates I/O and compression performance.
    For measurements of higher-level operations like resolving revisions,
    see ``perfrevlogrevisions`` and ``perfrevlogrevision``.
    """
    rl = cmdutil.openrevlog(repo, "perfrevlogchunks", file_, opts)

    # _chunkraw was renamed to _getsegmentforrevs.
    try:
        segmentforrevs = rl._getsegmentforrevs
    except AttributeError:
        segmentforrevs = rl._chunkraw

    # Verify engines argument.
    if engines:
        engines = set(e.strip() for e in engines.split(","))
        for engine in engines:
            try:
                util.compressionengines[engine]
            except KeyError:
                raise error.Abort("unknown compression engine: %s" % engine)
    else:
        engines = []
        for e in util.compengines:
            engine = util.compengines[e]
            try:
                if engine.available():
                    engine.revlogcompressor().compress("dummy")
                    engines.append(e)
            except NotImplementedError:
                pass

    revs = list(rl.revs(startrev, len(rl) - 1))

    def rlfh(rl):
        if rl._inline:
            return getsvfs(repo)(rl.indexfile)
        else:
            return getsvfs(repo)(rl.datafile)

    def doread():
        rl.clearcaches()
        for rev in revs:
            segmentforrevs(rev, rev)

    def doreadcachedfh():
        rl.clearcaches()
        fh = rlfh(rl)
        for rev in revs:
            segmentforrevs(rev, rev, df=fh)

    def doreadbatch():
        rl.clearcaches()
        segmentforrevs(revs[0], revs[-1])

    def doreadbatchcachedfh():
        rl.clearcaches()
        fh = rlfh(rl)
        segmentforrevs(revs[0], revs[-1], df=fh)

    def dochunk():
        rl.clearcaches()
        fh = rlfh(rl)
        for rev in revs:
            rl._chunk(rev, df=fh)

    chunks = [None]

    def dochunkbatch():
        rl.clearcaches()
        fh = rlfh(rl)
        # Save chunks as a side-effect.
        chunks[0] = rl._chunks(revs, df=fh)

    def docompress(compressor):
        rl.clearcaches()

        try:
            # Swap in the requested compression engine.
            oldcompressor = rl._compressor
            rl._compressor = compressor
            for chunk in chunks[0]:
                rl.compress(chunk)
        finally:
            rl._compressor = oldcompressor

    benches = [
        (lambda: doread(), "read"),
        (lambda: doreadcachedfh(), "read w/ reused fd"),
        (lambda: doreadbatch(), "read batch"),
        (lambda: doreadbatchcachedfh(), "read batch w/ reused fd"),
        (lambda: dochunk(), "chunk"),
        (lambda: dochunkbatch(), "chunk batch"),
    ]

    for engine in sorted(engines):
        compressor = util.compengines[engine].revlogcompressor()
        benches.append(
            (functools.partial(docompress, compressor), "compress w/ %s" % engine)
        )

    for fn, title in benches:
        timer, fm = gettimer(ui, opts)
        timer(fn, title=title)
        fm.end()


@command(
    "perfrevlogrevision",
    revlogopts
    + formatteropts
    + [("", "cache", False, "use caches instead of clearing")],
    "-c|-m|FILE REV",
)
def perfrevlogrevision(ui, repo, file_, rev=None, cache=None, **opts):
    """Benchmark obtaining a revlog revision.

    Obtaining a revlog revision consists of roughly the following steps:

    1. Compute the delta chain
    2. Obtain the raw chunks for that delta chain
    3. Decompress each raw chunk
    4. Apply binary patches to obtain fulltext
    5. Verify hash of fulltext

    This command measures the time spent in each of these phases.
    """
    if opts.get("changelog") or opts.get("manifest"):
        file_, rev = None, file_
    elif rev is None:
        raise error.CommandError("perfrevlogrevision", "invalid arguments")

    r = cmdutil.openrevlog(repo, "perfrevlogrevision", file_, opts)

    # _chunkraw was renamed to _getsegmentforrevs.
    try:
        segmentforrevs = r._getsegmentforrevs
    except AttributeError:
        segmentforrevs = r._chunkraw

    node = r.lookup(rev)
    rev = r.rev(node)

    def getrawchunks(data, chain):
        start = r.start
        length = r.length
        inline = r._inline
        iosize = r._io.size
        buffer = util.buffer
        offset = start(chain[0])

        chunks = []
        ladd = chunks.append

        for rev in chain:
            chunkstart = start(rev)
            if inline:
                chunkstart += (rev + 1) * iosize
            chunklength = length(rev)
            ladd(buffer(data, chunkstart - offset, chunklength))

        return chunks

    def dodeltachain(rev):
        if not cache:
            r.clearcaches()
        r._deltachain(rev)

    def doread(chain):
        if not cache:
            r.clearcaches()
        segmentforrevs(chain[0], chain[-1])

    def dorawchunks(data, chain):
        if not cache:
            r.clearcaches()
        getrawchunks(data, chain)

    def dodecompress(chunks):
        decomp = r.decompress
        for chunk in chunks:
            decomp(chunk)

    def dopatch(text, bins):
        if not cache:
            r.clearcaches()
        mdiff.patches(text, bins)

    def dohash(text):
        if not cache:
            r.clearcaches()
        r.checkhash(text, node, rev=rev)

    def dorevision():
        if not cache:
            r.clearcaches()
        r.revision(node)

    chain = r._deltachain(rev)[0]
    data = segmentforrevs(chain[0], chain[-1])[1]
    rawchunks = getrawchunks(data, chain)
    bins = r._chunks(chain)
    text = str(bins[0])
    bins = bins[1:]
    text = mdiff.patches(text, bins)

    benches = [
        (lambda: dorevision(), "full"),
        (lambda: dodeltachain(rev), "deltachain"),
        (lambda: doread(chain), "read"),
        (lambda: dorawchunks(data, chain), "rawchunks"),
        (lambda: dodecompress(rawchunks), "decompress"),
        (lambda: dopatch(text, bins), "patch"),
        (lambda: dohash(text), "hash"),
    ]

    for fn, title in benches:
        timer, fm = gettimer(ui, opts)
        timer(fn, title=title)
        fm.end()


@command(
    "perfrevset",
    [
        ("C", "clear", False, "clear volatile cache between each call."),
        ("", "contexts", False, "obtain changectx for each revision"),
    ]
    + formatteropts,
    "REVSET",
)
def perfrevset(ui, repo, expr, clear=False, contexts=False, **opts):
    """benchmark the execution time of a revset

    Use the --clean option if need to evaluate the impact of build volatile
    revisions set cache on the revset execution. Volatile cache hold filtered
    and obsolete related cache."""
    timer, fm = gettimer(ui, opts)

    def d():
        if clear:
            repo.invalidatevolatilesets()
        if contexts:
            for ctx in repo.set(expr):
                pass
        else:
            for r in repo.revs(expr):
                pass

    timer(d)
    fm.end()


@command(
    "perfvolatilesets",
    [("", "clear-obsstore", False, "drop obsstore between each call.")] + formatteropts,
)
def perfvolatilesets(ui, repo, *names, **opts):
    """benchmark the computation of various volatile set

    Volatile set computes element related to filtering and obsolescence."""
    timer, fm = gettimer(ui, opts)
    repo = repo.unfiltered()

    def getobs(name):
        def d():
            repo.invalidatevolatilesets()
            if opts["clear_obsstore"]:
                clearfilecache(repo, "obsstore")
            obsolete.getrevs(repo, name)

        return d

    allobs = sorted(obsolete.cachefuncs)
    if names:
        allobs = [n for n in allobs if n in names]

    for name in allobs:
        timer(getobs(name), title=name)

    def getfiltered(name):
        def d():
            repo.invalidatevolatilesets()
            if opts["clear_obsstore"]:
                clearfilecache(repo, "obsstore")
            repoview.filterrevs(repo, name)

        return d

    allfilter = sorted(repoview.filtertable)
    if names:
        allfilter = [n for n in allfilter if n in names]

    for name in allfilter:
        timer(getfiltered(name), title=name)
    fm.end()


@command("perfloadmarkers")
def perfloadmarkers(ui, repo):
    """benchmark the time to parse the on-disk markers for a repo

    Result is the number of markers in the repo."""
    timer, fm = gettimer(ui)
    svfs = getsvfs(repo)
    timer(lambda: len(obsolete.obsstore(svfs)))
    fm.end()


@command(
    "perflrucachedict",
    formatteropts
    + [
        ("", "size", 4, "size of cache"),
        ("", "gets", 10000, "number of key lookups"),
        ("", "sets", 10000, "number of key sets"),
        ("", "mixed", 10000, "number of mixed mode operations"),
        ("", "mixedgetfreq", 50, "frequency of get vs set ops in mixed mode"),
    ],
    norepo=True,
)
def perflrucache(
    ui, size=4, gets=10000, sets=10000, mixed=10000, mixedgetfreq=50, **opts
):
    def doinit():
        for i in xrange(10000):
            util.lrucachedict(size)

    values = []
    for i in xrange(size):
        values.append(random.randint(0, sys.maxint))

    # Get mode fills the cache and tests raw lookup performance with no
    # eviction.
    getseq = []
    for i in xrange(gets):
        getseq.append(random.choice(values))

    def dogets():
        d = util.lrucachedict(size)
        for v in values:
            d[v] = v
        for key in getseq:
            value = d[key]
            value  # silence pyflakes warning

    # Set mode tests insertion speed with cache eviction.
    setseq = []
    for i in xrange(sets):
        setseq.append(random.randint(0, sys.maxint))

    def dosets():
        d = util.lrucachedict(size)
        for v in setseq:
            d[v] = v

    # Mixed mode randomly performs gets and sets with eviction.
    mixedops = []
    for i in xrange(mixed):
        r = random.randint(0, 100)
        if r < mixedgetfreq:
            op = 0
        else:
            op = 1

        mixedops.append((op, random.randint(0, size * 2)))

    def domixed():
        d = util.lrucachedict(size)

        for op, v in mixedops:
            if op == 0:
                try:
                    d[v]
                except KeyError:
                    pass
            else:
                d[v] = v

    benches = [(doinit, "init"), (dogets, "gets"), (dosets, "sets"), (domixed, "mixed")]

    for fn, title in benches:
        timer, fm = gettimer(ui, opts)
        timer(fn, title=title)
        fm.end()


@command("perfwrite", formatteropts)
def perfwrite(ui, repo, **opts):
    """microbenchmark ui.write
    """
    timer, fm = gettimer(ui, opts)

    def write():
        for i in range(100000):
            ui.write(("Testing write performance\n"))

    timer(write)
    fm.end()


def uisetup(ui):
    if util.safehasattr(cmdutil, "openrevlog") and not util.safehasattr(
        commands, "debugrevlogopts"
    ):
        # for "historical portability":
        # In this case, Mercurial should be 1.9 (or a79fea6b3e77) -
        # 3.7 (or 5606f7d0d063). Therefore, '--dir' option for
        # openrevlog() should cause failure, because it has been
        # available since 3.5 (or 49c583ca48c4).
        def openrevlog(orig, repo, cmd, file_, opts):
            if opts.get("dir") and not util.safehasattr(repo, "dirlog"):
                raise error.Abort(
                    "This version doesn't support --dir option", hint="use 3.5 or later"
                )
            return orig(repo, cmd, file_, opts)

        extensions.wrapfunction(cmdutil, "openrevlog", openrevlog)
