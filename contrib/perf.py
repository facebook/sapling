# perf.py - performance test routines
'''helper extension to measure performance'''

from __future__ import absolute_import
import functools
import os
import random
import sys
import time
from mercurial import (
    branchmap,
    cmdutil,
    commands,
    copies,
    error,
    mdiff,
    merge,
    obsolete,
    repoview,
    revlog,
    scmutil,
    util,
)

formatteropts = commands.formatteropts
revlogopts = commands.debugrevlogopts

cmdtable = {}
command = cmdutil.command(cmdtable)

def getlen(ui):
    if ui.configbool("perf", "stub"):
        return lambda x: 1
    return len

def gettimer(ui, opts=None):
    """return a timer function and formatter: (timer, formatter)

    This function exists to gather the creation of formatter in a single
    place instead of duplicating it in all performance commands."""

    # enforce an idle period before execution to counteract power management
    # experimental config: perf.presleep
    time.sleep(ui.configint("perf", "presleep", 1))

    if opts is None:
        opts = {}
    # redirect all to stderr
    ui = ui.copy()
    ui.fout = ui.ferr
    # get a formatter
    fm = ui.formatter('perf', opts)
    # stub function, runs code only once instead of in a loop
    # experimental config: perf.stub
    if ui.configbool("perf", "stub"):
        return functools.partial(stub_timer, fm), fm
    return functools.partial(_timer, fm), fm

def stub_timer(fm, func, title=None):
    func()

def _timer(fm, func, title=None):
    results = []
    begin = time.time()
    count = 0
    while True:
        ostart = os.times()
        cstart = time.time()
        r = func()
        cstop = time.time()
        ostop = os.times()
        count += 1
        a, b = ostart, ostop
        results.append((cstop - cstart, b[0] - a[0], b[1]-a[1]))
        if cstop - begin > 3 and count >= 100:
            break
        if cstop - begin > 10 and count >= 3:
            break

    fm.startitem()

    if title:
        fm.write('title', '! %s\n', title)
    if r:
        fm.write('result', '! result: %s\n', r)
    m = min(results)
    fm.plain('!')
    fm.write('wall', ' wall %f', m[0])
    fm.write('comb', ' comb %f', m[1] + m[2])
    fm.write('user', ' user %f', m[1])
    fm.write('sys',  ' sys %f', m[2])
    fm.write('count',  ' (best of %d)', count)
    fm.plain('\n')

@command('perfwalk', formatteropts)
def perfwalk(ui, repo, *pats, **opts):
    timer, fm = gettimer(ui, opts)
    try:
        m = scmutil.match(repo[None], pats, {})
        timer(lambda: len(list(repo.dirstate.walk(m, [], True, False))))
    except Exception:
        try:
            m = scmutil.match(repo[None], pats, {})
            timer(lambda: len([b for a, b, c in repo.dirstate.statwalk([], m)]))
        except Exception:
            timer(lambda: len(list(cmdutil.walk(repo, pats, {}))))
    fm.end()

@command('perfannotate', formatteropts)
def perfannotate(ui, repo, f, **opts):
    timer, fm = gettimer(ui, opts)
    fc = repo['.'][f]
    timer(lambda: len(fc.annotate(True)))
    fm.end()

@command('perfstatus',
         [('u', 'unknown', False,
           'ask status to look for unknown files')] + formatteropts)
def perfstatus(ui, repo, **opts):
    #m = match.always(repo.root, repo.getcwd())
    #timer(lambda: sum(map(len, repo.dirstate.status(m, [], False, False,
    #                                                False))))
    timer, fm = gettimer(ui, opts)
    timer(lambda: sum(map(len, repo.status(unknown=opts['unknown']))))
    fm.end()

@command('perfaddremove', formatteropts)
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
    if util.safehasattr(cl, 'clearcaches'):
        cl.clearcaches()
    elif util.safehasattr(cl, '_nodecache'):
        from mercurial.node import nullid, nullrev
        cl._nodecache = {nullid: nullrev}
        cl._nodepos = None

@command('perfheads', formatteropts)
def perfheads(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    cl = repo.changelog
    def d():
        len(cl.headrevs())
        clearcaches(cl)
    timer(d)
    fm.end()

@command('perftags', formatteropts)
def perftags(ui, repo, **opts):
    import mercurial.changelog
    import mercurial.manifest
    timer, fm = gettimer(ui, opts)
    def t():
        repo.changelog = mercurial.changelog.changelog(repo.svfs)
        repo.manifest = mercurial.manifest.manifest(repo.svfs)
        repo._tags = None
        return len(repo.tags())
    timer(t)
    fm.end()

@command('perfancestors', formatteropts)
def perfancestors(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    heads = repo.changelog.headrevs()
    def d():
        for a in repo.changelog.ancestors(heads):
            pass
    timer(d)
    fm.end()

@command('perfancestorset', formatteropts)
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

@command('perfdirs', formatteropts)
def perfdirs(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    dirstate = repo.dirstate
    'a' in dirstate
    def d():
        dirstate.dirs()
        del dirstate._dirs
    timer(d)
    fm.end()

@command('perfdirstate', formatteropts)
def perfdirstate(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    "a" in repo.dirstate
    def d():
        repo.dirstate.invalidate()
        "a" in repo.dirstate
    timer(d)
    fm.end()

@command('perfdirstatedirs', formatteropts)
def perfdirstatedirs(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    "a" in repo.dirstate
    def d():
        "a" in repo.dirstate._dirs
        del repo.dirstate._dirs
    timer(d)
    fm.end()

@command('perfdirstatefoldmap', formatteropts)
def perfdirstatefoldmap(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    dirstate = repo.dirstate
    'a' in dirstate
    def d():
        dirstate._filefoldmap.get('a')
        del dirstate._filefoldmap
    timer(d)
    fm.end()

@command('perfdirfoldmap', formatteropts)
def perfdirfoldmap(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    dirstate = repo.dirstate
    'a' in dirstate
    def d():
        dirstate._dirfoldmap.get('a')
        del dirstate._dirfoldmap
        del dirstate._dirs
    timer(d)
    fm.end()

@command('perfdirstatewrite', formatteropts)
def perfdirstatewrite(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    ds = repo.dirstate
    "a" in ds
    def d():
        ds._dirty = True
        ds.write(repo.currenttransaction())
    timer(d)
    fm.end()

@command('perfmergecalculate',
         [('r', 'rev', '.', 'rev to merge against')] + formatteropts)
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
        merge.calculateupdates(repo, wctx, rctx, [ancestor], False, False,
                               acceptremote=True, followcopies=True)
    timer(d)
    fm.end()

@command('perfpathcopies', [], "REV REV")
def perfpathcopies(ui, repo, rev1, rev2, **opts):
    timer, fm = gettimer(ui, opts)
    ctx1 = scmutil.revsingle(repo, rev1, rev1)
    ctx2 = scmutil.revsingle(repo, rev2, rev2)
    def d():
        copies.pathcopies(ctx1, ctx2)
    timer(d)
    fm.end()

@command('perfmanifest', [], 'REV')
def perfmanifest(ui, repo, rev, **opts):
    timer, fm = gettimer(ui, opts)
    ctx = scmutil.revsingle(repo, rev, rev)
    t = ctx.manifestnode()
    def d():
        repo.manifest.clearcaches()
        repo.manifest.read(t)
    timer(d)
    fm.end()

@command('perfchangeset', formatteropts)
def perfchangeset(ui, repo, rev, **opts):
    timer, fm = gettimer(ui, opts)
    n = repo[rev].node()
    def d():
        repo.changelog.read(n)
        #repo.changelog._cache = None
    timer(d)
    fm.end()

@command('perfindex', formatteropts)
def perfindex(ui, repo, **opts):
    import mercurial.revlog
    timer, fm = gettimer(ui, opts)
    mercurial.revlog._prereadsize = 2**24 # disable lazy parser in old hg
    n = repo["tip"].node()
    def d():
        cl = mercurial.revlog.revlog(repo.svfs, "00changelog.i")
        cl.rev(n)
    timer(d)
    fm.end()

@command('perfstartup', formatteropts)
def perfstartup(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    cmd = sys.argv[0]
    def d():
        if os.name != 'nt':
            os.system("HGRCPATH= %s version -q > /dev/null" % cmd)
        else:
            os.environ['HGRCPATH'] = ''
            os.system("%s version -q > NUL" % cmd)
    timer(d)
    fm.end()

@command('perfparents', formatteropts)
def perfparents(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    # control the number of commits perfparents iterates over
    # experimental config: perf.parentscount
    count = ui.configint("perf", "parentscount", 1000)
    if len(repo.changelog) < count:
        raise error.Abort("repo needs %d commits for this test" % count)
    repo = repo.unfiltered()
    nl = [repo.changelog.node(i) for i in xrange(count)]
    def d():
        for n in nl:
            repo.changelog.parents(n)
    timer(d)
    fm.end()

@command('perfctxfiles', formatteropts)
def perfctxfiles(ui, repo, x, **opts):
    x = int(x)
    timer, fm = gettimer(ui, opts)
    def d():
        len(repo[x].files())
    timer(d)
    fm.end()

@command('perfrawfiles', formatteropts)
def perfrawfiles(ui, repo, x, **opts):
    x = int(x)
    timer, fm = gettimer(ui, opts)
    cl = repo.changelog
    def d():
        len(cl.read(x)[3])
    timer(d)
    fm.end()

@command('perflookup', formatteropts)
def perflookup(ui, repo, rev, **opts):
    timer, fm = gettimer(ui, opts)
    timer(lambda: len(repo.lookup(rev)))
    fm.end()

@command('perfrevrange', formatteropts)
def perfrevrange(ui, repo, *specs, **opts):
    timer, fm = gettimer(ui, opts)
    revrange = scmutil.revrange
    timer(lambda: len(revrange(repo, specs)))
    fm.end()

@command('perfnodelookup', formatteropts)
def perfnodelookup(ui, repo, rev, **opts):
    timer, fm = gettimer(ui, opts)
    import mercurial.revlog
    mercurial.revlog._prereadsize = 2**24 # disable lazy parser in old hg
    n = repo[rev].node()
    cl = mercurial.revlog.revlog(repo.svfs, "00changelog.i")
    def d():
        cl.rev(n)
        clearcaches(cl)
    timer(d)
    fm.end()

@command('perflog',
         [('', 'rename', False, 'ask log to follow renames')] + formatteropts)
def perflog(ui, repo, rev=None, **opts):
    if rev is None:
        rev=[]
    timer, fm = gettimer(ui, opts)
    ui.pushbuffer()
    timer(lambda: commands.log(ui, repo, rev=rev, date='', user='',
                               copies=opts.get('rename')))
    ui.popbuffer()
    fm.end()

@command('perfmoonwalk', formatteropts)
def perfmoonwalk(ui, repo, **opts):
    """benchmark walking the changelog backwards

    This also loads the changelog data for each revision in the changelog.
    """
    timer, fm = gettimer(ui, opts)
    def moonwalk():
        for i in xrange(len(repo), -1, -1):
            ctx = repo[i]
            ctx.branch() # read changelog data (in addition to the index)
    timer(moonwalk)
    fm.end()

@command('perftemplating', formatteropts)
def perftemplating(ui, repo, rev=None, **opts):
    if rev is None:
        rev=[]
    timer, fm = gettimer(ui, opts)
    ui.pushbuffer()
    timer(lambda: commands.log(ui, repo, rev=rev, date='', user='',
                               template='{date|shortdate} [{rev}:{node|short}]'
                               ' {author|person}: {desc|firstline}\n'))
    ui.popbuffer()
    fm.end()

@command('perfcca', formatteropts)
def perfcca(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    timer(lambda: scmutil.casecollisionauditor(ui, False, repo.dirstate))
    fm.end()

@command('perffncacheload', formatteropts)
def perffncacheload(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    s = repo.store
    def d():
        s.fncache._load()
    timer(d)
    fm.end()

@command('perffncachewrite', formatteropts)
def perffncachewrite(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    s = repo.store
    s.fncache._load()
    lock = repo.lock()
    tr = repo.transaction('perffncachewrite')
    def d():
        s.fncache._dirty = True
        s.fncache.write(tr)
    timer(d)
    lock.release()
    tr.close()
    fm.end()

@command('perffncacheencode', formatteropts)
def perffncacheencode(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    s = repo.store
    s.fncache._load()
    def d():
        for p in s.fncache.entries:
            s.encode(p)
    timer(d)
    fm.end()

@command('perfdiffwd', formatteropts)
def perfdiffwd(ui, repo, **opts):
    """Profile diff of working directory changes"""
    timer, fm = gettimer(ui, opts)
    options = {
        'w': 'ignore_all_space',
        'b': 'ignore_space_change',
        'B': 'ignore_blank_lines',
        }

    for diffopt in ('', 'w', 'b', 'B', 'wB'):
        opts = dict((options[c], '1') for c in diffopt)
        def d():
            ui.pushbuffer()
            commands.diff(ui, repo, **opts)
            ui.popbuffer()
        title = 'diffopts: %s' % (diffopt and ('-' + diffopt) or 'none')
        timer(d, title)
    fm.end()

@command('perfrevlog', revlogopts + formatteropts +
         [('d', 'dist', 100, 'distance between the revisions'),
          ('s', 'startrev', 0, 'revision to start reading at')],
         '-c|-m|FILE')
def perfrevlog(ui, repo, file_=None, startrev=0, **opts):
    """Benchmark reading a series of revisions from a revlog.

    By default, we read every ``-d/--dist`` revision from 0 to tip of
    the specified revlog.

    The start revision can be defined via ``-s/--startrev``.
    """
    timer, fm = gettimer(ui, opts)
    dist = opts['dist']
    _len = getlen(ui)
    def d():
        r = cmdutil.openrevlog(repo, 'perfrevlog', file_, opts)
        for x in xrange(startrev, _len(r), dist):
            r.revision(r.node(x))

    timer(d)
    fm.end()

@command('perfrevlogrevision', revlogopts + formatteropts +
         [('', 'cache', False, 'use caches instead of clearing')],
         '-c|-m|FILE REV')
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
    if opts.get('changelog') or opts.get('manifest'):
        file_, rev = None, file_
    elif rev is None:
        raise error.CommandError('perfrevlogrevision', 'invalid arguments')

    r = cmdutil.openrevlog(repo, 'perfrevlogrevision', file_, opts)
    node = r.lookup(rev)
    rev = r.rev(node)

    def dodeltachain(rev):
        if not cache:
            r.clearcaches()
        r._deltachain(rev)

    def doread(chain):
        if not cache:
            r.clearcaches()
        r._chunkraw(chain[0], chain[-1])

    def dodecompress(data, chain):
        if not cache:
            r.clearcaches()

        start = r.start
        length = r.length
        inline = r._inline
        iosize = r._io.size
        buffer = util.buffer
        offset = start(chain[0])

        for rev in chain:
            chunkstart = start(rev)
            if inline:
                chunkstart += (rev + 1) * iosize
            chunklength = length(rev)
            b = buffer(data, chunkstart - offset, chunklength)
            revlog.decompress(b)

    def dopatch(text, bins):
        if not cache:
            r.clearcaches()
        mdiff.patches(text, bins)

    def dohash(text):
        if not cache:
            r.clearcaches()
        r._checkhash(text, node, rev)

    def dorevision():
        if not cache:
            r.clearcaches()
        r.revision(node)

    chain = r._deltachain(rev)[0]
    data = r._chunkraw(chain[0], chain[-1])[1]
    bins = r._chunks(chain)
    text = str(bins[0])
    bins = bins[1:]
    text = mdiff.patches(text, bins)

    benches = [
        (lambda: dorevision(), 'full'),
        (lambda: dodeltachain(rev), 'deltachain'),
        (lambda: doread(chain), 'read'),
        (lambda: dodecompress(data, chain), 'decompress'),
        (lambda: dopatch(text, bins), 'patch'),
        (lambda: dohash(text), 'hash'),
    ]

    for fn, title in benches:
        timer, fm = gettimer(ui, opts)
        timer(fn, title=title)
        fm.end()

@command('perfrevset',
         [('C', 'clear', False, 'clear volatile cache between each call.'),
          ('', 'contexts', False, 'obtain changectx for each revision')]
         + formatteropts, "REVSET")
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
            for ctx in repo.set(expr): pass
        else:
            for r in repo.revs(expr): pass
    timer(d)
    fm.end()

@command('perfvolatilesets', formatteropts)
def perfvolatilesets(ui, repo, *names, **opts):
    """benchmark the computation of various volatile set

    Volatile set computes element related to filtering and obsolescence."""
    timer, fm = gettimer(ui, opts)
    repo = repo.unfiltered()

    def getobs(name):
        def d():
            repo.invalidatevolatilesets()
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
            repoview.filterrevs(repo, name)
        return d

    allfilter = sorted(repoview.filtertable)
    if names:
        allfilter = [n for n in allfilter if n in names]

    for name in allfilter:
        timer(getfiltered(name), title=name)
    fm.end()

@command('perfbranchmap',
         [('f', 'full', False,
           'Includes build time of subset'),
         ] + formatteropts)
def perfbranchmap(ui, repo, full=False, **opts):
    """benchmark the update of a branchmap

    This benchmarks the full repo.branchmap() call with read and write disabled
    """
    timer, fm = gettimer(ui, opts)
    def getbranchmap(filtername):
        """generate a benchmark function for the filtername"""
        if filtername is None:
            view = repo
        else:
            view = repo.filtered(filtername)
        def d():
            if full:
                view._branchcaches.clear()
            else:
                view._branchcaches.pop(filtername, None)
            view.branchmap()
        return d
    # add filter in smaller subset to bigger subset
    possiblefilters = set(repoview.filtertable)
    allfilters = []
    while possiblefilters:
        for name in possiblefilters:
            subset = branchmap.subsettable.get(name)
            if subset not in possiblefilters:
                break
        else:
            assert False, 'subset cycle %s!' % possiblefilters
        allfilters.append(name)
        possiblefilters.remove(name)

    # warm the cache
    if not full:
        for name in allfilters:
            repo.filtered(name).branchmap()
    # add unfiltered
    allfilters.append(None)
    oldread = branchmap.read
    oldwrite = branchmap.branchcache.write
    try:
        branchmap.read = lambda repo: None
        branchmap.write = lambda repo: None
        for name in allfilters:
            timer(getbranchmap(name), title=str(name))
    finally:
        branchmap.read = oldread
        branchmap.branchcache.write = oldwrite
    fm.end()

@command('perfloadmarkers')
def perfloadmarkers(ui, repo):
    """benchmark the time to parse the on-disk markers for a repo

    Result is the number of markers in the repo."""
    timer, fm = gettimer(ui)
    timer(lambda: len(obsolete.obsstore(repo.svfs)))
    fm.end()

@command('perflrucachedict', formatteropts +
    [('', 'size', 4, 'size of cache'),
     ('', 'gets', 10000, 'number of key lookups'),
     ('', 'sets', 10000, 'number of key sets'),
     ('', 'mixed', 10000, 'number of mixed mode operations'),
     ('', 'mixedgetfreq', 50, 'frequency of get vs set ops in mixed mode')],
    norepo=True)
def perflrucache(ui, size=4, gets=10000, sets=10000, mixed=10000,
                 mixedgetfreq=50, **opts):
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
            value # silence pyflakes warning

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

    benches = [
        (doinit, 'init'),
        (dogets, 'gets'),
        (dosets, 'sets'),
        (domixed, 'mixed')
    ]

    for fn, title in benches:
        timer, fm = gettimer(ui, opts)
        timer(fn, title=title)
        fm.end()
