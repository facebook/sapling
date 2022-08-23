# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

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
    import edemscm.mercurial.configitems
    import edemscm.mercurial.registrar
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
            """Minimized composition of baseformatter and plainformatter"""

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
    """Return appropriate object to access files under .hg/store"""
    # for "historical portability":
    # repo.svfs has been available since 2.3 (or 7034365089bf)
    svfs = getattr(repo, "svfs", None)
    if svfs:
        return svfs
    else:
        return getattr(repo, "sopener")


def getvfs(repo):
    """Return appropriate object to access files under .hg"""
    # for "historical portability":
    # repo.vfs has been available since 2.3 (or 7034365089bf)
    vfs = getattr(repo, "vfs", None)
    if vfs:
        return vfs
    else:
        return getattr(repo, "opener")


# utilities to clear cache


def clearfilecache(repo, attrname):
    unfi = repo
    if attrname in vars(unfi):
        delattr(unfi, attrname)
    unfi._filecache.pop(attrname, None)


# perf commands


@command("perfwalk", formatteropts)
def perfwalk(ui, repo, *pats, **opts):
    timer, fm = gettimer(ui, opts)
    m = scmutil.match(repo[None], pats, {})
    timer(lambda: len(list(set().union(*repo.dirstate.status(m, False, True, False)))))
    fm.end()


@command("perfannotate", formatteropts)
def perfannotate(ui, repo, f, **opts):
    timer, fm = gettimer(ui, opts)
    fc = repo["."][f]
    timer(lambda: len(list(fc.annotate(True))))
    fm.end()


@command("perfdatapack", formatteropts)
def perfdatapack(ui, repo, packpath, **opts):
    from edenscm.ext.remotefilelog.datapack import datapack

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
    from edenscm.ext.remotefilelog.repack import repackledger

    def f(pack):
        ledger = repackledger()
        pack.markledger(ledger, None)

    _packtestfn(ui, packpath, opts, f)


def _packtestfn(ui, packpath, opts, func):
    from bindings import revisionstore
    from edenscm.ext.remotefilelog.datapack import datapack, fastdatapack

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
    headrevs = repo.headrevs

    def d():
        len(headrevs())
        clearcaches(cl)

    timer(d)
    fm.end()


@command("perfancestors", formatteropts)
def perfancestors(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    heads = repo.headrevs()

    def d():
        for a in repo.changelog.ancestors(heads):
            pass

    timer(d)
    fm.end()


@command("perfancestorset", formatteropts)
def perfancestorset(ui, repo, revset, **opts):
    timer, fm = gettimer(ui, opts)
    revs = repo.revs(revset)
    heads = repo.headrevs()

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


@command("perfdirs", formatteropts)
def perfdirs(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    dirstate = repo.dirstate
    "a" in dirstate

    def d():
        dirstate.hasdir("a")

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

    timer(d)
    fm.end()


@command("perfdirstatefoldmap", formatteropts)
def perfdirstatefoldmap(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    dirstate = repo.dirstate
    "a" in dirstate

    def d():
        dirstate._map.filefoldmap.get("a")
        dirstate._map.filefoldmap.clear()

    timer(d)
    fm.end()


@command("perfdirfoldmap", formatteropts)
def perfdirfoldmap(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    dirstate = repo.dirstate
    "a" in dirstate

    def d():
        dirstate._map.dirfoldmap.get("a")
        dirstate._map.dirfoldmap.clear()

    timer(d)
    fm.end()


@command("perfdirstatewrite", formatteropts)
def perfdirstatewrite(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    ds = repo.dirstate
    "a" in ds

    def d():
        with repo.wlock(), repo.lock(), repo.transaction("perf"):
            ds._markforwrite()

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
    revlog._prereadsize = 2**24  # disable lazy parser in old hg
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
    revlog._prereadsize = 2**24  # disable lazy parser in old hg
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
    ui.popbufferbytes()
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
    "perflrucachedict|perflrucache",
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
    for i in range(size):
        values.append(random.randint(0, sys.maxsize))

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
    for i in range(sets):
        setseq.append(random.randint(0, sys.maxsize))

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
    """microbenchmark ui.write"""
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
