# perf.py - performance test routines
'''helper extension to measure performance'''

from mercurial import cmdutil, scmutil, match, commands
import time, os, sys

def timer(func, title=None):
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
    if title:
        sys.stderr.write("! %s\n" % title)
    if r:
        sys.stderr.write("! result: %s\n" % r)
    m = min(results)
    sys.stderr.write("! wall %f comb %f user %f sys %f (best of %d)\n"
                     % (m[0], m[1] + m[2], m[1], m[2], count))

def perfwalk(ui, repo, *pats):
    try:
        m = scmutil.match(repo[None], pats, {})
        timer(lambda: len(list(repo.dirstate.walk(m, [], True, False))))
    except:
        try:
            m = scmutil.match(repo[None], pats, {})
            timer(lambda: len([b for a, b, c in repo.dirstate.statwalk([], m)]))
        except:
            timer(lambda: len(list(cmdutil.walk(repo, pats, {}))))

def perfstatus(ui, repo, *pats):
    #m = match.always(repo.root, repo.getcwd())
    #timer(lambda: sum(map(len, repo.dirstate.status(m, [], False, False, False))))
    timer(lambda: sum(map(len, repo.status())))

def perfheads(ui, repo):
    timer(lambda: len(repo.changelog.heads()))

def perftags(ui, repo):
    import mercurial.changelog, mercurial.manifest
    def t():
        repo.changelog = mercurial.changelog.changelog(repo.sopener)
        repo.manifest = mercurial.manifest.manifest(repo.sopener)
        repo._tags = None
        return len(repo.tags())
    timer(t)

def perfdirstate(ui, repo):
    "a" in repo.dirstate
    def d():
        repo.dirstate.invalidate()
        "a" in repo.dirstate
    timer(d)

def perfdirstatedirs(ui, repo):
    "a" in repo.dirstate
    def d():
        "a" in repo.dirstate._dirs
        del repo.dirstate._dirs
    timer(d)

def perfmanifest(ui, repo):
    def d():
        t = repo.manifest.tip()
        m = repo.manifest.read(t)
        repo.manifest.mapcache = None
        repo.manifest._cache = None
    timer(d)

def perfindex(ui, repo):
    import mercurial.revlog
    mercurial.revlog._prereadsize = 2**24 # disable lazy parser in old hg
    n = repo["tip"].node()
    def d():
        repo.invalidate()
        repo[n]
    timer(d)

def perfstartup(ui, repo):
    cmd = sys.argv[0]
    def d():
        os.system("HGRCPATH= %s version -q > /dev/null" % cmd)
    timer(d)

def perfparents(ui, repo):
    nl = [repo.changelog.node(i) for i in xrange(1000)]
    def d():
        for n in nl:
            repo.changelog.parents(n)
    timer(d)

def perflookup(ui, repo, rev):
    timer(lambda: len(repo.lookup(rev)))

def perflog(ui, repo, **opts):
    ui.pushbuffer()
    timer(lambda: commands.log(ui, repo, rev=[], date='', user='',
                               copies=opts.get('rename')))
    ui.popbuffer()

def perftemplating(ui, repo):
    ui.pushbuffer()
    timer(lambda: commands.log(ui, repo, rev=[], date='', user='',
                               template='{date|shortdate} [{rev}:{node|short}]'
                               ' {author|person}: {desc|firstline}\n'))
    ui.popbuffer()

def perfdiffwd(ui, repo):
    """Profile diff of working directory changes"""
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

def perfrevlog(ui, repo, file_, **opts):
    from mercurial import revlog
    dist = opts['dist']
    def d():
        r = revlog.revlog(lambda fn: open(fn, 'rb'), file_)
        for x in xrange(0, len(r), dist):
            r.revision(r.node(x))

    timer(d)

cmdtable = {
    'perflookup': (perflookup, []),
    'perfparents': (perfparents, []),
    'perfstartup': (perfstartup, []),
    'perfstatus': (perfstatus, []),
    'perfwalk': (perfwalk, []),
    'perfmanifest': (perfmanifest, []),
    'perfindex': (perfindex, []),
    'perfheads': (perfheads, []),
    'perftags': (perftags, []),
    'perfdirstate': (perfdirstate, []),
    'perfdirstatedirs': (perfdirstate, []),
    'perflog': (perflog,
                [('', 'rename', False, 'ask log to follow renames')]),
    'perftemplating': (perftemplating, []),
    'perfdiffwd': (perfdiffwd, []),
    'perfrevlog': (perfrevlog,
                   [('d', 'dist', 100, 'distance between the revisions')],
                   "[INDEXFILE]"),
}
