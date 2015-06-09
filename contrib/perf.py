# perf.py - performance test routines
'''helper extension to measure performance'''

from mercurial import cmdutil, scmutil, util, commands, obsolete
from mercurial import repoview, branchmap, merge, copies
import time, os, sys
import functools

formatteropts = commands.formatteropts

cmdtable = {}
command = cmdutil.command(cmdtable)

def gettimer(ui, opts=None):
    """return a timer function and formatter: (timer, formatter)

    This functions exist to gather the creation of formatter in a single
    place instead of duplicating it in all performance command."""

    # enforce an idle period before execution to counteract power management
    time.sleep(ui.configint("perf", "presleep", 1))

    if opts is None:
        opts = {}
    # redirect all to stderr
    ui = ui.copy()
    ui.fout = ui.ferr
    # get a formatter
    fm = ui.formatter('perf', opts)
    return functools.partial(_timer, fm), fm

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
    timer, fm = gettimer(ui, **opts)
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
def perffilefoldmap(ui, repo, **opts):
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
        ds.write()
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
        merge.calculateupdates(repo, wctx, rctx, ancestor, False, False, False,
                               acceptremote=True)
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
        repo.manifest._mancache.clear()
        repo.manifest._cache = None
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
        os.system("HGRCPATH= %s version -q > /dev/null" % cmd)
    timer(d)
    fm.end()

@command('perfparents', formatteropts)
def perfparents(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    nl = [repo.changelog.node(i) for i in xrange(1000)]
    def d():
        for n in nl:
            repo.changelog.parents(n)
    timer(d)
    fm.end()

@command('perfctxfiles', formatteropts)
def perfparents(ui, repo, x, **opts):
    x = int(x)
    timer, fm = gettimer(ui, opts)
    def d():
        len(repo[x].files())
    timer(d)
    fm.end()

@command('perfrawfiles', formatteropts)
def perfparents(ui, repo, x, **opts):
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
def perflog(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    ui.pushbuffer()
    timer(lambda: commands.log(ui, repo, rev=[], date='', user='',
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
def perftemplating(ui, repo, **opts):
    timer, fm = gettimer(ui, opts)
    ui.pushbuffer()
    timer(lambda: commands.log(ui, repo, rev=[], date='', user='',
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
    def d():
        s.fncache._dirty = True
        s.fncache.write()
    timer(d)
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

@command('perfrevlog',
         [('d', 'dist', 100, 'distance between the revisions')] + formatteropts,
         "[INDEXFILE]")
def perfrevlog(ui, repo, file_, **opts):
    timer, fm = gettimer(ui, opts)
    from mercurial import revlog
    dist = opts['dist']
    def d():
        r = revlog.revlog(lambda fn: open(fn, 'rb'), file_)
        for x in xrange(0, len(r), dist):
            r.revision(r.node(x))

    timer(d)
    fm.end()

@command('perfrevset',
         [('C', 'clear', False, 'clear volatile cache between each call.')]
         + formatteropts, "REVSET")
def perfrevset(ui, repo, expr, clear=False, **opts):
    """benchmark the execution time of a revset

    Use the --clean option if need to evaluate the impact of build volatile
    revisions set cache on the revset execution. Volatile cache hold filtered
    and obsolete related cache."""
    timer, fm = gettimer(ui, opts)
    def d():
        if clear:
            repo.invalidatevolatilesets()
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
